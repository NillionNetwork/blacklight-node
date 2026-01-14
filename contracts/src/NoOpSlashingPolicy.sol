// SPDX-License-Identifier: MIT
pragma solidity ^0.8.22;

import "./Interfaces.sol";

/// @notice Slashing policy stub that intentionally performs no action.
contract NoOpSlashingPolicy is ISlashingPolicy {
    function onRoundFinalized(
        bytes32, /* heartbeatKey */
        uint8,   /* round */
        Outcome, /* outcome */
        bytes32, /* committeeRoot */
        uint32   /* committeeSize */
    ) external override {
        // no-op
    }
}
