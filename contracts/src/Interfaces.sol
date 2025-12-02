// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/* -------------------------------------------------------------------------- */
/*                                Staking Ops                                 */
/* -------------------------------------------------------------------------- */

interface IStakingOperators {
    struct OperatorInfo {
        bool active;
        string metadataURI;
    }

    // Views
    function stakeOf(address operator) external view returns (uint256);
    function totalStaked() external view returns (uint256);
    function isJailed(address operator) external view returns (bool);
    function isActiveOperator(address operator) external view returns (bool);
    function getOperatorInfo(address operator) external view returns (OperatorInfo memory);
    function getActiveOperators() external view returns (address[] memory);
    function operatorStaker(address operator) external view returns (address);
    function stakingToken() external view returns (address);
    function unstakeDelay() external view returns (uint256);

    // Staking
    function stakeTo(address operator, uint256 amount) external;
    function requestUnstake(address operator, uint256 amount) external;
    function withdrawUnstaked(address operator) external;

    // Operator registry
    function registerOperator(string calldata metadataURI) external;
    function deactivateOperator() external;

    // Slashing / jailing (governance-controlled)
    function slash(address operator, uint256 amount) external;
    function jail(address operator, uint64 untilTimestamp) external;
}

/* -------------------------------------------------------------------------- */
/*                              Committee Selector                             */
/* -------------------------------------------------------------------------- */

interface ICommitteeSelector {
    function selectCommittee(
        bytes32 workloadKey,
        uint8 round,
        uint32 committeeSize
    ) external view returns (address[] memory members);
}

/* -------------------------------------------------------------------------- */
/*                               Slashing Policy                               */
/* -------------------------------------------------------------------------- */

interface ISlashingPolicy {
    enum Outcome {
        ValidThreshold,
        InvalidThreshold,
        Inconclusive
    }

    function onRoundFinalized(
        bytes32 workloadKey,
        uint8 round,
        Outcome outcome,
        address[] calldata committeeMembers
    ) external;
}

/* -------------------------------------------------------------------------- */
/*                               Reward Policy                                 */
/* -------------------------------------------------------------------------- */

interface IRewardPolicy {
    function onWorkloadValidated(
        bytes32 workloadKey,
        uint8 round,
        address[] calldata committeeMembers
    ) external;
}

/* -------------------------------------------------------------------------- */
/*                               Protocol Config                               */
/* -------------------------------------------------------------------------- */

interface IProtocolConfig {
    // Modules
    function stakingOps() external view returns (address);
    function committeeSelector() external view returns (address);
    function slashingPolicy() external view returns (address);
    function rewardPolicy() external view returns (address);

    // Params
    /// @notice Minimum stake required for an operator to be considered active.
    /// @dev Used by the staking module to gate operator activation/eligibility.
    function minOperatorStake() external view returns (uint256);
    function verificationBps() external view returns (uint16);
    function responseWindow() external view returns (uint256);
    function maxEscalations() external view returns (uint8);
    function baseCommitteeSize() external view returns (uint32);
    function committeeSizeGrowthBps() external view returns (uint32);
}