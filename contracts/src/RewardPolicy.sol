// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "@openzeppelin/contracts/access/Ownable.sol";

import "./Interfaces.sol";
import "./WorkloadManager.sol";

/// @title RewardPolicy
/// @notice Stake-weighted reward distribution for validated workloads.
contract RewardPolicy is IRewardPolicy, Ownable {
    using SafeERC20 for IERC20;

    error ZeroAddress();
    error NotWorkloadManager();
    error NothingToClaim();

    IERC20 public immutable rewardToken;
    WorkloadManager public immutable workloadManager;
    IStakingOperators public immutable stakingOps;

    uint256 public totalRewardPerWorkload;
    mapping(address => uint256) public rewards;

    event TotalRewardPerWorkloadUpdated(uint256 oldReward, uint256 newReward);
    event RewardsAccrued(bytes32 indexed workloadKey, uint8 round, address indexed operator, uint256 amount);
    event RewardClaimed(address indexed operator, uint256 amount);
    event SkippedNoCorrectVoters(bytes32 indexed workloadKey, uint8 round);

    modifier onlyWorkloadManager() {
        if (msg.sender != address(workloadManager)) revert NotWorkloadManager();
        _;
    }

    constructor(
        IERC20 _rewardToken,
        WorkloadManager _manager,
        IStakingOperators _stakingOps,
        address _owner,
        uint256 _totalRewardPerWorkload
    ) Ownable(_owner) {
        if (address(_rewardToken) == address(0)) revert ZeroAddress();
        if (address(_manager) == address(0)) revert ZeroAddress();
        if (address(_stakingOps) == address(0)) revert ZeroAddress();

        rewardToken = _rewardToken;
        workloadManager = _manager;
        stakingOps = _stakingOps;
        totalRewardPerWorkload = _totalRewardPerWorkload;
    }

    function setTotalRewardPerWorkload(uint256 newReward) external onlyOwner {
        emit TotalRewardPerWorkloadUpdated(totalRewardPerWorkload, newReward);
        totalRewardPerWorkload = newReward;
    }

    function fund(uint256 amount) external onlyOwner {
        rewardToken.safeTransferFrom(msg.sender, address(this), amount);
    }

    function withdraw(uint256 amount, address to) external onlyOwner {
        if (to == address(0)) revert ZeroAddress();
        rewardToken.safeTransfer(to, amount);
    }

    function onWorkloadValidated(
        bytes32 workloadKey,
        uint8 round,
        address[] calldata committeeMembers
    ) external override onlyWorkloadManager {
        uint256 len = committeeMembers.length;
        if (len == 0) {
            emit SkippedNoCorrectVoters(workloadKey, round);
            return;
        }

        address[] memory correct = new address[](len);
        uint256 correctCount;
        uint256 totalCorrectStake;

        for (uint256 i = 0; i < len; ) {
            address op = committeeMembers[i];
            WorkloadManager.Verdict v = workloadManager.verdicts(workloadKey, round, op);
            if (v == WorkloadManager.Verdict.Valid) {
                uint256 s = stakingOps.stakeOf(op);
                if (s > 0) {
                    correct[correctCount] = op;
                    unchecked { ++correctCount; }
                    totalCorrectStake += s;
                }
            }
            unchecked { ++i; }
        }

        if (correctCount == 0 || totalCorrectStake == 0 || totalRewardPerWorkload == 0) {
            emit SkippedNoCorrectVoters(workloadKey, round);
            return;
        }

        uint256 rewardTotal = totalRewardPerWorkload;

        for (uint256 i = 0; i < correctCount; ) {
            address op = correct[i];
            uint256 s = stakingOps.stakeOf(op);
            if (s > 0) {
                uint256 share = (rewardTotal * s) / totalCorrectStake;
                if (share > 0) {
                    rewards[op] += share;
                    emit RewardsAccrued(workloadKey, round, op, share);
                }
            }
            unchecked { ++i; }
        }
    }

    function claim() external {
        uint256 amount = rewards[msg.sender];
        if (amount == 0) revert NothingToClaim();

        rewards[msg.sender] = 0;
        rewardToken.safeTransfer(msg.sender, amount);

        emit RewardClaimed(msg.sender, amount);
    }
}