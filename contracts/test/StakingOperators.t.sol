// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";

import "../src/mocks/MockERC20.sol";
import "../src/ProtocolConfig.sol";
import "../src/StakingOperators.sol";

contract StakingOperatorsTest is Test {
    MockERC20 stakeToken;
    ProtocolConfig config;
    StakingOperators stakingOps;

    address admin = address(0xA11CE);

    function setUp() public {
        stakeToken = new MockERC20("STAKE", "STK");

        vm.startPrank(admin);
        stakingOps = new StakingOperators(IERC20(address(stakeToken)), admin, 1 days);
        vm.stopPrank();

        config = new ProtocolConfig(
            address(this),
            address(stakingOps),
            address(0x1111),
            address(0x2222),
            address(0x3333),
            2,
            0,
            5,
            0,
            0,
            0,
            10,
            10,
            100,
            1e18
        );

        vm.prank(admin);
        stakingOps.setProtocolConfig(config);
    }

    function test_stakeTo_bindsStaker() public {
        address operator = address(0xB0B);
        address staker1 = address(0x111);
        address staker2 = address(0x222);

        stakeToken.mint(staker1, 2e18);
        stakeToken.mint(staker2, 2e18);

        vm.startPrank(staker1);
        stakeToken.approve(address(stakingOps), type(uint256).max);
        stakingOps.stakeTo(operator, 1e18);
        vm.stopPrank();

        vm.startPrank(staker2);
        stakeToken.approve(address(stakingOps), type(uint256).max);
        vm.expectRevert(StakingOperators.DifferentStaker.selector);
        stakingOps.stakeTo(operator, 1e18);
        vm.stopPrank();

        assertEq(stakingOps.operatorStaker(operator), staker1);
        assertEq(stakingOps.stakeOf(operator), 1e18);
    }

    function test_register_requiresMinStake() public {
        address operator = address(0xB0B);
        stakeToken.mint(operator, 2e18);

        vm.startPrank(operator);
        stakeToken.approve(address(stakingOps), type(uint256).max);
        stakingOps.stakeTo(operator, 0.5e18);
        vm.expectRevert(StakingOperators.InsufficientStakeForActivation.selector);
        stakingOps.registerOperator("ipfs://x");
        stakingOps.stakeTo(operator, 0.5e18);
        stakingOps.registerOperator("ipfs://x");
        vm.stopPrank();

        assertTrue(stakingOps.isActiveOperator(operator));
    }

    function test_requestUnstake_and_withdraw() public {
        address operator = address(0xB0B);
        stakeToken.mint(operator, 3e18);

        vm.startPrank(operator);
        stakeToken.approve(address(stakingOps), type(uint256).max);
        stakingOps.stakeTo(operator, 2e18);
        stakingOps.registerOperator("ipfs://x");

        stakingOps.requestUnstake(operator, 1e18);
        assertEq(stakingOps.stakeOf(operator), 1e18);

        // before delay
        vm.expectRevert(StakingOperators.NotReady.selector);
        stakingOps.withdrawUnstaked(operator);

        vm.warp(block.timestamp + 1 days + 1);
        stakingOps.withdrawUnstaked(operator);
        vm.stopPrank();

        assertEq(stakeToken.balanceOf(operator), 2e18); // 3e18 minted - 2e18 staked + 1e18 withdrawn
    }

    function test_slash_active_and_unbonding() public {
        address operator = address(0xB0B);
        stakeToken.mint(operator, 5e18);

        vm.startPrank(operator);
        stakeToken.approve(address(stakingOps), type(uint256).max);
        stakingOps.stakeTo(operator, 3e18);
        stakingOps.registerOperator("ipfs://x");
        stakingOps.requestUnstake(operator, 1e18); // active stake 2e18, 1e18 unbonding
        vm.stopPrank();

        uint256 deadBefore = stakeToken.balanceOf(address(0xdead));

        vm.startPrank(admin);
        stakingOps.grantRole(stakingOps.SLASHER_ROLE(), admin);
        stakingOps.slash(operator, 3e18);
        vm.stopPrank();

        // slashed all active (2e18) + unbonding (1e18)
        assertEq(stakingOps.stakeOf(operator), 0);
        assertEq(stakeToken.balanceOf(address(0xdead)) - deadBefore, 3e18);
    }

    function test_jail_deactivates_and_requires_reactivate() public {
        address operator = address(0xB0B);
        stakeToken.mint(operator, 2e18);

        vm.startPrank(operator);
        stakeToken.approve(address(stakingOps), type(uint256).max);
        stakingOps.stakeTo(operator, 1e18);
        stakingOps.registerOperator("ipfs://x");
        vm.stopPrank();

        assertTrue(stakingOps.isActiveOperator(operator));

        vm.startPrank(admin);
        stakingOps.grantRole(stakingOps.SLASHER_ROLE(), admin);
        stakingOps.jail(operator, uint64(block.timestamp + 7 days));
        vm.stopPrank();

        assertTrue(stakingOps.isJailed(operator));
        assertFalse(stakingOps.isActiveOperator(operator));

        vm.warp(block.timestamp + 8 days);

        // still inactive (policy set active=false); must reactivate manually
        assertFalse(stakingOps.isActiveOperator(operator));
        vm.prank(operator);
        stakingOps.reactivateOperator();
        assertTrue(stakingOps.isActiveOperator(operator));
    }

    function test_snapshot_and_stakeAt_checkpoints() public {
        address operator = address(0xB0B);
        stakeToken.mint(operator, 3e18);

        vm.startPrank(operator);
        stakeToken.approve(address(stakingOps), type(uint256).max);
        stakingOps.stakeTo(operator, 2e18);
        vm.stopPrank();

        // set snapshotter
        vm.prank(admin);
        stakingOps.setSnapshotter(address(this));

        vm.roll(2);
        uint32 snap1 = stakingOps.snapshot();
        assertEq(stakingOps.stakeAt(operator, snap1), 2e18);

        vm.startPrank(operator);
        stakingOps.requestUnstake(operator, 1e18); // stake drops to 1e18
        vm.stopPrank();

        vm.roll(4);
        uint32 snap2 = stakingOps.snapshot();
        assertEq(stakingOps.stakeAt(operator, snap2), 1e18);
        assertEq(stakingOps.stakeAt(operator, snap1), 2e18);
    }
}
