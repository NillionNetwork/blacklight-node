// SPDX-License-Identifier: MIT
pragma solidity ^0.8.22;

import "forge-std/Test.sol";

import "../src/mocks/MockERC20.sol";
import "../src/mocks/MockL1StandardBridge.sol";
import "../src/EmissionsController.sol";

contract EmissionsControllerTest is Test {
    MockERC20 token;
    MockL1StandardBridge bridge;
    EmissionsController controller;

    address owner = address(this);
    address l2Token = address(0xBEEF);
    address l2Recipient = address(0xCAFE);

    function setUp() public {
        token = new MockERC20("REWARD", "RWD");
        bridge = new MockL1StandardBridge();

        uint256[] memory schedule = new uint256[](2);
        schedule[0] = 100;
        schedule[1] = 150;

        controller = new EmissionsController(
            IERC20Mintable(address(token)),
            IL1StandardBridge(address(bridge)),
            l2Token,
            l2Recipient,
            block.timestamp + 5,
            10,        // epochDuration
            200_000,   // l2GasLimit
            300,       // global cap
            schedule,
            owner
        );
    }

    function test_mintAndBridge_enforcesEpochTiming() public {
        uint256 readyAt0 = controller.nextEpochReadyAt();
        vm.expectRevert(abi.encodeWithSelector(EmissionsController.EpochNotElapsed.selector, block.timestamp, readyAt0));
        controller.mintAndBridgeNextEpoch();

        vm.warp(readyAt0);
        (uint256 epoch1, uint256 amount1) = controller.mintAndBridgeNextEpoch();
        assertEq(epoch1, 1);
        assertEq(amount1, 100);

        assertEq(token.balanceOf(address(bridge)), 100);
        assertEq(controller.mintedEpochs(), 1);
        assertEq(controller.mintedTotal(), 100);

        // too early for epoch 2
        uint256 readyAt1 = controller.nextEpochReadyAt();
        vm.expectRevert(abi.encodeWithSelector(EmissionsController.EpochNotElapsed.selector, block.timestamp, readyAt1));
        controller.mintAndBridgeNextEpoch();

        vm.warp(readyAt1);
        (uint256 epoch2, uint256 amount2) = controller.mintAndBridgeNextEpoch();
        assertEq(epoch2, 2);
        assertEq(amount2, 150);

        assertEq(token.balanceOf(address(bridge)), 250);
        assertEq(controller.mintedEpochs(), 2);
        assertEq(controller.mintedTotal(), 250);

        vm.expectRevert(EmissionsController.NoRemainingEpochs.selector);
        controller.mintAndBridgeNextEpoch();
    }

    function test_mintAndBridge_isPermissionless() public {
        address caller = address(0x1234);
        vm.warp(controller.nextEpochReadyAt());

        vm.prank(caller);
        controller.mintAndBridgeNextEpoch();

        assertEq(controller.mintedEpochs(), 1);
        assertEq(token.balanceOf(address(bridge)), 100);
    }

    function test_globalCapExceeded_reverts() public {
        uint256[] memory schedule = new uint256[](2);
        schedule[0] = 200;
        schedule[1] = 150;

        EmissionsController c2 = new EmissionsController(
            IERC20Mintable(address(token)),
            IL1StandardBridge(address(bridge)),
            l2Token,
            l2Recipient,
            block.timestamp,
            10,
            200_000,
            300,
            schedule,
            owner
        );

        c2.mintAndBridgeNextEpoch(); // 200 ok

        vm.warp(block.timestamp + 10);
        // remaining cap is 100, but epoch wants 150
        vm.expectRevert(abi.encodeWithSelector(EmissionsController.GlobalCapExceeded.selector, uint256(150), uint256(100)));
        c2.mintAndBridgeNextEpoch();
    }

    function test_emissionForEpoch_bounds() public {
        assertEq(controller.epochs(), 2);
        assertEq(controller.emissionForEpoch(1), 100);
        assertEq(controller.emissionForEpoch(2), 150);

        vm.expectRevert(abi.encodeWithSelector(EmissionsController.InvalidEpoch.selector, uint256(0)));
        controller.emissionForEpoch(0);

        vm.expectRevert(abi.encodeWithSelector(EmissionsController.InvalidEpoch.selector, uint256(3)));
        controller.emissionForEpoch(3);
    }
}
