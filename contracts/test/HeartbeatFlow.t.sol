// SPDX-License-Identifier: MIT
pragma solidity ^0.8.22;

import "./helpers/BlacklightFixture.sol";

contract HeartbeatFlowTest is BlacklightFixture {
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

    function testFullHeartbeatFlow() public {
        rewardToken.mint(governance, 1_000e18);
        vm.prank(governance);
        rewardToken.approve(address(rewardPolicy), type(uint256).max);
        vm.prank(governance);
        rewardPolicy.fund(1_000e18);

        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        assertEq(members.length, 2);

        // Single valid vote (50%) meets quorum/verification thresholds.
        _vote(hbKey, round, members, members[0], 1);

        _finalizeDefault(hbKey, round);
        (HeartbeatManager.HeartbeatStatus status, , , , , , , , , ) = manager.heartbeats(hbKey);
        assertEq(uint8(status), uint8(HeartbeatManager.HeartbeatStatus.Verified));

        // Unlock funded rewards before distribution.
        vm.warp(block.timestamp + 1 days + 1);

        address[] memory voters = new address[](1);
        voters[0] = members[0];

        manager.distributeRewards(hbKey, round, voters);

        uint256 beforeClaim = rewardToken.balanceOf(members[0]);
        vm.prank(members[0]);
        rewardPolicy.claim();

        assertGt(rewardToken.balanceOf(members[0]) - beforeClaim, 0);
        assertEq(rewardToken.balanceOf(members[1]), 0);
    }
}

contract HeartbeatInvalidRewardFlowTest is BlacklightFixture {
    function setUp() public {
        uint256[] memory stakes = new uint256[](10);
        for (uint256 i = 0; i < stakes.length; i++) {
            stakes[i] = 100e18;
        }

        _deploySystem(
            10,
            stakes,
            10,   // baseCommitteeSize
            10,   // maxCommitteeSize
            7000, // quorumBps (70%)
            7000, // verificationBps (70%)
            1 days,
            7 days,
            0     // maxEscalations
        );
    }

    function testInvalidThresholdRewardsInvalidVoters() public {
        rewardToken.mint(governance, 1_000e18);
        vm.prank(governance);
        rewardToken.approve(address(rewardPolicy), type(uint256).max);
        vm.prank(governance);
        rewardPolicy.fund(1_000e18);

        (bytes32 hbKey, uint8 round, , , address[] memory members) = _submitPointerAndGetRound();
        assertEq(members.length, 10);

        for (uint256 i = 0; i < 9; i++) {
            _vote(hbKey, round, members, members[i], 2);
        }
        _vote(hbKey, round, members, members[9], 1);

        _finalizeDefault(hbKey, round);
        (HeartbeatManager.HeartbeatStatus status, , , , , , , , , ) = manager.heartbeats(hbKey);
        assertEq(uint8(status), uint8(HeartbeatManager.HeartbeatStatus.Invalid));

        vm.warp(block.timestamp + 1 days + 1);

        address[] memory voters = new address[](9);
        for (uint256 i = 0; i < 9; i++) {
            voters[i] = members[i];
        }

        manager.distributeRewards(hbKey, round, voters);

        for (uint256 i = 0; i < 9; i++) {
            assertGt(rewardPolicy.rewards(voters[i]), 0);
        }
        assertEq(rewardPolicy.rewards(members[9]), 0);
    }
}
