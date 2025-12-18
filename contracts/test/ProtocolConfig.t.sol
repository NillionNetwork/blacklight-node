// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "../src/ProtocolConfig.sol";

contract ProtocolConfigTest is Test {
    function test_constructor_revertsOnZeroModules() public {
        vm.expectRevert(ProtocolConfig.ZeroAddress.selector);
        new ProtocolConfig(
            address(this),
            address(0),
            address(1),
            address(2),
            address(3),
            1,
            0,
            1,
            0,
            0,
            0,
            1,
            1,
            0,
            0
        );
    }

    function test_constructor_revertsOnInvalidBps() public {
        vm.expectRevert(abi.encodeWithSelector(ProtocolConfig.InvalidBps.selector, uint256(10001)));
        new ProtocolConfig(
            address(this),
            address(1),
            address(2),
            address(3),
            address(4),
            1,
            0,
            1,
            0,
            10001,
            0,
            1,
            1,
            0,
            0
        );
    }

    function test_setParams_validatesCommitteeCaps() public {
        ProtocolConfig cfg = new ProtocolConfig(
            address(this),
            address(1),
            address(2),
            address(3),
            address(4),
            2,
            0,
            5,
            1,
            5000,
            5000,
            10,
            10,
            100,
            1
        );

        vm.expectRevert(abi.encodeWithSelector(ProtocolConfig.InvalidCommitteeCap.selector, uint32(10), uint32(5)));
        cfg.setParams(10, 0, 5, 1, 5000, 5000, 10, 10, 100, 1);
    }

    function test_setModules_onlyOwner() public {
        ProtocolConfig cfg = new ProtocolConfig(
            address(this),
            address(1),
            address(2),
            address(3),
            address(4),
            2,
            0,
            5,
            1,
            5000,
            5000,
            10,
            10,
            100,
            1
        );

        vm.prank(address(0xBEEF));
        vm.expectRevert();
        cfg.setModules(address(10), address(11), address(12), address(13));

        cfg.setModules(address(10), address(11), address(12), address(13));
        assertEq(cfg.stakingOps(), address(10));
        assertEq(cfg.committeeSelector(), address(11));
        assertEq(cfg.slashingPolicy(), address(12));
        assertEq(cfg.rewardPolicy(), address(13));
    }
}
