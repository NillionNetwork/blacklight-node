// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/access/Ownable.sol";
import "./Interfaces.sol";

/// @title ProtocolConfig
/// @notice Governance config + module registry.
contract ProtocolConfig is IProtocolConfig, Ownable {
    address private _stakingOps;
    address private _selector;
    address private _slashing;
    address private _reward;

    uint16  private _verificationBps;
    uint256 private _responseWindow;
    uint8   private _maxEscalations;
    uint32  private _baseCommitteeSize;
    uint32  private _committeeSizeGrowthBps;

    event ModulesUpdated(
        address stakingOps,
        address selector,
        address slashing,
        address reward
    );

    event ParamsUpdated(
        uint16 verificationBps,
        uint256 responseWindow,
        uint8 maxEscalations,
        uint32 baseCommitteeSize,
        uint32 committeeSizeGrowthBps
    );

    constructor(
        address owner_,
        address stakingOps_,
        address selector_,
        address slashing_,
        address reward_,
        uint16 verificationBps_,
        uint256 responseWindow_,
        uint8 maxEscalations_,
        uint32 baseCommitteeSize_,
        uint32 committeeSizeGrowthBps_
    ) Ownable(owner_) {
        _stakingOps = stakingOps_;
        _selector = selector_;
        _slashing = slashing_;
        _reward = reward_;

        _verificationBps = verificationBps_;
        _responseWindow = responseWindow_;
        _maxEscalations = maxEscalations_;
        _baseCommitteeSize = baseCommitteeSize_;
        _committeeSizeGrowthBps = committeeSizeGrowthBps_;
    }

    // IProtocolConfig views

    function stakingOps() external view override returns (address) { return _stakingOps; }
    function committeeSelector() external view override returns (address) { return _selector; }
    function slashingPolicy() external view override returns (address) { return _slashing; }
    function rewardPolicy() external view override returns (address) { return _reward; }

    function verificationBps() external view override returns (uint16) { return _verificationBps; }
    function responseWindow() external view override returns (uint256) { return _responseWindow; }
    function maxEscalations() external view override returns (uint8) { return _maxEscalations; }
    function baseCommitteeSize() external view override returns (uint32) { return _baseCommitteeSize; }
    function committeeSizeGrowthBps() external view override returns (uint32) { return _committeeSizeGrowthBps; }

    // Admin setters

    function setModules(
        address stakingOps_,
        address selector_,
        address slashing_,
        address reward_
    ) external onlyOwner {
        _stakingOps = stakingOps_;
        _selector = selector_;
        _slashing = slashing_;
        _reward = reward_;
        emit ModulesUpdated(stakingOps_, selector_, slashing_, reward_);
    }

    function setParams(
        uint16 verificationBps_,
        uint256 responseWindow_,
        uint8 maxEscalations_,
        uint32 baseCommitteeSize_,
        uint32 committeeSizeGrowthBps_
    ) external onlyOwner {
        _verificationBps = verificationBps_;
        _responseWindow = responseWindow_;
        _maxEscalations = maxEscalations_;
        _baseCommitteeSize = baseCommitteeSize_;
        _committeeSizeGrowthBps = committeeSizeGrowthBps_;
        emit ParamsUpdated(
            verificationBps_,
            responseWindow_,
            maxEscalations_,
            baseCommitteeSize_,
            committeeSizeGrowthBps_
        );
    }
}