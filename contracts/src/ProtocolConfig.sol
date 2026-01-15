// SPDX-License-Identifier: MIT
pragma solidity ^0.8.22;

import "@openzeppelin/contracts/access/Ownable.sol";
import "./Interfaces.sol";

/// @title ProtocolConfig
/// @notice Governance-owned module registry + parameter store.
contract ProtocolConfig is IProtocolConfig, Ownable {
    error ZeroAddress();
    error InvalidBps(uint256 bps);
    error InvalidCommitteeCap(uint32 base, uint32 max);
    error InvalidMaxVoteBatchSize(uint256 maxBatch);
    error InvalidModuleAddress(address module);
    error ZeroQuorumBps();
    error ZeroVerificationBps();
    error ZeroResponseWindow();
    error ZeroJailDuration();
    error ZeroHeartbeatBond();

    // Modules
    address private _stakingOps;
    address private _selector;
    address private _slashing;
    address private _reward;

    // Params
    uint32 private _baseCommitteeSize;
    uint32 private _committeeSizeGrowthBps;
    uint32 private _maxCommitteeSize;

    uint8  private _maxEscalations;

    uint16 private _quorumBps;
    uint16 private _verificationBps;
    uint256 private _responseWindow;
    uint256 private _jailDuration;

    uint256 private _maxVoteBatchSize;

    uint256 private _minOperatorStake;
    uint256 private _heartbeatBond;
    uint16 private _heartbeatBondBurnBps;

    event ModulesUpdated(address stakingOps, address selector, address slashing, address reward);
    event ParamsUpdated(
        uint32 baseCommitteeSize,
        uint32 committeeSizeGrowthBps,
        uint32 maxCommitteeSize,
        uint8  maxEscalations,
        uint16 quorumBps,
        uint16 verificationBps,
        uint256 responseWindow,
        uint256 jailDuration,
        uint256 maxVoteBatchSize,
        uint256 minOperatorStake,
        uint256 heartbeatBond,
        uint16 heartbeatBondBurnBps
    );

    constructor(
        address owner_,
        address stakingOps_,
        address selector_,
        address slashing_,
        address reward_,
        // committee params
        uint32 baseCommitteeSize_,
        uint32 committeeSizeGrowthBps_,
        uint32 maxCommitteeSize_,
        // escalation
        uint8 maxEscalations_,
        // vote params
        uint16 quorumBps_,
        uint16 verificationBps_,
        uint256 responseWindow_,
        uint256 jailDuration_,
        // batching / staking
        uint256 maxVoteBatchSize_,
        uint256 minOperatorStake_,
        // heartbeat bond
        uint256 heartbeatBond_,
        uint16 heartbeatBondBurnBps_
    ) Ownable(owner_) {
        if (stakingOps_ == address(0) || selector_ == address(0) || slashing_ == address(0) || reward_ == address(0)) {
            revert ZeroAddress();
        }
        _requireContract(stakingOps_);
        _requireContract(selector_);
        _requireContract(slashing_);
        _requireContract(reward_);

        if (quorumBps_ == 0) revert ZeroQuorumBps();
        if (verificationBps_ == 0) revert ZeroVerificationBps();
        if (responseWindow_ == 0) revert ZeroResponseWindow();
        if (jailDuration_ == 0) revert ZeroJailDuration();
        if (heartbeatBond_ == 0) revert ZeroHeartbeatBond();
        _validateBps(quorumBps_);
        _validateBps(verificationBps_);
        _validateBps(committeeSizeGrowthBps_);
        _validateBps(heartbeatBondBurnBps_);
        _validateCommitteeCaps(baseCommitteeSize_, maxCommitteeSize_);
        _validateMaxVoteBatch(maxVoteBatchSize_);

        _stakingOps = stakingOps_;
        _selector = selector_;
        _slashing = slashing_;
        _reward = reward_;

        _baseCommitteeSize = baseCommitteeSize_;
        _committeeSizeGrowthBps = committeeSizeGrowthBps_;
        _maxCommitteeSize = maxCommitteeSize_;

        _maxEscalations = maxEscalations_;

        _quorumBps = quorumBps_;
        _verificationBps = verificationBps_;
        _responseWindow = responseWindow_;
        _jailDuration = jailDuration_;

        _maxVoteBatchSize = maxVoteBatchSize_;
        _minOperatorStake = minOperatorStake_;
        _heartbeatBond = heartbeatBond_;
        _heartbeatBondBurnBps = heartbeatBondBurnBps_;

        emit ModulesUpdated(stakingOps_, selector_, slashing_, reward_);
        emit ParamsUpdated(
            baseCommitteeSize_,
            committeeSizeGrowthBps_,
            maxCommitteeSize_,
            maxEscalations_,
            quorumBps_,
            verificationBps_,
            responseWindow_,
            jailDuration_,
            maxVoteBatchSize_,
            minOperatorStake_,
            heartbeatBond_,
            heartbeatBondBurnBps_
        );
    }

    function _validateBps(uint256 bps) internal pure {
        if (bps > 10_000) revert InvalidBps(bps);
    }

    function _validateCommitteeCaps(uint32 baseSize, uint32 maxSize) internal pure {
        if (baseSize == 0 || maxSize == 0 || baseSize > maxSize) revert InvalidCommitteeCap(baseSize, maxSize);
    }

    function _validateMaxVoteBatch(uint256 maxBatch) internal pure {
        // 0 = unlimited (still limited by HeartbeatManager hard limit); otherwise require sane cap.
        if (maxBatch != 0 && maxBatch > 500) revert InvalidMaxVoteBatchSize(maxBatch);
    }

    // Modules
    function stakingOps() external view override returns (address) { return _stakingOps; }
    function committeeSelector() external view override returns (address) { return _selector; }
    function slashingPolicy() external view override returns (address) { return _slashing; }
    function rewardPolicy() external view override returns (address) { return _reward; }

    // Committee sizing
    function baseCommitteeSize() external view override returns (uint32) { return _baseCommitteeSize; }
    function committeeSizeGrowthBps() external view override returns (uint32) { return _committeeSizeGrowthBps; }
    function maxCommitteeSize() external view override returns (uint32) { return _maxCommitteeSize; }

    // Escalation
    function maxEscalations() external view override returns (uint8) { return _maxEscalations; }

    // Voting / timing
    function quorumBps() external view override returns (uint16) { return _quorumBps; }
    function verificationBps() external view override returns (uint16) { return _verificationBps; }
    function responseWindow() external view override returns (uint256) { return _responseWindow; }
    function jailDuration() external view override returns (uint256) { return _jailDuration; }

    // Misc
    function maxVoteBatchSize() external view override returns (uint256) { return _maxVoteBatchSize; }
    function minOperatorStake() external view override returns (uint256) { return _minOperatorStake; }
    function heartbeatBond() external view override returns (uint256) { return _heartbeatBond; }
    function heartbeatBondBurnBps() external view override returns (uint16) { return _heartbeatBondBurnBps; }

    // Admin setters

    function setModules(address stakingOps_, address selector_, address slashing_, address reward_) external onlyOwner {
        if (stakingOps_ == address(0) || selector_ == address(0) || slashing_ == address(0) || reward_ == address(0)) {
            revert ZeroAddress();
        }
        _requireContract(stakingOps_);
        _requireContract(selector_);
        _requireContract(slashing_);
        _requireContract(reward_);
        _stakingOps = stakingOps_;
        _selector = selector_;
        _slashing = slashing_;
        _reward = reward_;
        emit ModulesUpdated(stakingOps_, selector_, slashing_, reward_);
    }

    function setParams(
        uint32 baseCommitteeSize_,
        uint32 committeeSizeGrowthBps_,
        uint32 maxCommitteeSize_,
        uint8  maxEscalations_,
        uint16 quorumBps_,
        uint16 verificationBps_,
        uint256 responseWindow_,
        uint256 jailDuration_,
        uint256 maxVoteBatchSize_,
        uint256 minOperatorStake_,
        uint256 heartbeatBond_,
        uint16 heartbeatBondBurnBps_
    ) external onlyOwner {
        if (quorumBps_ == 0) revert ZeroQuorumBps();
        if (verificationBps_ == 0) revert ZeroVerificationBps();
        if (responseWindow_ == 0) revert ZeroResponseWindow();
        if (jailDuration_ == 0) revert ZeroJailDuration();
        if (heartbeatBond_ == 0) revert ZeroHeartbeatBond();
        _validateBps(quorumBps_);
        _validateBps(verificationBps_);
        _validateBps(committeeSizeGrowthBps_);
        _validateBps(heartbeatBondBurnBps_);
        _validateCommitteeCaps(baseCommitteeSize_, maxCommitteeSize_);
        _validateMaxVoteBatch(maxVoteBatchSize_);

        _baseCommitteeSize = baseCommitteeSize_;
        _committeeSizeGrowthBps = committeeSizeGrowthBps_;
        _maxCommitteeSize = maxCommitteeSize_;

        _maxEscalations = maxEscalations_;

        _quorumBps = quorumBps_;
        _verificationBps = verificationBps_;
        _responseWindow = responseWindow_;
        _jailDuration = jailDuration_;

        _maxVoteBatchSize = maxVoteBatchSize_;
        _minOperatorStake = minOperatorStake_;
        _heartbeatBond = heartbeatBond_;
        _heartbeatBondBurnBps = heartbeatBondBurnBps_;

        emit ParamsUpdated(
            baseCommitteeSize_,
            committeeSizeGrowthBps_,
            maxCommitteeSize_,
            maxEscalations_,
            quorumBps_,
            verificationBps_,
            responseWindow_,
            jailDuration_,
            maxVoteBatchSize_,
            minOperatorStake_,
            heartbeatBond_,
            heartbeatBondBurnBps_
        );
    }

    function _requireContract(address module) internal view {
        if (module.code.length == 0) revert InvalidModuleAddress(module);
    }
}
