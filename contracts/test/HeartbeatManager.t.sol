// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "./helpers/BlacklightFixture.sol";
import "../src/mocks/MockERC20.sol";
import "../src/RewardPolicy.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/utils/Pausable.sol";

contract HeartbeatManagerTest is BlacklightFixture {
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

    function test_submitHeartbeat_startsRoundAndSetsPending() public {
        (bytes32 hbKey, uint8 round, bytes32 root, uint64 snap, address[] memory members) = _submitPointerAndGetRound();
        assertEq(round, 1);
        assertTrue(root != bytes32(0));
        assertTrue(snap != 0);
        assertEq(members.length, 10);

        (HeartbeatManager.HeartbeatStatus status, uint8 currentRound, , , , ) = manager.heartbeats(hbKey);
        assertEq(uint8(status), uint8(HeartbeatManager.HeartbeatStatus.Pending));
        assertEq(currentRound, 1);

        // members are sorted ascending
        for (uint256 i = 1; i < members.length; i++) {
            assertTrue(members[i-1] < members[i], "not sorted");
        }

        // round info snapshot addresses must be set
        ( , , , , , , uint32 committeeSize, uint64 snapshotId, bytes32 committeeRoot, , , , address stakingAddr, address selectorAddr, address slashingAddr, address rewardAddr, , , , ) =
            manager.rounds(hbKey, 1);

        assertEq(committeeSize, 10);
        assertEq(snapshotId, snap);
        assertEq(committeeRoot, root);
        assertEq(stakingAddr, address(stakingOps));
        assertEq(selectorAddr, address(selector));
        assertEq(slashingAddr, address(jailingPolicy));
        assertEq(rewardAddr, address(rewardPolicy));
    }

    function test_submitHeartbeat_idempotent_noSecondRoundStarted() public {
        bytes memory rawHTX = _defaultRawHTX(1);
        uint64 submissionBlock = uint64(block.number);
        bytes32 hbKey = manager.deriveHeartbeatKey(rawHTX, submissionBlock);
        (uint64 snap, ) = _prepareCommittee(hbKey, 1, 0);

        vm.recordLogs();
        manager.submitHeartbeat(rawHTX, snap);
        manager.submitHeartbeat(rawHTX, snap);

        Vm.Log[] memory logs = vm.getRecordedLogs();
        bytes32 sig = keccak256("RoundStarted(bytes32,uint8,bytes32,uint64,uint64,uint64,address[],bytes)");

        uint256 startedCount;
        for (uint256 i = 0; i < logs.length; i++) {
            if (logs[i].topics.length > 0 && logs[i].topics[0] == sig && bytes32(logs[i].topics[1]) == hbKey) {
                startedCount++;
            }
        }
        assertEq(startedCount, 1, "round started twice");
    }

    function test_submitVerdict_revertsForNonMember() public {
        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        address notMember = address(0x9999);
        vm.prank(notMember);
        vm.expectRevert(HeartbeatManager.NotInCommittee.selector);
        manager.submitVerdict(hbKey, 1, new bytes32[](0));
        assertEq(round, 1);
        assertEq(members.length, 10);
    }

    function test_submitHeartbeat_revertsForInvalidSnapshotId() public {
        bytes memory rawHTX = _defaultRawHTX(123);

        vm.expectRevert(abi.encodeWithSelector(HeartbeatManager.SnapshotBlockUnavailable.selector, uint64(0)));
        manager.submitHeartbeat(rawHTX, 0);

        uint64 future = uint64(block.number);
        vm.expectRevert(abi.encodeWithSelector(HeartbeatManager.SnapshotBlockUnavailable.selector, future));
        manager.submitHeartbeat(rawHTX, future);
    }

    function test_submitVerdict_revertsAfterDeadline() public {
        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        // take first member
        address voter = members[0];
        bytes32[] memory proof = _proofForMember(hbKey, round, members, voter);

        // warp past deadline
        (, , , , , , , , , , uint64 deadline, , , , , , , , , ) = manager.rounds(hbKey, round);
        vm.warp(uint256(deadline) + 1);

        vm.prank(voter);
        vm.expectRevert(HeartbeatManager.RoundClosed.selector);
        manager.submitVerdict(hbKey, 1, proof);
    }

    function test_doubleVote_reverts() public {
        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        address voter = members[0];
        _vote(hbKey, round, members, voter, 1);

        bytes32[] memory proof = _proofForMember(hbKey, round, members, voter);
        vm.prank(voter);
        vm.expectRevert(HeartbeatManager.AlreadyResponded.selector);
        manager.submitVerdict(hbKey, 1, proof);
    }

    function test_finalize_validThreshold_updatesStatus() public {
        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();

        // 5 votes out of 10 => 50% quorum + 50% valid threshold
        for (uint256 i = 0; i < 5; i++) {
            _vote(hbKey, round, members, members[i], 1);
        }

        _finalizeDefault(hbKey, round);
        assertEq(uint8(manager.roundOutcome(hbKey, round)), uint8(ISlashingPolicy.Outcome.ValidThreshold));
        (HeartbeatManager.HeartbeatStatus status, , , , , ) = manager.heartbeats(hbKey);
        assertEq(uint8(status), uint8(HeartbeatManager.HeartbeatStatus.Verified));
    }

    function test_finalize_invalidThreshold_updatesStatus() public {
        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();

        for (uint256 i = 0; i < 5; i++) {
            _vote(hbKey, round, members, members[i], 2);
        }

        _finalizeDefault(hbKey, round);
        assertEq(uint8(manager.roundOutcome(hbKey, round)), uint8(ISlashingPolicy.Outcome.InvalidThreshold));
        (HeartbeatManager.HeartbeatStatus status, , , , , ) = manager.heartbeats(hbKey);
        assertEq(uint8(status), uint8(HeartbeatManager.HeartbeatStatus.Invalid));
    }

    function test_pause_blocks_submitHeartbeat_and_votes() public {
        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        manager.pause();

        vm.expectRevert(Pausable.EnforcedPause.selector);
        manager.submitHeartbeat(_defaultRawHTX(77), uint64(block.number - 1));

        bytes32[] memory proof = _proofForMember(hbKey, round, members, members[0]);
        vm.prank(members[0]);
        vm.expectRevert(Pausable.EnforcedPause.selector);
        manager.submitVerdict(hbKey, 1, proof);
    }

    function test_escalateOrExpire_beforeDeadline_reverts() public {
        (bytes32 hbKey, , , , ) = _submitPointerAndGetRound();
        vm.expectRevert(HeartbeatManager.BeforeDeadline.selector);
        manager.escalateOrExpire(hbKey, _defaultRawHTX(1));
    }

    function test_escalateOrExpire_inconclusive_startsNewRound_and_then_expires() public {
        // quorum requires 50%; only 1 vote => inconclusive
        (bytes32 hbKey, uint8 round1, , , address[] memory members1) = _submitPointerAndGetRound();
        _vote(hbKey, round1, members1, members1[0], 1);

        (, , , , , , , , , , uint64 deadline1, , , , , , , , , ) = manager.rounds(hbKey, round1);
        vm.warp(uint256(deadline1) + 1);

        vm.recordLogs();
        manager.escalateOrExpire(hbKey, _defaultRawHTX(1));
        Vm.Log[] memory logs = vm.getRecordedLogs();

        // round 1 finalized inconclusive and round2 started
        assertEq(uint8(manager.roundOutcome(hbKey, round1)), uint8(ISlashingPolicy.Outcome.Inconclusive));

        (, uint8 currentRound, uint8 escalationLevel, , , ) = manager.heartbeats(hbKey);
        assertEq(currentRound, 2);
        assertEq(escalationLevel, 1);

        // parse round2 started
        bytes32 sig = keccak256("RoundStarted(bytes32,uint8,bytes32,uint64,uint64,uint64,address[],bytes)");
        bool found;
        for (uint256 i = 0; i < logs.length; i++) {
            if (logs[i].topics.length > 0 && logs[i].topics[0] == sig && bytes32(logs[i].topics[1]) == hbKey) {
                (uint8 r2,, , , , , ) = abi.decode(logs[i].data, (uint8, bytes32, uint64, uint64, uint64, address[], bytes));
                if (r2 == 2) found = true;
            }
        }
        assertTrue(found, "round2 not started");

        // expire after round2 deadline with no quorum
        (, , , , , , , , , , uint64 deadline2, , , , , , , , , ) = manager.rounds(hbKey, 2);
        vm.warp(uint256(deadline2) + 1);
        manager.escalateOrExpire(hbKey, _defaultRawHTX(1));

        (HeartbeatManager.HeartbeatStatus status2, , , , , ) = manager.heartbeats(hbKey);
        assertEq(uint8(status2), uint8(HeartbeatManager.HeartbeatStatus.Expired));
    }

    function test_escalateOrExpire_revertsOnRawHashMismatch() public {
        (bytes32 hbKey, uint8 round, , , ) = _submitPointerAndGetRound();
        (, , , , , , , , , , uint64 deadline, , , , , , , , , ) = manager.rounds(hbKey, round);
        vm.warp(uint256(deadline) + 1);

        vm.expectRevert(HeartbeatManager.RawHTXHashMismatch.selector);
        manager.escalateOrExpire(hbKey, _defaultRawHTX(999));
    }

    function test_moduleUpgrade_doesNotAffectExistingRoundRewardAddress() public {
        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();

        for (uint256 i = 0; i < 5; i++) {
            _vote(hbKey, round, members, members[i], 1);
        }

        _finalizeDefault(hbKey, round);
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

        manager.distributeRewards(hbKey, round, voters);

        // old policy has rewards, new one doesn't
        assertGt(rewardPolicy.rewards(voters[0]), 0);
        assertEq(newReward.rewards(voters[0]), 0);
    }

    function test_submitVerdictsBatched_signature() public {
        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        address voter = members[0];
        uint256 pk = _findPk(voter);

        _batchedVote(hbKey, round, members, pk, voter, 1);

        uint256 packed = manager.getVotePacked(hbKey, round, voter);
        assertTrue((packed & (1 << 2)) != 0, "not responded");
        assertEq(uint8(packed & 0x3), 1);

        // invalid signature should revert
        bytes32[] memory proof = _proofForMember(hbKey, round, members, voter);
        bytes32 digest = manager.voteDigest(hbKey, round, 2);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(uint256(123456), digest);

        HeartbeatManager.SignedBatchedVote[] memory batch = new HeartbeatManager.SignedBatchedVote[](1);
        batch[0] = HeartbeatManager.SignedBatchedVote({
            operator: voter,
            heartbeatKey: hbKey,
            round: round,
            verdict: 2,
            memberProof: proof,
            sigV: v,
            sigR: r,
            sigS: s
        });

        vm.expectRevert(HeartbeatManager.InvalidSignature.selector);
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

        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();

        HeartbeatManager.SignedBatchedVote[] memory batch = new HeartbeatManager.SignedBatchedVote[](3);
        for (uint256 i = 0; i < 3; i++) {
            address voter = members[i];
            uint256 pk = _findPk(voter);
            bytes32[] memory proof = _proofForMember(hbKey, round, members, voter);
            bytes32 digest = manager.voteDigest(hbKey, round, 1);
            (uint8 v, bytes32 r, bytes32 s) = vm.sign(pk, digest);

            batch[i] = HeartbeatManager.SignedBatchedVote({
                operator: voter,
                heartbeatKey: hbKey,
                round: round,
                verdict: 1,
                memberProof: proof,
                sigV: v,
                sigR: r,
                sigS: s
            });
        }

        vm.expectRevert(HeartbeatManager.InvalidBatchSize.selector);
        manager.submitVerdictsBatched(batch);
    }

    function test_submitVerdictsBatched_hardLimit500() public {
        HeartbeatManager.SignedBatchedVote[] memory batch = new HeartbeatManager.SignedBatchedVote[](501);
        vm.expectRevert(HeartbeatManager.InvalidBatchSize.selector);
        manager.submitVerdictsBatched(batch);
    }

    function test_distributeRewards_revertsOnUnsortedVoters() public {
        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        for (uint256 i = 0; i < 5; i++) _vote(hbKey, round, members, members[i], 1);

        _finalizeDefault(hbKey, round);
        // fund rewards
        rewardToken.mint(governance, 1000);
        rewardToken.approve(address(rewardPolicy), type(uint256).max);
        rewardPolicy.fund(1000);
        vm.warp(block.timestamp + 2 days);
        rewardPolicy.sync();

        address[] memory voters = new address[](5);
        for (uint256 i = 0; i < 5; i++) voters[i] = members[4 - i]; // reversed => unsorted

        vm.expectRevert(HeartbeatManager.UnsortedVoters.selector);
        manager.distributeRewards(hbKey, round, voters);
    }

    function test_distributeRewards_revertsOnCountMismatch() public {
        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        for (uint256 i = 0; i < 5; i++) _vote(hbKey, round, members, members[i], 1);

        _finalizeDefault(hbKey, round);
        rewardToken.mint(governance, 1000);
        rewardToken.approve(address(rewardPolicy), type(uint256).max);
        rewardPolicy.fund(1000);
        vm.warp(block.timestamp + 2 days);
        rewardPolicy.sync();

        address[] memory voters = new address[](4);
        for (uint256 i = 0; i < 4; i++) voters[i] = members[i];

        vm.expectRevert(abi.encodeWithSelector(HeartbeatManager.InvalidVoterCount.selector, uint256(4), uint256(5)));
        manager.distributeRewards(hbKey, round, voters);
    }

    function test_distributeRewards_revertsOnInvalidVoterInList() public {
        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        for (uint256 i = 0; i < 5; i++) _vote(hbKey, round, members, members[i], 1);

        _finalizeDefault(hbKey, round);
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

        vm.expectRevert(HeartbeatManager.InvalidVoterInList.selector);
        manager.distributeRewards(hbKey, round, voters);
    }

    function test_committeeRoot_matchesOffchainMerkleComputation() public {
        (bytes32 hbKey, uint8 round, bytes32 root, , address[] memory members) = _submitPointerAndGetRound();
        bytes32[] memory leaves = MerkleTestUtils.buildLeaves(address(manager), hbKey, round, members);
        bytes32 computed = MerkleTestUtils.computeRoot(leaves);
        assertEq(computed, root);

        // proof check for one member
        bytes32[] memory proof = MerkleTestUtils.proofForIndex(leaves, 0);
        // OpenZeppelin MerkleProof expects leaf and root; verification happens inside HeartbeatManager anyway, but assert non-empty proof for larger trees
        if (members.length > 1) assertGt(proof.length, 0);
    }

    function test_abandonRewardDistribution_blocksPayout_and_is_ownerOnly() public {
        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        for (uint256 i = 0; i < 5; i++) {
            _vote(hbKey, round, members, members[i], 1);
        }

        _finalizeDefault(hbKey, round);
        address nonOwner = address(0xBEEF);
        vm.prank(nonOwner);
        vm.expectRevert(abi.encodeWithSelector(Ownable.OwnableUnauthorizedAccount.selector, nonOwner));
        manager.abandonRewardDistribution(hbKey, round);

        manager.abandonRewardDistribution(hbKey, round);

        address[] memory voters = new address[](5);
        for (uint256 i = 0; i < 5; i++) voters[i] = members[i];

        vm.expectRevert(HeartbeatManager.RewardsAlreadyDone.selector);
        manager.distributeRewards(hbKey, round, voters);
    }
}
