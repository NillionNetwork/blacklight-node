// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "./Interfaces.sol";

/// @title NoOpSlashingPolicy
/// @notice v1 slashing policy (deliberately does nothing for now)
contract NoOpSlashingPolicy is ISlashingPolicy {
    function onRoundFinalized(
        bytes32,
        uint8,
        Outcome,
        address[] calldata
    ) external pure override {
        // intentionally empty
    }
}