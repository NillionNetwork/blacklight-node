// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "./helpers/RCFixture.sol";

contract WorkloadFlowTest is RCFixture {
    function setUp() public {
        uint256[] memory stakes = new uint256[](2);
        stakes[0] = 150e18;
        stakes[1] = 150e18;

        _deploySystem(
            2,
            stakes,
            2,    // baseCommitteeSize
            2,    // maxCommitteeSize
            5000, // quorumBps (50%)
            5000, // verificationBps (50%)
            1 days,
            7 days,
            0     // maxEscalations
        );
    }

    function testFullWorkloadFlow() public {
        rewardToken.mint(governance, 1_000e18);
        vm.prank(governance);
        rewardToken.approve(address(rewardPolicy), type(uint256).max);
        vm.prank(governance);
        rewardPolicy.fund(1_000e18);

        (bytes32 wk, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        assertEq(members.length, 2);

        // Single valid vote (50%) meets quorum/verification thresholds.
        _vote(wk, round, members, members[0], 1);

        (WorkloadManager.WorkloadStatus status, , , , ) = manager.workloads(wk);
        assertEq(uint8(status), uint8(WorkloadManager.WorkloadStatus.Verified));

        // Unlock funded rewards before distribution.
        vm.warp(block.timestamp + 1 days + 1);

        address[] memory voters = new address[](1);
        voters[0] = members[0];

        manager.distributeRewards(wk, round, voters);

        uint256 beforeClaim = rewardToken.balanceOf(members[0]);
        vm.prank(members[0]);
        rewardPolicy.claim();

        assertGt(rewardToken.balanceOf(members[0]) - beforeClaim, 0);
        assertEq(rewardToken.balanceOf(members[1]), 0);
    }
}
