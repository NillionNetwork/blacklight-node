// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "./helpers/BlacklightFixture.sol";

contract BlacklightSystemTest is BlacklightFixture {
    mapping(address => uint256) internal expectedRewards;

    function test_endToEnd_valid_workflow_rewards_and_claim() public {
        uint256 nOps = 15;
        uint256[] memory stakes = new uint256[](nOps);
        for (uint256 i = 0; i < nOps; i++) stakes[i] = 2e18;

        _deploySystem(
            nOps,
            stakes,
            10,   // baseCommitteeSize
            10,   // maxCommitteeSize
            5000, // quorumBps
            5000, // verificationBps
            1 days,
            7 days,
            0
        );

        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();

        // Finalize valid threshold with 5/10 votes
        for (uint256 i = 0; i < 5; i++) _vote(hbKey, round, members, members[i], 1);

        _finalizeDefault(hbKey, round);
        assertEq(uint8(manager.roundOutcome(hbKey, round)), uint8(ISlashingPolicy.Outcome.ValidThreshold));

        // Fund & unlock rewards
        rewardToken.mint(governance, 1000);
        rewardToken.approve(address(rewardPolicy), type(uint256).max);
        rewardPolicy.fund(1000);
        vm.warp(block.timestamp + 2 days);
        rewardPolicy.sync();

        // Distribute rewards to valid voters (already sorted)
        address[] memory voters = new address[](5);
        for (uint256 i = 0; i < 5; i++) voters[i] = members[i];

        manager.distributeRewards(hbKey, round, voters);

        // Each gets 200 (equal weights)
        for (uint256 i = 0; i < 5; i++) {
            assertEq(rewardPolicy.rewards(voters[i]), 200);

            vm.prank(voters[i]);
            rewardPolicy.claim();
            assertEq(rewardToken.balanceOf(voters[i]), 200);
        }
    }

    function test_largeCommittee_200_members_finalizes_and_jailing_enforcement() public {
        uint256 nOps = 250;
        uint256[] memory stakes = new uint256[](nOps);
        for (uint256 i = 0; i < nOps; i++) stakes[i] = 2e18;

        _deploySystem(
            nOps,
            stakes,
            200,  // baseCommitteeSize
            200,  // maxCommitteeSize
            3000, // quorumBps (30%)
            3000, // verificationBps (30%)
            1 days,
            3 days,
            0
        );

        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        assertEq(members.length, 200);

        // vote valid from first 60 members => 30% of total stake (since equal stake)
        for (uint256 i = 0; i < 60; i++) _vote(hbKey, round, members, members[i], 1);

        _finalizeDefault(hbKey, round);
        assertEq(uint8(manager.roundOutcome(hbKey, round)), uint8(ISlashingPolicy.Outcome.ValidThreshold));

        // enforce jailing (nonvoters should be jailed)
        jailingPolicy.enforceJailFromMembers(hbKey, round, members);

        for (uint256 i = 60; i < members.length; i++) {
            assertTrue(stakingOps.isJailed(members[i]), "nonvoter not jailed");
        }
        for (uint256 i = 0; i < 60; i++) {
            assertFalse(stakingOps.isJailed(members[i]), "voter jailed");
        }
    }

    function test_largeCommittee_weighted_rewards_track_stake_across_rounds() public {
        uint256 nOps = 60;
        uint256[] memory stakes = new uint256[](nOps);
        for (uint256 i = 0; i < nOps; i++) {
            stakes[i] = ((i % 7) + 1) * 1e18; // varied stakes to change committee weights
        }

        _deploySystem(
            nOps,
            stakes,
            50,    // baseCommitteeSize
            50,    // maxCommitteeSize
            10_000, // quorumBps (force full participation)
            10_000, // verificationBps (force full participation)
            1 days,
            3 days,
            0
        );

        rewardToken.mint(governance, 30_000);
        rewardToken.approve(address(rewardPolicy), type(uint256).max);
        rewardPolicy.fund(30_000);
        vm.warp(block.timestamp + 3 days);
        rewardPolicy.sync();
        rewardPolicy.setMaxPayoutPerFinalize(5_000);

        for (uint64 id = 1; id <= 3; id++) {
            (bytes32 hbKey, uint8 round, , uint64 snapshotId, address[] memory members) = _submitRawHTXAndGetRound(id);

            uint256 spendableBefore = rewardPolicy.spendableBudget();
            uint256 budget = spendableBefore;
            uint256 cap = rewardPolicy.maxPayoutPerFinalize();
            if (cap != 0 && budget > cap) budget = cap;

            uint256[] memory memberStakes = new uint256[](members.length);
            uint256 totalStake;

            // everyone votes valid so the entire committee weight is rewarded
            for (uint256 i = 0; i < members.length; i++) {
                address voter = members[i];
                _vote(hbKey, round, members, voter, 1);

                uint256 s = stakingOps.stakeAt(voter, snapshotId);
                memberStakes[i] = s;
                totalStake += s;
            }

            _finalizeRound(hbKey, round, id);
            assertEq(uint8(manager.roundOutcome(hbKey, round)), uint8(ISlashingPolicy.Outcome.ValidThreshold));

            manager.distributeRewards(hbKey, round, members);

            uint256 distributed;
            for (uint256 i = 0; i < members.length; i++) {
                uint256 share = (budget * memberStakes[i]) / totalStake;
                expectedRewards[members[i]] += share;
                distributed += share;
                assertEq(rewardPolicy.rewards(members[i]), expectedRewards[members[i]], "stake-weight mismatch");
            }

            uint256 spendableAfter = rewardPolicy.spendableBudget();
            assertEq(spendableBefore - spendableAfter, distributed, "budget accounting drift");
        }
    }

    function test_jailing_removesFromNextCommitteeSelection() public {
        uint256 nOps = 20;
        uint256[] memory stakes = new uint256[](nOps);
        for (uint256 i = 0; i < nOps; i++) stakes[i] = 2e18;

        _deploySystem(
            nOps,
            stakes,
            10,
            10,
            5000,
            5000,
            1 days,
            7 days,
            0
        );

        (bytes32 hbKey1, uint8 round1, , , address[] memory members1) = _submitPointerAndGetRound();

        // Finalize with 5 valid votes and 1 invalid vote (invalid before finalizing)
        for (uint256 i = 0; i < 4; i++) _vote(hbKey1, round1, members1, members1[i], 1);
        _vote(hbKey1, round1, members1, members1[5], 2);
        _vote(hbKey1, round1, members1, members1[4], 1);

        _finalizeDefault(hbKey1, round1);
        // Jail everyone except correct voters
        jailingPolicy.enforceJailFromMembers(hbKey1, round1, members1);

        // Submit a new heartbeat (different HTX)
        vm.recordLogs();
        bytes memory rawHTX2 = _defaultRawHTX(2);
        uint64 submissionBlock = uint64(block.number);
        bytes32 hbKey2 = manager.deriveHeartbeatKey(rawHTX2, submissionBlock);
        (uint64 snap2, ) = _prepareCommittee(hbKey2, 1, 0);
        manager.submitHeartbeat(rawHTX2, snap2);
        Vm.Log[] memory logs = vm.getRecordedLogs();
        bytes32 sig = keccak256("RoundStarted(bytes32,uint8,bytes32,uint64,uint64,uint64,address[],bytes)");

        address[] memory members2;
        for (uint256 i = 0; i < logs.length; i++) {
            if (logs[i].topics.length > 0 && logs[i].topics[0] == sig && bytes32(logs[i].topics[1]) == hbKey2) {
                (, , , , , members2, ) =
                    abi.decode(logs[i].data, (uint8, bytes32, uint32, uint64, uint64, address[], bytes));
                break;
            }
        }
        require(members2.length != 0, "round2 members missing");

        // Ensure jailed operators from previous round are not selected
        for (uint256 i = 5; i < members1.length; i++) {
            // members1[5] (incorrect) + members1[6..9] (nonvoters) should be jailed
            if (i >= 5) {
                assertTrue(stakingOps.isJailed(members1[i]), "expected jailed");
                for (uint256 j = 0; j < members2.length; j++) {
                    assertTrue(members2[j] != members1[i], "jailed operator selected");
                }
            }
        }
    }

    function test_veryLargeOperatorPool_reward_distribution_and_jailing() public {
        uint256 nOps = 1000;
        uint256[] memory stakes = new uint256[](nOps);
        for (uint256 i = 0; i < nOps; i++) stakes[i] = 1e18;

        _deploySystem(
            nOps,
            stakes,
            200,  // baseCommitteeSize
            200,  // maxCommitteeSize
            5000, // quorumBps (50%)
            5000, // verificationBps (50%)
            1 days,
            7 days,
            0
        );

        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        assertEq(members.length, 200);

        // 120 valid votes (60% of committee) finalize to ValidThreshold.
        for (uint256 i = 0; i < 120; i++) _vote(hbKey, round, members, members[i], 1);
        _finalizeDefault(hbKey, round);
        assertEq(uint8(manager.roundOutcome(hbKey, round)), uint8(ISlashingPolicy.Outcome.ValidThreshold));

        // Fund rewards and unlock budget.
        rewardToken.mint(governance, 12_000);
        rewardToken.approve(address(rewardPolicy), type(uint256).max);
        rewardPolicy.fund(12_000);
        vm.warp(block.timestamp + 2 days);
        rewardPolicy.sync();

        address[] memory voters = new address[](120);
        for (uint256 i = 0; i < 120; i++) voters[i] = members[i];

        manager.distributeRewards(hbKey, round, voters);

        // Each voter should get an equal 100 tokens (12_000 / 120) since stakes are equal.
        for (uint256 i = 0; i < 120; i++) {
            assertEq(rewardPolicy.rewards(voters[i]), 100);
        }

        // Enforce jailing on the large committee; only non-voters should be jailed.
        jailingPolicy.enforceJailFromMembers(hbKey, round, members);
        for (uint256 i = 0; i < members.length; i++) {
            if (i < 120) {
                assertFalse(stakingOps.isJailed(members[i]), "voter should not be jailed");
            } else {
                assertTrue(stakingOps.isJailed(members[i]), "nonvoter should be jailed");
            }
        }
    }
}
