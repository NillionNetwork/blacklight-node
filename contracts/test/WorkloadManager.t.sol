// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "./helpers/RCFixture.sol";
import "../src/mocks/MockERC20.sol";
import "../src/RewardPolicy.sol";

contract WorkloadManagerTest is RCFixture {
    function setUp() public {
        uint256[] memory stakes = new uint256[](12);
        for (uint256 i = 0; i < stakes.length; i++) stakes[i] = 2e18;
        _deploySystem(
            12,
            stakes,
            10,   // baseCommitteeSize
            10,   // maxCommitteeSize
            5000, // quorumBps
            5000, // verificationBps
            1 days,
            7 days,
            1      // maxEscalations
        );
    }

    function _findPk(address op) internal view returns (uint256) {
        for (uint256 i = 0; i < ops.length; i++) {
            if (ops[i] == op) return opPks[i];
        }
        revert("pk not found");
    }

    function test_submitWorkload_startsRoundAndSetsPending() public {
        (bytes32 wk, uint8 round, bytes32 root, uint64 snap, address[] memory members) = _submitPointerAndGetRound();
        assertEq(round, 1);
        assertTrue(root != bytes32(0));
        assertTrue(snap != 0);
        assertEq(members.length, 10);

        (WorkloadManager.WorkloadStatus status, uint8 currentRound, , , , ) = manager.workloads(wk);
        assertEq(uint8(status), uint8(WorkloadManager.WorkloadStatus.Pending));
        assertEq(currentRound, 1);

        // members are sorted ascending
        for (uint256 i = 1; i < members.length; i++) {
            assertTrue(members[i-1] < members[i], "not sorted");
        }

        // round info snapshot addresses must be set
        ( , , , , , , uint32 committeeSize, uint64 snapshotId, bytes32 committeeRoot, , , , address stakingAddr, address selectorAddr, address slashingAddr, address rewardAddr, , , , ) =
            manager.rounds(wk, 1);

        assertEq(committeeSize, 10);
        assertEq(snapshotId, snap);
        assertEq(committeeRoot, root);
        assertEq(stakingAddr, address(stakingOps));
        assertEq(selectorAddr, address(selector));
        assertEq(slashingAddr, address(jailingPolicy));
        assertEq(rewardAddr, address(rewardPolicy));
    }

    function test_submitWorkload_idempotent_noSecondRoundStarted() public {
        bytes memory rawHTX = _defaultRawHTX(1);
        bytes32 wk = manager.deriveWorkloadKey(rawHTX);
        (uint64 snap, ) = _prepareCommittee(wk, 1, 0);

        vm.recordLogs();
        manager.submitWorkload(rawHTX, snap);
        manager.submitWorkload(rawHTX, snap);

        Vm.Log[] memory logs = vm.getRecordedLogs();
        bytes32 sig = keccak256("RoundStarted(bytes32,uint8,bytes32,uint64,uint64,uint64,address[],bytes)");

        uint256 startedCount;
        for (uint256 i = 0; i < logs.length; i++) {
            if (logs[i].topics.length > 0 && logs[i].topics[0] == sig && bytes32(logs[i].topics[1]) == wk) {
                startedCount++;
            }
        }
        assertEq(startedCount, 1, "round started twice");
    }

    function test_submitVerdict_revertsForNonMember() public {
        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        address notMember = address(0x9999);
        vm.prank(notMember);
        vm.expectRevert(WorkloadManager.NotInCommittee.selector);
        manager.submitVerdict(wk, 1, new bytes32[](0));
        assertEq(round, 1);
        assertEq(members.length, 10);
    }

    function test_submitVerdict_revertsAfterDeadline() public {
        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        // take first member
        address voter = members[0];
        bytes32[] memory proof = _proofForMember(wk, round, members, voter);

        // warp past deadline
        (, , , , , , , , , , uint64 deadline, , , , , , , , , ) = manager.rounds(wk, round);
        vm.warp(uint256(deadline) + 1);

        vm.prank(voter);
        vm.expectRevert(WorkloadManager.RoundClosed.selector);
        manager.submitVerdict(wk, 1, proof);
    }

    function test_doubleVote_reverts() public {
        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        address voter = members[0];
        _vote(wk, round, members, voter, 1);

        bytes32[] memory proof = _proofForMember(wk, round, members, voter);
        vm.prank(voter);
        vm.expectRevert(WorkloadManager.AlreadyResponded.selector);
        manager.submitVerdict(wk, 1, proof);
    }

    function test_finalize_validThreshold_updatesStatus() public {
        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();

        // 5 votes out of 10 => 50% quorum + 50% valid threshold
        for (uint256 i = 0; i < 5; i++) {
            _vote(wk, round, members, members[i], 1);
        }

        assertEq(uint8(manager.roundOutcome(wk, round)), uint8(ISlashingPolicy.Outcome.ValidThreshold));
        (WorkloadManager.WorkloadStatus status, , , , , ) = manager.workloads(wk);
        assertEq(uint8(status), uint8(WorkloadManager.WorkloadStatus.Verified));
    }

    function test_finalize_invalidThreshold_updatesStatus() public {
        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();

        for (uint256 i = 0; i < 5; i++) {
            _vote(wk, round, members, members[i], 2);
        }

        assertEq(uint8(manager.roundOutcome(wk, round)), uint8(ISlashingPolicy.Outcome.InvalidThreshold));
        (WorkloadManager.WorkloadStatus status, , , , , ) = manager.workloads(wk);
        assertEq(uint8(status), uint8(WorkloadManager.WorkloadStatus.Invalid));
    }

    function test_escalateOrExpire_beforeDeadline_reverts() public {
        (bytes32 wk, , , , ) = _submitPointerAndGetRound();
        vm.expectRevert(WorkloadManager.BeforeDeadline.selector);
        manager.escalateOrExpire(wk, _defaultRawHTX(1));
    }

    function test_escalateOrExpire_inconclusive_startsNewRound_and_then_expires() public {
        // quorum requires 50%; only 1 vote => inconclusive
        (bytes32 wk, uint8 round1, , , address[] memory members1) = _submitPointerAndGetRound();
        _vote(wk, round1, members1, members1[0], 1);

        (, , , , , , , , , , uint64 deadline1, , , , , , , , , ) = manager.rounds(wk, round1);
        vm.warp(uint256(deadline1) + 1);

        vm.recordLogs();
        manager.escalateOrExpire(wk, _defaultRawHTX(1));
        Vm.Log[] memory logs = vm.getRecordedLogs();

        // round 1 finalized inconclusive and round2 started
        assertEq(uint8(manager.roundOutcome(wk, round1)), uint8(ISlashingPolicy.Outcome.Inconclusive));

        (, uint8 currentRound, uint8 escalationLevel, , , ) = manager.workloads(wk);
        assertEq(currentRound, 2);
        assertEq(escalationLevel, 1);

        // parse round2 started
        bytes32 sig = keccak256("RoundStarted(bytes32,uint8,bytes32,uint64,uint64,uint64,address[],bytes)");
        bool found;
        for (uint256 i = 0; i < logs.length; i++) {
            if (logs[i].topics.length > 0 && logs[i].topics[0] == sig && bytes32(logs[i].topics[1]) == wk) {
                (uint8 r2,, , , , , ) = abi.decode(logs[i].data, (uint8, bytes32, uint64, uint64, uint64, address[], bytes));
                if (r2 == 2) found = true;
            }
        }
        assertTrue(found, "round2 not started");

        // expire after round2 deadline with no quorum
        (, , , , , , , , , , uint64 deadline2, , , , , , , , , ) = manager.rounds(wk, 2);
        vm.warp(uint256(deadline2) + 1);
        manager.escalateOrExpire(wk, _defaultRawHTX(1));

        (WorkloadManager.WorkloadStatus status2, , , , , ) = manager.workloads(wk);
        assertEq(uint8(status2), uint8(WorkloadManager.WorkloadStatus.Expired));
    }

    function test_moduleUpgrade_doesNotAffectExistingRoundRewardAddress() public {
        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();

        for (uint256 i = 0; i < 5; i++) {
            _vote(wk, round, members, members[i], 1);
        }

        // Upgrade reward policy after round start (should not affect this round)
        RewardPolicy newReward = new RewardPolicy(IERC20(address(rewardToken)), address(manager), governance, 1 days, 0);
        config.setModules(address(stakingOps), address(selector), address(jailingPolicy), address(newReward));

        // Fund old reward policy and unlock
        rewardToken.mint(governance, 1000);
        rewardToken.approve(address(rewardPolicy), type(uint256).max);
        rewardPolicy.fund(1000);
        vm.warp(block.timestamp + 2 days);
        rewardPolicy.sync();

        // distribute rewards for valid voters (first 5 members)
        address[] memory voters = new address[](5);
        for (uint256 i = 0; i < 5; i++) voters[i] = members[i];

        manager.distributeRewards(wk, round, voters);

        // old policy has rewards, new one doesn't
        assertGt(rewardPolicy.rewards(voters[0]), 0);
        assertEq(newReward.rewards(voters[0]), 0);
    }

    function test_submitVerdictsBatched_signature() public {
        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        address voter = members[0];
        uint256 pk = _findPk(voter);

        _batchedVote(wk, round, members, pk, voter, 1);

        uint256 packed = manager.getVotePacked(wk, round, voter);
        assertTrue((packed & (1 << 2)) != 0, "not responded");
        assertEq(uint8(packed & 0x3), 1);

        // invalid signature should revert
        bytes32[] memory proof = _proofForMember(wk, round, members, voter);
        bytes32 digest = manager.voteDigest(wk, round, 2);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(uint256(123456), digest);

        WorkloadManager.SignedBatchedVote[] memory batch = new WorkloadManager.SignedBatchedVote[](1);
        batch[0] = WorkloadManager.SignedBatchedVote({
            operator: voter,
            workloadKey: wk,
            round: round,
            verdict: 2,
            memberProof: proof,
            sigV: v,
            sigR: r,
            sigS: s
        });

        vm.expectRevert(WorkloadManager.InvalidSignature.selector);
        manager.submitVerdictsBatched(batch);
    }

    function test_submitVerdictsBatched_respects_configMaxBatch() public {
        // shrink max batch size to 2
        config.setParams(
            config.baseCommitteeSize(),
            config.committeeSizeGrowthBps(),
            config.maxCommitteeSize(),
            config.maxEscalations(),
            config.quorumBps(),
            config.verificationBps(),
            config.responseWindow(),
            config.jailDuration(),
            2,
            config.minOperatorStake()
        );

        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();

        WorkloadManager.SignedBatchedVote[] memory batch = new WorkloadManager.SignedBatchedVote[](3);
        for (uint256 i = 0; i < 3; i++) {
            address voter = members[i];
            uint256 pk = _findPk(voter);
            bytes32[] memory proof = _proofForMember(wk, round, members, voter);
            bytes32 digest = manager.voteDigest(wk, round, 1);
            (uint8 v, bytes32 r, bytes32 s) = vm.sign(pk, digest);

            batch[i] = WorkloadManager.SignedBatchedVote({
                operator: voter,
                workloadKey: wk,
                round: round,
                verdict: 1,
                memberProof: proof,
                sigV: v,
                sigR: r,
                sigS: s
            });
        }

        vm.expectRevert(WorkloadManager.InvalidBatchSize.selector);
        manager.submitVerdictsBatched(batch);
    }

    function test_submitVerdictsBatched_hardLimit500() public {
        WorkloadManager.SignedBatchedVote[] memory batch = new WorkloadManager.SignedBatchedVote[](501);
        vm.expectRevert(WorkloadManager.InvalidBatchSize.selector);
        manager.submitVerdictsBatched(batch);
    }

    function test_distributeRewards_revertsOnUnsortedVoters() public {
        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        for (uint256 i = 0; i < 5; i++) _vote(wk, round, members, members[i], 1);

        // fund rewards
        rewardToken.mint(governance, 1000);
        rewardToken.approve(address(rewardPolicy), type(uint256).max);
        rewardPolicy.fund(1000);
        vm.warp(block.timestamp + 2 days);
        rewardPolicy.sync();

        address[] memory voters = new address[](5);
        for (uint256 i = 0; i < 5; i++) voters[i] = members[4 - i]; // reversed => unsorted

        vm.expectRevert(WorkloadManager.UnsortedVoters.selector);
        manager.distributeRewards(wk, round, voters);
    }

    function test_distributeRewards_revertsOnCountMismatch() public {
        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        for (uint256 i = 0; i < 5; i++) _vote(wk, round, members, members[i], 1);

        rewardToken.mint(governance, 1000);
        rewardToken.approve(address(rewardPolicy), type(uint256).max);
        rewardPolicy.fund(1000);
        vm.warp(block.timestamp + 2 days);
        rewardPolicy.sync();

        address[] memory voters = new address[](4);
        for (uint256 i = 0; i < 4; i++) voters[i] = members[i];

        vm.expectRevert(abi.encodeWithSelector(WorkloadManager.InvalidVoterCount.selector, uint256(4), uint256(5)));
        manager.distributeRewards(wk, round, voters);
    }

    function test_distributeRewards_revertsOnInvalidVoterInList() public {
        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        for (uint256 i = 0; i < 5; i++) _vote(wk, round, members, members[i], 1);

        rewardToken.mint(governance, 1000);
        rewardToken.approve(address(rewardPolicy), type(uint256).max);
        rewardPolicy.fund(1000);
        vm.warp(block.timestamp + 2 days);
        rewardPolicy.sync();

        // include a non-voter (members[5]) while keeping length=5
        address[] memory voters = new address[](5);
        voters[0] = members[0];
        voters[1] = members[1];
        voters[2] = members[2];
        voters[3] = members[3];
        voters[4] = members[5]; // nonvoter

        // sort voters to satisfy sorting precondition and hit invalid voter check
        for (uint256 i = 1; i < voters.length; i++) {
            address key = voters[i];
            int256 j = int256(i) - 1;
            while (j >= 0 && voters[uint256(j)] > key) {
                voters[uint256(j + 1)] = voters[uint256(j)];
                j--;
            }
            voters[uint256(j + 1)] = key;
        }

        vm.expectRevert(WorkloadManager.InvalidVoterInList.selector);
        manager.distributeRewards(wk, round, voters);
    }

    function test_committeeRoot_matchesOffchainMerkleComputation() public {
        (bytes32 wk, uint8 round, bytes32 root, , address[] memory members) = _submitPointerAndGetRound();
        bytes32[] memory leaves = MerkleTestUtils.buildLeaves(address(manager), wk, round, members);
        bytes32 computed = MerkleTestUtils.computeRoot(leaves);
        assertEq(computed, root);

        // proof check for one member
        bytes32[] memory proof = MerkleTestUtils.proofForIndex(leaves, 0);
        // OpenZeppelin MerkleProof expects leaf and root; verification happens inside WorkloadManager anyway, but assert non-empty proof for larger trees
        if (members.length > 1) assertGt(proof.length, 0);
    }
}
