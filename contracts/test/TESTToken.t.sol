// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "src/core/TESTToken.sol";

/// @title TESTToken Test Suite
/// @notice Comprehensive tests for the TESTToken contract
contract TESTTokenTest is Test {
    TESTToken public token;

    address public owner = address(this);
    address public user1 = address(0x1);
    address public user2 = address(0x2);
    address public user3 = address(0x3);

    function setUp() public {
        token = new TESTToken(owner);
    }

    // ========================================================================
    // Constructor & Initialization Tests
    // ========================================================================

    function testTokenNameAndSymbol() public view {
        assertEq(token.name(), "Test Token");
        assertEq(token.symbol(), "TEST");
    }

    function testTokenDecimals() public view {
        assertEq(token.decimals(), 18);
    }

    function testOwnerIsSetCorrectly() public view {
        assertEq(token.owner(), owner);
    }

    function testInitialSupplyIsZero() public view {
        assertEq(token.totalSupply(), 0);
    }

    // ========================================================================
    // Minting Tests
    // ========================================================================

    function testOwnerCanMint() public {
        token.mint(user1, 1000 ether);

        assertEq(token.balanceOf(user1), 1000 ether);
        assertEq(token.totalSupply(), 1000 ether);
    }

    function testMintMultipleTimes() public {
        token.mint(user1, 100 ether);
        token.mint(user1, 200 ether);
        token.mint(user2, 300 ether);

        assertEq(token.balanceOf(user1), 300 ether);
        assertEq(token.balanceOf(user2), 300 ether);
        assertEq(token.totalSupply(), 600 ether);
    }

    function testMintToMultipleAddresses() public {
        token.mint(user1, 100 ether);
        token.mint(user2, 200 ether);
        token.mint(user3, 300 ether);

        assertEq(token.balanceOf(user1), 100 ether);
        assertEq(token.balanceOf(user2), 200 ether);
        assertEq(token.balanceOf(user3), 300 ether);
        assertEq(token.totalSupply(), 600 ether);
    }

    function testMintZeroAmount() public {
        token.mint(user1, 0);

        assertEq(token.balanceOf(user1), 0);
        assertEq(token.totalSupply(), 0);
    }

    function testNonOwnerCannotMint() public {
        vm.prank(user1);
        vm.expectRevert();
        token.mint(user1, 1000 ether);
    }

    function testMintLargeAmount() public {
        uint256 largeAmount = type(uint256).max / 2;
        token.mint(user1, largeAmount);

        assertEq(token.balanceOf(user1), largeAmount);
        assertEq(token.totalSupply(), largeAmount);
    }

    // ========================================================================
    // Transfer Tests
    // ========================================================================

    function testTransferTokens() public {
        token.mint(owner, 1000 ether);

        token.transfer(user1, 300 ether);

        assertEq(token.balanceOf(owner), 700 ether);
        assertEq(token.balanceOf(user1), 300 ether);
    }

    function testTransferBetweenUsers() public {
        token.mint(user1, 1000 ether);

        vm.prank(user1);
        token.transfer(user2, 400 ether);

        assertEq(token.balanceOf(user1), 600 ether);
        assertEq(token.balanceOf(user2), 400 ether);
    }

    function testCannotTransferMoreThanBalance() public {
        token.mint(user1, 100 ether);

        vm.prank(user1);
        vm.expectRevert();
        token.transfer(user2, 101 ether);
    }

    function testTransferZeroAmount() public {
        token.mint(user1, 100 ether);

        vm.prank(user1);
        token.transfer(user2, 0);

        assertEq(token.balanceOf(user1), 100 ether);
        assertEq(token.balanceOf(user2), 0);
    }

    function testTransferToSelf() public {
        token.mint(user1, 100 ether);

        vm.prank(user1);
        token.transfer(user1, 50 ether);

        assertEq(token.balanceOf(user1), 100 ether);
    }

    // ========================================================================
    // Approval & TransferFrom Tests
    // ========================================================================

    function testApprove() public {
        token.mint(user1, 1000 ether);

        vm.prank(user1);
        token.approve(user2, 500 ether);

        assertEq(token.allowance(user1, user2), 500 ether);
    }

    function testTransferFrom() public {
        token.mint(user1, 1000 ether);

        vm.prank(user1);
        token.approve(user2, 500 ether);

        vm.prank(user2);
        token.transferFrom(user1, user3, 300 ether);

        assertEq(token.balanceOf(user1), 700 ether);
        assertEq(token.balanceOf(user3), 300 ether);
        assertEq(token.allowance(user1, user2), 200 ether);
    }

    function testCannotTransferFromWithoutApproval() public {
        token.mint(user1, 1000 ether);

        vm.prank(user2);
        vm.expectRevert();
        token.transferFrom(user1, user3, 100 ether);
    }

    function testCannotTransferFromMoreThanApproved() public {
        token.mint(user1, 1000 ether);

        vm.prank(user1);
        token.approve(user2, 100 ether);

        vm.prank(user2);
        vm.expectRevert();
        token.transferFrom(user1, user3, 101 ether);
    }

    function testApproveUpdatesAllowance() public {
        token.mint(user1, 1000 ether);

        vm.startPrank(user1);
        token.approve(user2, 100 ether);
        assertEq(token.allowance(user1, user2), 100 ether);

        token.approve(user2, 200 ether);
        assertEq(token.allowance(user1, user2), 200 ether);
        vm.stopPrank();
    }

    function testApproveZeroResetsAllowance() public {
        token.mint(user1, 1000 ether);

        vm.startPrank(user1);
        token.approve(user2, 500 ether);
        token.approve(user2, 0);
        vm.stopPrank();

        assertEq(token.allowance(user1, user2), 0);
    }

    // ========================================================================
    // Ownership Tests
    // ========================================================================

    function testTransferOwnership() public {
        token.transferOwnership(user1);
        assertEq(token.owner(), user1);
    }

    function testNewOwnerCanMint() public {
        token.transferOwnership(user1);

        vm.prank(user1);
        token.mint(user2, 1000 ether);

        assertEq(token.balanceOf(user2), 1000 ether);
    }

    function testOldOwnerCannotMintAfterTransfer() public {
        token.transferOwnership(user1);

        vm.expectRevert();
        token.mint(user2, 1000 ether);
    }

    function testNonOwnerCannotTransferOwnership() public {
        vm.prank(user1);
        vm.expectRevert();
        token.transferOwnership(user2);
    }

    // ========================================================================
    // Integration Tests
    // ========================================================================

    function testCompleteTokenFlow() public {
        // Mint
        token.mint(user1, 1000 ether);
        assertEq(token.balanceOf(user1), 1000 ether);

        // Transfer
        vm.prank(user1);
        token.transfer(user2, 300 ether);
        assertEq(token.balanceOf(user2), 300 ether);

        // Approve
        vm.prank(user2);
        token.approve(user3, 100 ether);

        // TransferFrom
        vm.prank(user3);
        token.transferFrom(user2, user3, 100 ether);

        assertEq(token.balanceOf(user1), 700 ether);
        assertEq(token.balanceOf(user2), 200 ether);
        assertEq(token.balanceOf(user3), 100 ether);
        assertEq(token.totalSupply(), 1000 ether);
    }

    function testMultipleUsersInteracting() public {
        // Mint to multiple users
        token.mint(user1, 100 ether);
        token.mint(user2, 200 ether);
        token.mint(user3, 300 ether);

        // User1 transfers to User2
        vm.prank(user1);
        token.transfer(user2, 50 ether);

        // User2 transfers to User3
        vm.prank(user2);
        token.transfer(user3, 100 ether);

        // User3 transfers to User1
        vm.prank(user3);
        token.transfer(user1, 150 ether);

        assertEq(token.balanceOf(user1), 200 ether); // 100 - 50 + 150
        assertEq(token.balanceOf(user2), 150 ether); // 200 + 50 - 100
        assertEq(token.balanceOf(user3), 250 ether); // 300 + 100 - 150
        assertEq(token.totalSupply(), 600 ether);
    }

    // ========================================================================
    // Edge Cases
    // ========================================================================

    function testMintToZeroAddress() public {
        vm.expectRevert();
        token.mint(address(0), 1000 ether);
    }

    function testTransferToZeroAddress() public {
        token.mint(owner, 1000 ether);

        vm.expectRevert();
        token.transfer(address(0), 100 ether);
    }

    function testApproveZeroAddress() public {
        token.mint(owner, 1000 ether);

        vm.expectRevert();
        token.approve(address(0), 100 ether);
    }

    function testBalanceOfZeroAddress() public view {
        uint256 balance = token.balanceOf(address(0));
        assertEq(balance, 0);
    }

    function testMintMaxUint() public {
        // This should fail due to overflow
        token.mint(user1, type(uint256).max);
        vm.expectRevert();
        token.mint(user1, 1);
    }

    // ========================================================================
    // ERC20 Standard Compliance Tests
    // ========================================================================

    function testTotalSupplyMatchesMintedTokens() public {
        token.mint(user1, 100 ether);
        token.mint(user2, 200 ether);
        token.mint(user3, 300 ether);

        assertEq(token.totalSupply(), token.balanceOf(user1) + token.balanceOf(user2) + token.balanceOf(user3));
    }

    function testAllowanceIsIndependentBetweenSpenders() public {
        token.mint(user1, 1000 ether);

        vm.startPrank(user1);
        token.approve(user2, 100 ether);
        token.approve(user3, 200 ether);
        vm.stopPrank();

        assertEq(token.allowance(user1, user2), 100 ether);
        assertEq(token.allowance(user1, user3), 200 ether);
    }

    function testTransferEmitsEvent() public {
        token.mint(owner, 1000 ether);

        vm.expectEmit(true, true, false, true);
        emit IERC20.Transfer(owner, user1, 100 ether);

        token.transfer(user1, 100 ether);
    }

    function testApprovalEmitsEvent() public {
        token.mint(owner, 1000 ether);

        vm.expectEmit(true, true, false, true);
        emit IERC20.Approval(owner, user1, 500 ether);

        token.approve(user1, 500 ether);
    }

    // ========================================================================
    // Fuzz Tests
    // ========================================================================

    function testFuzzMint(uint256 amount) public {
        vm.assume(amount < type(uint256).max / 2); // Avoid overflow

        token.mint(user1, amount);

        assertEq(token.balanceOf(user1), amount);
        assertEq(token.totalSupply(), amount);
    }

    function testFuzzTransfer(uint256 mintAmount, uint256 transferAmount) public {
        vm.assume(mintAmount < type(uint256).max / 2);
        vm.assume(transferAmount <= mintAmount);

        token.mint(user1, mintAmount);

        vm.prank(user1);
        token.transfer(user2, transferAmount);

        assertEq(token.balanceOf(user1), mintAmount - transferAmount);
        assertEq(token.balanceOf(user2), transferAmount);
    }

    function testFuzzApproveAndTransferFrom(uint256 amount) public {
        vm.assume(amount < type(uint256).max / 2);

        token.mint(user1, amount);

        vm.prank(user1);
        token.approve(user2, amount);

        vm.prank(user2);
        token.transferFrom(user1, user3, amount);

        assertEq(token.balanceOf(user3), amount);
        assertEq(token.balanceOf(user1), 0);
    }
}
