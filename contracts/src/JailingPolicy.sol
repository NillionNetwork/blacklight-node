// SPDX-License-Identifier: MIT
pragma solidity ^0.8.22;

import "@openzeppelin/contracts/utils/cryptography/MerkleProof.sol";
import "./Interfaces.sol";

interface IHeartbeatManagerPolicyView {
    function getVotePacked(bytes32 heartbeatKey, uint8 round, address operator) external view returns (uint256);
    function getRoundForPolicy(bytes32 heartbeatKey, uint8 round)
        external
        view
        returns (
            bool finalized,
            ISlashingPolicy.Outcome outcome,
            bytes32 committeeRoot,
            address stakingOps,
            uint64 jailDurationSec,
            uint32 committeeSize
        );
}

/// @title JailingPolicy
/// @notice No-slash policy: non-voters + incorrect voters can be jailed (forced inactive).
/// @dev Designed to be called permissionlessly after rounds finalize.
contract JailingPolicy is ISlashingPolicy {
    error NotHeartbeatManager();
    error RoundNotFinalized();
    error NotInCommittee();
    error AlreadyEnforced();
    error NotJailable();
    error ZeroJailDuration();
    error CommitteeRootMismatch();
    error UnsortedMembers();
    error ProofsLengthMismatch(uint256 operators, uint256 proofs);
    error CommitteeSizeMismatch(uint256 got, uint256 expected);

    uint256 private constant RESPONDED_BIT = 1 << 2;
    uint256 private constant VERDICT_MASK = 0x3;

    address public immutable heartbeatManager;

    struct RoundRecord {
        bool set;
        Outcome outcome;
        bytes32 committeeRoot;
        address stakingOps;
        uint64 jailDurationSec;
        uint32 committeeSize;
    }

    mapping(bytes32 => mapping(uint8 => RoundRecord)) public roundRecord;
    mapping(bytes32 => mapping(uint8 => mapping(address => bool))) public enforced;

    event RoundRecorded(
        bytes32 indexed heartbeatKey,
        uint8 indexed round,
        Outcome outcome,
        bytes32 committeeRoot,
        address stakingOps,
        uint64 jailDurationSec,
        uint32 committeeSize
    );
    event JailEnforced(bytes32 indexed heartbeatKey, uint8 indexed round, address indexed operator, uint64 until);

    constructor(address _heartbeatManager) {
        if (_heartbeatManager == address(0)) revert NotHeartbeatManager();
        heartbeatManager = _heartbeatManager;
    }

    function recordRound(bytes32 heartbeatKey, uint8 round) public {
        (bool finalized, Outcome o2, bytes32 root2, address stakingOps, uint64 jailDurationSec, uint32 committeeSize) =
            IHeartbeatManagerPolicyView(heartbeatManager).getRoundForPolicy(heartbeatKey, round);

        if (!finalized || root2 == bytes32(0)) revert RoundNotFinalized();

        RoundRecord storage rr = roundRecord[heartbeatKey][round];
        if (rr.set) return; // idempotent

        rr.set = true;
        rr.outcome = o2;
        rr.committeeRoot = root2;
        rr.stakingOps = stakingOps;
        rr.jailDurationSec = jailDurationSec;
        rr.committeeSize = committeeSize;

        emit RoundRecorded(heartbeatKey, round, o2, root2, stakingOps, jailDurationSec, committeeSize);
    }

    function onRoundFinalized(
        bytes32 heartbeatKey,
        uint8 round,
        Outcome /*outcome*/,
        bytes32 /*committeeRoot*/,
        uint32 /*committeeSize*/
    ) external override {
        if (msg.sender != heartbeatManager) revert NotHeartbeatManager();
        recordRound(heartbeatKey, round);
    }

    function enforceJail(bytes32 heartbeatKey, uint8 round, address operator, bytes32[] calldata memberProof) public {
        RoundRecord memory rr = roundRecord[heartbeatKey][round];
        if (!rr.set) revert RoundNotFinalized();
        if (enforced[heartbeatKey][round][operator]) revert AlreadyEnforced();
        if (rr.jailDurationSec == 0) revert ZeroJailDuration();

        // membership proof
        bytes32 leaf = keccak256(abi.encodePacked(bytes1(0xA1), heartbeatManager, heartbeatKey, round, operator));
        if (!MerkleProof.verifyCalldata(memberProof, rr.committeeRoot, leaf)) revert NotInCommittee();

        if (!_isJailable(heartbeatKey, round, rr.outcome, operator)) revert NotJailable();

        enforced[heartbeatKey][round][operator] = true;
        uint64 until = uint64(block.timestamp + uint256(rr.jailDurationSec));
        IStakingOperators(rr.stakingOps).jail(operator, until);

        emit JailEnforced(heartbeatKey, round, operator, until);
    }

    function enforceJailMany(
        bytes32 heartbeatKey,
        uint8 round,
        address[] calldata operators,
        bytes32[][] calldata proofs
    ) external {
        uint256 n = operators.length;
        if (n != proofs.length) revert ProofsLengthMismatch(n, proofs.length);
        for (uint256 i = 0; i < n; ) {
            if (enforced[heartbeatKey][round][operators[i]]) {
                ++i;
                continue;
            }
            enforceJail(heartbeatKey, round, operators[i], proofs[i]);
            ++i;
        }
    }

    /// @notice Enforce jailing using the full sorted committee list (no individual proofs required).
    /// @dev Recomputes the committee root and checks it matches the recorded root.
    function enforceJailFromMembers(bytes32 heartbeatKey, uint8 round, address[] calldata sortedMembers) external {
        RoundRecord memory rr = roundRecord[heartbeatKey][round];
        if (!rr.set) revert RoundNotFinalized();
        if (rr.jailDurationSec == 0) revert ZeroJailDuration();
        if (sortedMembers.length != rr.committeeSize) revert CommitteeSizeMismatch(sortedMembers.length, rr.committeeSize);

        // Ensure strictly ascending + build leaves
        uint256 n = sortedMembers.length;
        bytes32[] memory leaves = new bytes32[](n);
        address last = address(0);

        for (uint256 i = 0; i < n; ) {
            address op = sortedMembers[i];
            if (op == address(0) || op <= last) revert UnsortedMembers();
            last = op;
            leaves[i] = keccak256(abi.encodePacked(bytes1(0xA1), heartbeatManager, heartbeatKey, round, op));
            ++i;
        }

        bytes32 root = _computeMerkleRoot(leaves);
        if (root != rr.committeeRoot) revert CommitteeRootMismatch();

        for (uint256 i = 0; i < n; ) {
            address op = sortedMembers[i];
            if (!enforced[heartbeatKey][round][op]) {
                if (_isJailable(heartbeatKey, round, rr.outcome, op)) {
                    enforced[heartbeatKey][round][op] = true;
                    uint64 until = uint64(block.timestamp + uint256(rr.jailDurationSec));
                    IStakingOperators(rr.stakingOps).jail(op, until);
                    emit JailEnforced(heartbeatKey, round, op, until);
                }
            }
            ++i;
        }
    }

    function _isJailable(bytes32 heartbeatKey, uint8 round, Outcome outcome, address operator) internal view returns (bool) {
        uint256 packed = IHeartbeatManagerPolicyView(heartbeatManager).getVotePacked(heartbeatKey, round, operator);
        bool responded = (packed & RESPONDED_BIT) != 0;

        if (!responded) return outcome != Outcome.Inconclusive;

        uint8 verdict = uint8(packed & VERDICT_MASK);

        if (outcome == Outcome.Inconclusive) return false;

        if (outcome == Outcome.ValidThreshold) {
            return verdict != 1;
        } else if (outcome == Outcome.InvalidThreshold) {
            return verdict != 2;
        }

        return false;
    }

    // --- Merkle helpers (commutative pair hashing, compatible with OZ MerkleProof) ---

    function _hashPair(bytes32 a, bytes32 b) internal pure returns (bytes32) {
        return a < b ? keccak256(abi.encodePacked(a, b)) : keccak256(abi.encodePacked(b, a));
    }

    function _computeMerkleRoot(bytes32[] memory leaves) internal pure returns (bytes32) {
        uint256 len = leaves.length;
        if (len == 0) return bytes32(0);

        while (len > 1) {
            uint256 nextLen = (len + 1) / 2;
            for (uint256 i = 0; i < nextLen; ) {
                uint256 idx = i * 2;
                bytes32 left = leaves[idx];
                bytes32 right = idx + 1 < len ? leaves[idx + 1] : left;
                leaves[i] = _hashPair(left, right);
                ++i;
            }
            len = nextLen;
        }
        return leaves[0];
    }
}
