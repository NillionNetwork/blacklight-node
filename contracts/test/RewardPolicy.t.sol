// SPDX-License-Identifier: MIT
pragma solidity ^0.8.22;

import "forge-std/Test.sol";
import "../src/mocks/MockERC20.sol";
import "../src/RewardPolicy.sol";

contract RewardPolicyTest is Test {
    MockERC20 token;
    RewardPolicy policy;

    address manager = address(this);
    address owner = address(this);

    function setUp() public {
        token = new MockERC20("REWARD", "RWD");
        policy = new RewardPolicy(IERC20(address(token)), manager, owner, 1 days, 0);
    }

    function _fundAndUnlock(uint256 amount) internal {
        token.mint(owner, amount);
        token.approve(address(policy), type(uint256).max);
        policy.fund(amount);

        // unlock entire stream
        vm.warp(block.timestamp + 2 days);
        policy.sync();
        assertGe(policy.spendableBudget(), amount);
    }

    function test_accrueWeights_revertsOnLengthMismatch() public {
        _fundAndUnlock(100);

        address[] memory rec = new address[](1);
        rec[0] = address(0x1);
        uint256[] memory w = new uint256[](2);
        w[0] = 1;
        w[1] = 2;

        vm.expectRevert(RewardPolicy.LengthMismatch.selector);
        policy.accrueWeights(bytes32("hbKey"), 1, rec, w);
    }

    function test_accrueWeights_revertsOnUnsortedRecipients() public {
        _fundAndUnlock(100);

        address[] memory rec = new address[](2);
        rec[0] = address(0x2);
        rec[1] = address(0x1);
        uint256[] memory w = new uint256[](2);
        w[0] = 1;
        w[1] = 1;

        vm.expectRevert(RewardPolicy.UnsortedRecipients.selector);
        policy.accrueWeights(bytes32("hbKey"), 1, rec, w);
    }

    function test_commitmentMismatch_afterInitialRevert() public {
        // No budget yet, so accrueWeights will revert but commitment should be set.
        address[] memory rec = new address[](1);
        rec[0] = address(0x1);
        uint256[] memory w = new uint256[](1);
        w[0] = 1;

        vm.expectRevert(RewardPolicy.InsufficientBudget.selector);
        policy.accrueWeights(bytes32("hbKey"), 1, rec, w);

        _fundAndUnlock(100);

        // first successful accrue sets commitment
        policy.accrueWeights(bytes32("hbKey"), 1, rec, w);

        address[] memory rec2 = new address[](1);
        rec2[0] = address(0x2);
        uint256[] memory w2 = new uint256[](1);
        w2[0] = 1;

        vm.expectRevert(RewardPolicy.AlreadyProcessed.selector);
        policy.accrueWeights(bytes32("hbKey"), 1, rec2, w2);

        assertEq(policy.rewards(address(0x1)), 100); // full budget goes to single recipient
    }

    function test_maxPayoutCap() public {
        token.mint(owner, 1000);
        token.approve(address(policy), type(uint256).max);
        policy.fund(1000);
        vm.warp(block.timestamp + 2 days);
        policy.sync();

        policy.setMaxPayoutPerFinalize(100);

        address[] memory rec = new address[](2);
        rec[0] = address(0x1);
        rec[1] = address(0x2);
        uint256[] memory w = new uint256[](2);
        w[0] = 1;
        w[1] = 1;

        uint256 before = policy.spendableBudget();
        policy.accrueWeights(bytes32("hbKey"), 1, rec, w);

        uint256 afterB = policy.spendableBudget();
        assertEq(before - afterB, 100);
        assertEq(policy.rewards(address(0x1)) + policy.rewards(address(0x2)), 100);
    }

    function test_dustDistribution_allocatesOneWei() public {
        // Make spendableBudget exactly 1
        token.mint(owner, 1);
        token.approve(address(policy), type(uint256).max);
        policy.fund(1);
        vm.warp(block.timestamp + 2 days);
        policy.sync();
        assertEq(policy.spendableBudget(), 1);

        address[] memory rec = new address[](2);
        rec[0] = address(0x1);
        rec[1] = address(0x2);
        uint256[] memory w = new uint256[](2);
        w[0] = 1000;
        w[1] = 1;

        policy.accrueWeights(bytes32("hbKey"), 1, rec, w);

        // floor division gives 0 for both -> dust path gives 1 to highest weight
        assertEq(policy.rewards(address(0x1)), 1);
        assertEq(policy.rewards(address(0x2)), 0);
    }

    function test_claim_transfersAndClears() public {
        _fundAndUnlock(10);

        address[] memory rec = new address[](1);
        rec[0] = address(0xBEEF);
        uint256[] memory w = new uint256[](1);
        w[0] = 1;

        policy.accrueWeights(bytes32("hbKey"), 1, rec, w);
        assertEq(policy.rewards(address(0xBEEF)), 10);

        vm.prank(address(0xBEEF));
        policy.claim();
        assertEq(token.balanceOf(address(0xBEEF)), 10);
        assertEq(policy.rewards(address(0xBEEF)), 0);
    }
}
