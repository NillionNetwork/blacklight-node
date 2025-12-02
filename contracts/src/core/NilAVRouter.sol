// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "../interfaces/Interfaces.sol";

/// @title NilAV Router
/// @dev Integrates with StakingOperators to select nodes from active staked operators
///      - Node selection uses active operators from staking contract
///      - Node selection randomness is not cryptographically secure
///      - No HTX reassignment / timeout

contract NilAVRouter {
    // ------------------------------------------------------------------------
    // Data structures
    // ------------------------------------------------------------------------

    struct Assignment {
        address node; // nilAV node chosen for this HTX
        bool responded; // has the node responded?
        bool result; // True/False from the node
    }

    // Reference to the StakingOperators contract
    IStakingOperators public immutable stakingOperators;

    // DEPRECATED: Legacy node registry (kept for backwards compatibility)
    address[] public nodes;
    mapping(address => bool) public isNode;

    // htxId => assignment
    mapping(bytes32 => Assignment) public assignments;

    // ------------------------------------------------------------------------
    // Events
    // ------------------------------------------------------------------------

    /// @dev Only the keccak256 of rawHTX is emitted to avoid storing raw data.
    event HTXSubmitted(bytes32 indexed htxId, bytes32 indexed rawHTXHash, address indexed sender);

    event HTXAssigned(bytes32 indexed htxId, address indexed node);

    event HTXResponded(bytes32 indexed htxId, address indexed node, bool result);

    event NodeRegistered(address indexed node);
    event NodeDeregistered(address indexed node);

    // ------------------------------------------------------------------------
    // Constructor
    // ------------------------------------------------------------------------

    /// @notice Initialize the router with a reference to the staking contract
    /// @param _stakingOperators Address of the StakingOperators contract
    constructor(address _stakingOperators) {
        require(_stakingOperators != address(0), "NilAV: zero staking address");
        stakingOperators = IStakingOperators(_stakingOperators);
    }

    // ------------------------------------------------------------------------
    // Node management (DEPRECATED - use StakingOperators instead)
    // ------------------------------------------------------------------------

    /// @notice DEPRECATED: Register a new nilAV node. Use StakingOperators.registerOperator() instead.
    /// @dev Kept for backwards compatibility. New deployments should use StakingOperators.
    function registerNode(address node) external {
        require(node != address(0), "NilAV: zero address");
        require(!isNode[node], "NilAV: already registered");

        isNode[node] = true;
        nodes.push(node);

        emit NodeRegistered(node);
    }

    /// @notice DEPRECATED: Deregister a nilAV node. Use StakingOperators.deactivateOperator() instead.
    function deregisterNode(address node) external {
        require(isNode[node], "NilAV: not registered");

        isNode[node] = false;

        uint256 len = nodes.length;
        for (uint256 i = 0; i < len; ++i) {
            if (nodes[i] == node) {
                // swap and pop to keep array compact
                nodes[i] = nodes[len - 1];
                nodes.pop();
                break;
            }
        }

        emit NodeDeregistered(node);
    }

    /// @notice Returns the total number of active operators from staking contract
    function nodeCount() external view returns (uint256) {
        return stakingOperators.getActiveOperators().length;
    }

    /// @notice Returns the full list of active operators from staking contract
    function getNodes() external view returns (address[] memory) {
        return stakingOperators.getActiveOperators();
    }

    /// @notice Returns the number of legacy registered nodes (deprecated)
    function legacyNodeCount() external view returns (uint256) {
        return nodes.length;
    }

    // ------------------------------------------------------------------------
    // HTX flow
    // ------------------------------------------------------------------------

    /// @notice HTX submitted for verification.
    /// @dev Selects a node from active staked operators
    /// @param rawHTX The raw HTX payload (e.g. JSON bytes).
    /// @return htxId A deterministic ID for this HTX.
    function submitHTX(bytes calldata rawHTX) external returns (bytes32 htxId) {
        address[] memory activeOperators = stakingOperators.getActiveOperators();
        require(activeOperators.length > 0, "NilAV: no active operators");

        bytes32 rawHTXHash = keccak256(rawHTX);

        // Derive an ID from the HTX contents + sender + block info.
        htxId = keccak256(abi.encode(rawHTXHash, msg.sender, block.number));

        Assignment storage existing = assignments[htxId];
        require(existing.node == address(0), "NilAV: HTX already exists");

        address chosenNode = _chooseNode(htxId, activeOperators);

        assignments[htxId] = Assignment({node: chosenNode, responded: false, result: false});

        emit HTXSubmitted(htxId, rawHTXHash, msg.sender);
        emit HTXAssigned(htxId, chosenNode);
    }

    /// @notice nilAV node responds to an assigned HTX with True/False.
    /// @param htxId The ID of the HTX (from `submitHTX` / events).
    /// @param result True/False result of the verification.
    function respondHTX(bytes32 htxId, bool result) external {
        Assignment storage a = assignments[htxId];
        require(a.node != address(0), "NilAV: unknown HTX");
        require(msg.sender == a.node, "NilAV: not assigned node");
        require(!a.responded, "NilAV: already responded");

        a.responded = true;
        a.result = result;

        emit HTXResponded(htxId, msg.sender, result);
    }

    // ------------------------------------------------------------------------
    // Views
    // ------------------------------------------------------------------------

    function getAssignment(bytes32 htxId) external view returns (Assignment memory) {
        return assignments[htxId];
    }

    // ------------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------------

    /// @dev Selects a node from the active operators using pseudo-random selection
    /// @param htxId The HTX ID for additional randomness
    /// @param activeOperators Array of active operators to choose from
    /// @return Selected operator address
    function _chooseNode(bytes32 htxId, address[] memory activeOperators) internal view returns (address) {
        require(activeOperators.length > 0, "NilAV: no active operators");

        uint256 rand = uint256(keccak256(abi.encode(block.prevrandao, block.timestamp, htxId)));
        return activeOperators[rand % activeOperators.length];
    }
}
