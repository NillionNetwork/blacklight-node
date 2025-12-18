// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "./helpers/BlacklightFixture.sol";

contract HeartbeatManagerCommitteeGrowthTest is BlacklightFixture {
    function setUp() public {
        uint256 nOps = 30;
        uint256[] memory stakes = new uint256[](nOps);
        for (uint256 i = 0; i < nOps; i++) stakes[i] = 2e18;

        _deploySystem(
            nOps,
            stakes,
            4,    // baseCommitteeSize
            10,   // maxCommitteeSize
            9000, // quorumBps
            9000, // verificationBps
            1 days,
            7 days,
            2     // maxEscalations
        );

        // Increase growth bps to 50%
        config.setParams(
            4,
            5000, // growth bps
            10,
            2,
            9000,
            9000,
            1 days,
            7 days,
            100,
            config.minOperatorStake()
        );
    }

    function test_committeeSize_grows_on_escalation() public {
        (bytes32 hbKey, uint8 round1, , , ) = _submitPointerAndGetRound();
        assertEq(round1, 1);

        (, , , , , , uint32 size1, , , , uint64 deadline1, , , , , , , , , ) = manager.rounds(hbKey, 1);
        assertEq(size1, 4);

        vm.warp(uint256(deadline1) + 1);
        manager.escalateOrExpire(hbKey, _defaultRawHTX(1));

        (, , , , , , uint32 size2, , , , uint64 deadline2, , , , , , , , , ) = manager.rounds(hbKey, 2);
        assertEq(size2, 6); // 4 * 1.5

        vm.warp(uint256(deadline2) + 1);
        manager.escalateOrExpire(hbKey, _defaultRawHTX(1));

        (, , , , , , uint32 size3, , , , uint64 deadline3, , , , , , , , , ) = manager.rounds(hbKey, 3);
        assertEq(size3, 9); // 6 * 1.5

        // After max escalations (2), third inconclusive should expire
        vm.warp(uint256(deadline3) + 1);
        manager.escalateOrExpire(hbKey, _defaultRawHTX(1));

        (HeartbeatManager.HeartbeatStatus status, , , , , ) = manager.heartbeats(hbKey);
        assertEq(uint8(status), uint8(HeartbeatManager.HeartbeatStatus.Expired));
    }
}
