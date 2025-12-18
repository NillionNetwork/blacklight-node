// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "./helpers/RCFixture.sol";

contract RCSystemTest is RCFixture {
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

        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();

        // Finalize valid threshold with 5/10 votes
        for (uint256 i = 0; i < 5; i++) _vote(wk, round, members, members[i], 1);

        assertEq(uint8(manager.roundOutcome(wk, round)), uint8(ISlashingPolicy.Outcome.ValidThreshold));

        // Fund & unlock rewards
        rewardToken.mint(governance, 1000);
        rewardToken.approve(address(rewardPolicy), type(uint256).max);
        rewardPolicy.fund(1000);
        vm.warp(block.timestamp + 2 days);
        rewardPolicy.sync();

        // Distribute rewards to valid voters (already sorted)
        address[] memory voters = new address[](5);
        for (uint256 i = 0; i < 5; i++) voters[i] = members[i];

        manager.distributeRewards(wk, round, voters);

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

        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        assertEq(members.length, 200);

        // vote valid from first 60 members => 30% of total stake (since equal stake)
        for (uint256 i = 0; i < 60; i++) _vote(wk, round, members, members[i], 1);

        assertEq(uint8(manager.roundOutcome(wk, round)), uint8(ISlashingPolicy.Outcome.ValidThreshold));

        // enforce jailing (nonvoters should be jailed)
        jailingPolicy.enforceJailFromMembers(wk, round, members);

        for (uint256 i = 60; i < members.length; i++) {
            assertTrue(stakingOps.isJailed(members[i]), "nonvoter not jailed");
        }
        for (uint256 i = 0; i < 60; i++) {
            assertFalse(stakingOps.isJailed(members[i]), "voter jailed");
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

        (bytes32 wk1, uint8 round1, , , address[] memory members1) = _submitPointerAndGetRound();

        // Finalize with 5 valid votes and 1 invalid vote (invalid before finalizing)
        for (uint256 i = 0; i < 4; i++) _vote(wk1, round1, members1, members1[i], 1);
        _vote(wk1, round1, members1, members1[5], 2);
        _vote(wk1, round1, members1, members1[4], 1);

        // Jail everyone except correct voters
        jailingPolicy.enforceJailFromMembers(wk1, round1, members1);

        // Submit a new workload (different pointer)
        vm.recordLogs();
        WorkloadManager.WorkloadPointer memory p2 = _defaultPointer(2);
        bytes32 wk2 = manager.deriveWorkloadKey(p2);
        (uint32 snap2, address[] memory members2Offchain) = _prepareCommittee(wk2, 1, 0);
        manager.submitWorkload(p2, snap2, members2Offchain);
        Vm.Log[] memory logs = vm.getRecordedLogs();
        bytes32 sig = keccak256("RoundStarted(bytes32,uint8,bytes32,uint32,uint32,uint64,uint64,address[])");

        address[] memory members2;
        for (uint256 i = 0; i < logs.length; i++) {
            if (logs[i].topics.length > 0 && logs[i].topics[0] == sig && bytes32(logs[i].topics[1]) == wk2) {
                (, , , , , , members2) =
                    abi.decode(logs[i].data, (uint8, bytes32, uint32, uint32, uint64, uint64, address[]));
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
}
