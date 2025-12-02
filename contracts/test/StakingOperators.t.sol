// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "../src/MockERC20.sol";
import "../src/StakingOperators.sol";

contract StakingOperatorsTest is Test {
    MockERC20 token;
    StakingOperators staking;

    address gov = address(0xA11CE);
    address operator = address(0xBEEF);
    address staker = address(0xCAFE);

    function setUp() public {
        token = new MockERC20("StakeToken", "STK");
        staking = new StakingOperators(token, gov, 7 days);

        token.mint(staker, 1_000e18);
        vm.prank(staker);
        token.approve(address(staking), type(uint256).max);
    }

    function testStakeAndUnstakeFlow() public {
        vm.startPrank(staker);
        staking.stakeTo(operator, 100e18);
        vm.stopPrank();

        assertEq(staking.stakeOf(operator), 100e18);
        assertEq(staking.totalStaked(), 100e18);
        assertEq(staking.operatorStaker(operator), staker);

        vm.prank(operator);
        staking.registerOperator("meta");

        vm.prank(staker);
        staking.requestUnstake(operator, 60e18);

        assertEq(staking.stakeOf(operator), 40e18);

        vm.prank(staker);
        vm.expectRevert(StakingOperators.NotReady.selector);
        staking.withdrawUnstaked(operator);

        vm.warp(block.timestamp + 7 days + 1);

        uint256 beforeBal = token.balanceOf(staker);
        vm.prank(staker);
        staking.withdrawUnstaked(operator);
        uint256 afterBal = token.balanceOf(staker);

        assertEq(afterBal - beforeBal, 60e18);
        assertEq(staking.totalStaked(), 40e18);
    }

    function testOneStakerPerOperator() public {
        address otherStaker = address(0xDEAD);

        token.mint(otherStaker, 1_000e18);
        vm.prank(otherStaker);
        token.approve(address(staking), type(uint256).max);

        vm.prank(staker);
        staking.stakeTo(operator, 100e18);

        vm.prank(otherStaker);
        vm.expectRevert(StakingOperators.DifferentStaker.selector);
        staking.stakeTo(operator, 50e18);
    }

    function testJailedCannotUnstake() public {
        vm.prank(staker);
        staking.stakeTo(operator, 100e18);

        vm.startPrank(gov);
        staking.grantRole(staking.SLASHER_ROLE(), gov);
        staking.jail(operator, uint64(block.timestamp + 10 days));
        vm.stopPrank();

        vm.prank(staker);
        vm.expectRevert(StakingOperators.OperatorJailed.selector);
        staking.requestUnstake(operator, 10e18);
    }

    function testIsActiveOperatorRequiresStake() public {
        vm.prank(staker);
        staking.stakeTo(operator, 100e18);

        vm.prank(operator);
        staking.registerOperator("metadata");

        assertTrue(staking.isActiveOperator(operator));

        vm.startPrank(gov);
        staking.grantRole(staking.SLASHER_ROLE(), gov);
        staking.slash(operator, 200e18);
        vm.stopPrank();

        assertEq(staking.stakeOf(operator), 0);
        assertFalse(staking.isActiveOperator(operator));
    }
}