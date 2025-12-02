// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "src/core/StakingOperators.sol";
import "src/core/TESTToken.sol";

/// @title StakingOperators Test Suite
/// @notice Comprehensive tests for the StakingOperators contract
contract StakingOperatorsTest is Test {
    StakingOperators public staking;
    TESTToken public token;

    address public admin = address(this);
    address public operator1 = address(0x1);
    address public operator2 = address(0x2);
    address public operator3 = address(0x3);
    address public staker1 = address(0x4);
    address public staker2 = address(0x5);

    uint256 public constant UNSTAKE_DELAY = 7 days;

    function setUp() public {
        token = new TESTToken(admin);
        staking = new StakingOperators(IERC20(address(token)), admin, UNSTAKE_DELAY);

        // Mint tokens to admin and stakers
        token.mint(admin, 10000 ether);
        token.mint(staker1, 1000 ether);
        token.mint(staker2, 1000 ether);

        staking.grantRole(staking.SLASHER_ROLE(), admin);
    }

    // ========================================================================
    // Constructor Tests
    // ========================================================================

    function testConstructorSetsParameters() public view {
        assertEq(address(staking.stakingToken()), address(token));
        assertEq(staking.unstakeDelay(), UNSTAKE_DELAY);
        assertTrue(staking.hasRole(staking.DEFAULT_ADMIN_ROLE(), admin));
    }

    function testConstructorRevertsWithZeroToken() public {
        vm.expectRevert();
        new StakingOperators(IERC20(address(0)), admin, UNSTAKE_DELAY);
    }

    function testConstructorRevertsWithZeroAdmin() public {
        vm.expectRevert();
        new StakingOperators(IERC20(address(token)), address(0), UNSTAKE_DELAY);
    }

    // ========================================================================
    // Operator Registration Tests
    // ========================================================================

    function testRegisterOperator() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("ipfs://metadata");

        assertTrue(staking.isActiveOperator(operator1));
        assertEq(staking.operatorStaker(operator1), address(this));

        IStakingOperators.OperatorInfo memory info = staking.getOperatorInfo(operator1);
        assertTrue(info.active);
        assertEq(info.metadataURI, "ipfs://metadata");
    }

    function testRegisterOperatorWithEmptyMetadata() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        assertTrue(staking.isActiveOperator(operator1));
    }

    function testCannotRegisterOperatorTwice() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.startPrank(operator1);
        staking.registerOperator("metadata1");

        // Should allow updating metadata
        staking.registerOperator("metadata2");
        
        IStakingOperators.OperatorInfo memory info = staking.getOperatorInfo(operator1);
        assertEq(info.metadataURI, "metadata2");
        vm.stopPrank();
    }

    function testGetActiveOperatorsAfterRegistration() public {
        token.approve(address(staking), 200 ether);
        staking.stakeTo(operator1, 100 ether);
        staking.stakeTo(operator2, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        vm.prank(operator2);
        staking.registerOperator("");

        address[] memory activeOps = staking.getActiveOperators();
        assertEq(activeOps.length, 2);
    }

    // ========================================================================
    // Deactivation Tests
    // ========================================================================

    function testDeactivateOperator() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.startPrank(operator1);
        staking.registerOperator("");
        staking.deactivateOperator();
        vm.stopPrank();

        assertFalse(staking.isActiveOperator(operator1));

        address[] memory activeOps = staking.getActiveOperators();
        assertEq(activeOps.length, 0);
    }

    function testCannotDeactivateIfNotRegistered() public {
        vm.prank(operator1);
        vm.expectRevert();
        staking.deactivateOperator();
    }

    function testCannotDeactivateIfAlreadyInactive() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.startPrank(operator1);
        staking.registerOperator("");
        staking.deactivateOperator();

        vm.expectRevert();
        staking.deactivateOperator();
        vm.stopPrank();
    }

    // ========================================================================
    // Staking Tests
    // ========================================================================

    function testStakeToOperator() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        assertEq(staking.stakeOf(operator1), 100 ether);
        assertEq(staking.totalStaked(), 100 ether);
        assertEq(token.balanceOf(address(staking)), 100 ether);
    }

    function testStakeFromDifferentStaker() public {
        // Staker1 stakes for operator1
        vm.startPrank(staker1);
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);
        vm.stopPrank();

        vm.prank(operator1);
        staking.registerOperator("");
        vm.stopPrank();

        assertEq(staking.stakeOf(operator1), 100 ether);
        assertEq(staking.operatorStaker(operator1), staker1);
    }



    function testCannotStakeZeroAmount() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        token.approve(address(staking), 100 ether);

        vm.expectRevert();
        staking.stakeTo(operator1, 0);
    }

    function testMultipleStakesAccumulate() public {
        token.approve(address(staking), 300 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        staking.stakeTo(operator1, 100 ether);
        staking.stakeTo(operator1, 100 ether);

        assertEq(staking.stakeOf(operator1), 300 ether);
        assertEq(staking.totalStaked(), 300 ether);
    }

    // ========================================================================
    // Unstaking Tests
    // ========================================================================

    function testRequestUnstake() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        staking.requestUnstake(operator1, 50 ether);

        assertEq(staking.stakeOf(operator1), 50 ether);
        assertEq(staking.totalStaked(), 50 ether);
    }

    function testCannotUnstakeMoreThanStaked() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        vm.expectRevert();
        staking.requestUnstake(operator1, 150 ether);
    }

    function testCannotRequestUnstakeForOtherOperator() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        vm.prank(operator2);
        vm.expectRevert();
        staking.requestUnstake(operator1, 50 ether);
    }

    function testWithdrawUnstakedAfterDelay() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        staking.requestUnstake(operator1, 50 ether);

        // Fast forward past delay
        vm.warp(block.timestamp + UNSTAKE_DELAY + 1);

        uint256 balanceBefore = token.balanceOf(admin);
        staking.withdrawUnstaked(operator1);
        uint256 balanceAfter = token.balanceOf(admin);

        assertEq(balanceAfter - balanceBefore, 50 ether);
    }

    function testCannotWithdrawBeforeDelay() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        staking.requestUnstake(operator1, 50 ether);

        vm.expectRevert();
        staking.withdrawUnstaked(operator1);
    }

    function testMultipleUnstakeRequests() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        staking.requestUnstake(operator1, 30 ether);
        
        vm.expectRevert(StakingOperators.UnbondingExists.selector);
        staking.requestUnstake(operator1, 20 ether);
    }

    // ========================================================================
    // Slashing Tests
    // ========================================================================

    function testSlashOperator() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        // Admin slashes
        staking.slash(operator1, 30 ether);

        assertEq(staking.stakeOf(operator1), 70 ether);
        assertEq(staking.totalStaked(), 70 ether);
    }

    function testCannotSlashMoreThanStake() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        // Should cap slash amount to balance
        staking.slash(operator1, 150 ether);
        
        assertEq(staking.stakeOf(operator1), 0);
        assertEq(staking.totalStaked(), 0);
    }

    function testOnlyAdminCanSlash() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        vm.prank(staker1);
        vm.expectRevert();
        staking.slash(operator1, 30 ether);
    }

    function testSlashInactiveOperator() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        vm.prank(operator1);
        staking.deactivateOperator();

        // Can still slash inactive operator
        staking.slash(operator1, 30 ether);
        assertEq(staking.stakeOf(operator1), 70 ether);
    }

    // ========================================================================
    // Jailing Tests
    // ========================================================================

    function testJailOperator() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        uint64 jailUntil = uint64(block.timestamp + 1 days);
        staking.jail(operator1, jailUntil);

        assertTrue(staking.isJailed(operator1));
        assertFalse(staking.isActiveOperator(operator1));
    }

    function testOperatorUnjailedAfterTime() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        uint64 jailUntil = uint64(block.timestamp + 1 days);
        staking.jail(operator1, jailUntil);

        assertTrue(staking.isJailed(operator1));

        // Fast forward past jail time
        vm.warp(block.timestamp + 1 days + 1);

        assertFalse(staking.isJailed(operator1));
    }

    function testOnlyAdminCanJail() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        vm.prank(staker1);
        vm.expectRevert();
        staking.jail(operator1, uint64(block.timestamp + 1 days));
    }

    function testJailedOperatorNotInActiveList() public {
        token.approve(address(staking), 200 ether);
        staking.stakeTo(operator1, 100 ether);
        staking.stakeTo(operator2, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");
        vm.prank(operator2);
        staking.registerOperator("");

        staking.jail(operator1, uint64(block.timestamp + 1 days));

        address[] memory activeOps = staking.getActiveOperators();
        assertEq(activeOps.length, 1);
        assertEq(activeOps[0], operator2);
    }

    // ========================================================================
    // Total Staked Tests
    // ========================================================================

    function testTotalStakedAccumulates() public {
        token.approve(address(staking), 300 ether);
        staking.stakeTo(operator1, 100 ether);
        staking.stakeTo(operator2, 200 ether);

        vm.prank(operator1);
        staking.registerOperator("");
        vm.prank(operator2);
        staking.registerOperator("");

        assertEq(staking.totalStaked(), 300 ether);
    }

    function testTotalStakedDecreasesOnUnstake() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        staking.requestUnstake(operator1, 30 ether);

        assertEq(staking.totalStaked(), 70 ether);
    }

    function testTotalStakedDecreasesOnSlash() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        staking.slash(operator1, 30 ether);

        assertEq(staking.totalStaked(), 70 ether);
    }

    // ========================================================================
    // Integration Tests
    // ========================================================================

    function testCompleteOperatorLifecycle() public {
        // Stake
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);
        assertEq(staking.stakeOf(operator1), 100 ether);

        // Register
        vm.prank(operator1);
        staking.registerOperator("ipfs://metadata");
        assertTrue(staking.isActiveOperator(operator1));

        // Request unstake
        staking.requestUnstake(operator1, 30 ether);
        assertEq(staking.stakeOf(operator1), 70 ether);

        // Withdraw after delay
        vm.warp(block.timestamp + UNSTAKE_DELAY + 1);
        staking.withdrawUnstaked(operator1);

        // Deactivate
        vm.prank(operator1);
        staking.deactivateOperator();
        assertFalse(staking.isActiveOperator(operator1));
    }

    function testMultipleOperatorsIndependent() public {
        // Stake different amounts
        token.approve(address(staking), 600 ether);
        staking.stakeTo(operator1, 100 ether);
        staking.stakeTo(operator2, 200 ether);
        staking.stakeTo(operator3, 300 ether);

        // Register operators
        vm.prank(operator1);
        staking.registerOperator("op1");
        vm.prank(operator2);
        staking.registerOperator("op2");
        vm.prank(operator3);
        staking.registerOperator("op3");

        assertEq(staking.totalStaked(), 600 ether);

        // Slash operator2
        staking.slash(operator2, 50 ether);
        assertEq(staking.stakeOf(operator2), 150 ether);
        assertEq(staking.totalStaked(), 550 ether);

        // Operator1 and operator3 unaffected
        assertEq(staking.stakeOf(operator1), 100 ether);
        assertEq(staking.stakeOf(operator3), 300 ether);
    }

    // ========================================================================
    // Access Control Tests
    // ========================================================================

    function testAdminCanGrantRoles() public {
        address newAdmin = address(0x999);

        staking.grantRole(staking.DEFAULT_ADMIN_ROLE(), newAdmin);
        assertTrue(staking.hasRole(staking.DEFAULT_ADMIN_ROLE(), newAdmin));
    }

    function testNonAdminCannotSlash() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        vm.prank(operator2);
        vm.expectRevert();
        staking.slash(operator1, 10 ether);
    }

    function testNonAdminCannotJail() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        vm.prank(operator2);
        vm.expectRevert();
        staking.jail(operator1, uint64(block.timestamp + 1 days));
    }

    // ========================================================================
    // Edge Cases
    // ========================================================================

    function testStakeToJailedOperator() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("");

        staking.jail(operator1, uint64(block.timestamp + 1 days));

        token.approve(address(staking), 100 ether);

        // Should revert because operator is jailed (inactive)
        vm.expectRevert();
        staking.stakeTo(operator1, 100 ether);
    }

    function testOperatorCanStakeForThemselves() public {
        // Admin transfers to operator1
        token.transfer(operator1, 100 ether);

        vm.startPrank(operator1);
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);
        staking.registerOperator("");
        vm.stopPrank();

        assertEq(staking.stakeOf(operator1), 100 ether);
        assertEq(staking.operatorStaker(operator1), operator1);
    }

    function testGetOperatorInfoReturnsCorrectData() public {
        token.approve(address(staking), 100 ether);
        staking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        staking.registerOperator("ipfs://QmTest123");

        IStakingOperators.OperatorInfo memory info = staking.getOperatorInfo(operator1);
        assertTrue(info.active);
        assertEq(info.metadataURI, "ipfs://QmTest123");

        // Deactivate
        vm.prank(operator1);
        staking.deactivateOperator();

        info = staking.getOperatorInfo(operator1);
        assertFalse(info.active);
    }
}
