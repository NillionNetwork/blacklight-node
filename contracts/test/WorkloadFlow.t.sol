// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";

import "../src/MockERC20.sol";
import "../src/StakingOperators.sol";
import "../src/WeightedCommitteeSelector.sol";
import "../src/NoOpSlashingPolicy.sol";
import "../src/RewardPolicy.sol";
import "../src/ProtocolConfig.sol";
import "../src/WorkloadManager.sol";
import "../src/Interfaces.sol";

contract WorkloadFlowTest is Test {
    MockERC20 stakeToken;
    MockERC20 rewardToken;

    StakingOperators staking;
    WeightedCommitteeSelector selector;
    NoOpSlashingPolicy slashing;
    RewardPolicy rewardPolicy;
    ProtocolConfig config;
    WorkloadManager wm;

    address gov = address(0xA11CE);
    address op1 = address(0xBEEF);
    address op2 = address(0xBEE2);

    address staker1 = address(0xCAFE);
    address staker2 = address(0xCAFF);

    function setUp() public {
        vm.startPrank(gov);

        stakeToken = new MockERC20("StakeToken", "STK");
        rewardToken = new MockERC20("RewardToken", "RWD");

        staking = new StakingOperators(stakeToken, gov, 7 days);

        selector = new WeightedCommitteeSelector(
            staking,
            gov,
            0,
            10
        );

        slashing = new NoOpSlashingPolicy();

        vm.stopPrank();

        _deployFullConfig();
        _stakeAndRegisterOperators();
    }

    function _deployFullConfig() internal {
        vm.startPrank(gov);

        uint16 verificationBps = 5100; // 51% - requires both votes with equal stakes
        uint256 responseWindow = 1 days;
        uint8 maxEscalations = 1;
        uint32 baseCommitteeSize = 2;
        uint32 committeeSizeGrowthBps = 0;

        config = new ProtocolConfig(
            gov,
            address(staking),
            address(selector),
            address(slashing),
            address(0),
            verificationBps,
            responseWindow,
            maxEscalations,
            baseCommitteeSize,
            committeeSizeGrowthBps
        );

        wm = new WorkloadManager(config, gov);

        rewardPolicy = new RewardPolicy(
            rewardToken,
            wm,
            staking,
            gov,
            30e18
        );

        config.setModules(
            address(staking),
            address(selector),
            address(slashing),
            address(rewardPolicy)
        );

        vm.stopPrank();
    }

    function _stakeAndRegisterOperators() internal {
        stakeToken.mint(staker1, 1_000e18);
        stakeToken.mint(staker2, 1_000e18);

        vm.prank(staker1);
        stakeToken.approve(address(staking), type(uint256).max);
        vm.prank(staker2);
        stakeToken.approve(address(staking), type(uint256).max);

        vm.prank(staker1);
        staking.stakeTo(op1, 150e18);
        vm.prank(staker2);
        staking.stakeTo(op2, 150e18);

        vm.prank(op1);
        staking.registerOperator("meta1");
        vm.prank(op2);
        staking.registerOperator("meta2");
    }

    function testFullWorkloadFlow() public {
        rewardToken.mint(gov, 1_000e18);
        vm.prank(gov);
        rewardToken.approve(address(rewardPolicy), 1_000e18);
        vm.prank(gov);
        rewardPolicy.fund(1_000e18);

        WorkloadManager.WorkloadPointer memory ptr = WorkloadManager.WorkloadPointer({
            currentId: 1,
            previousId: 0,
            contentHash: keccak256("dummy"),
            blobIndex: 42
        });

        bytes32 key = wm.submitWorkload(ptr);

        (
            WorkloadManager.WorkloadStatus status_,
            uint8 currentRound,
            ,
            ,

        ) = wm.workloads(key);
        assertEq(uint8(status_), uint8(WorkloadManager.WorkloadStatus.Pending));
        assertEq(currentRound, 1);

        address[] memory members = wm.getCommittee(key, 1);
        assertEq(members.length, 2);

        // Submit verdicts - round will finalize after first vote meets threshold
        vm.prank(op1);
        wm.submitVerdict(key, WorkloadManager.Verdict.Valid);
        
        // Try to submit second verdict - should fail as round is already finalized
        vm.prank(op2);
        vm.expectRevert(WorkloadManager.NotPending.selector);
        wm.submitVerdict(key, WorkloadManager.Verdict.Valid);

        (
            WorkloadManager.WorkloadStatus status2,
            ,
            ,
            ,

        ) = wm.workloads(key);

        assertEq(uint8(status2), uint8(WorkloadManager.WorkloadStatus.Verified));

        assertEq(wm.assignments(op1), 1);
        assertEq(wm.assignments(op2), 1);
        assertEq(wm.responses(op1), 1);
        assertEq(wm.responses(op2), 0); // op2 didn't get to vote

        // Only op1 gets rewards since they voted Valid
        uint256 reward1 = rewardPolicy.rewards(op1);
        uint256 reward2 = rewardPolicy.rewards(op2);
        assertEq(reward1, 30e18); // 30e18 * 150 / 150 (only op1 voted)
        assertEq(reward2, 0); // op2 didn't vote

        uint256 before1 = rewardToken.balanceOf(op1);

        vm.prank(op1);
        rewardPolicy.claim();

        assertEq(rewardToken.balanceOf(op1) - before1, 30e18);
    }
}