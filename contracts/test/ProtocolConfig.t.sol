// SPDX-License-Identifier: MIT
pragma solidity ^0.8.22;

import "forge-std/Test.sol";
import "../src/ProtocolConfig.sol";

contract DummyModule {}

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
            0,
            1,
            0
        );
    }

    function test_constructor_revertsOnInvalidBps() public {
        vm.expectRevert(abi.encodeWithSelector(ProtocolConfig.InvalidBps.selector, uint256(10001)));
        new ProtocolConfig(
            address(this),
            address(this),
            address(this),
            address(this),
            address(this),
            1,
            0,
            1,
            0,
            10001,
            1,
            1,
            1,
            0,
            0,
            1,
            0
        );
    }

    function test_setParams_validatesCommitteeCaps() public {
        ProtocolConfig cfg = new ProtocolConfig(
            address(this),
            address(this),
            address(this),
            address(this),
            address(this),
            2,
            0,
            5,
            1,
            5000,
            5000,
            10,
            10,
            100,
            1,
            1,
            0
        );

        vm.expectRevert(abi.encodeWithSelector(ProtocolConfig.InvalidCommitteeCap.selector, uint32(10), uint32(5)));
        cfg.setParams(10, 0, 5, 1, 5000, 5000, 10, 10, 100, 1, 1, 0);
    }

    function test_setModules_onlyOwner() public {
        ProtocolConfig cfg = new ProtocolConfig(
            address(this),
            address(this),
            address(this),
            address(this),
            address(this),
            2,
            0,
            5,
            1,
            5000,
            5000,
            10,
            10,
            100,
            1,
            1,
            0
        );

        DummyModule notOwnerModule1 = new DummyModule();
        DummyModule notOwnerModule2 = new DummyModule();
        DummyModule notOwnerModule3 = new DummyModule();
        DummyModule notOwnerModule4 = new DummyModule();
        vm.prank(address(0xBEEF));
        vm.expectRevert();
        cfg.setModules(address(notOwnerModule1), address(notOwnerModule2), address(notOwnerModule3), address(notOwnerModule4));

        DummyModule module1 = new DummyModule();
        DummyModule module2 = new DummyModule();
        DummyModule module3 = new DummyModule();
        DummyModule module4 = new DummyModule();
        cfg.setModules(address(module1), address(module2), address(module3), address(module4));
        assertEq(cfg.stakingOps(), address(module1));
        assertEq(cfg.committeeSelector(), address(module2));
        assertEq(cfg.slashingPolicy(), address(module3));
        assertEq(cfg.rewardPolicy(), address(module4));
    }
}
