// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title NilAV Router Stub
/// @dev This is a stub:
///      - Anyone can register/deregister nodes
///      - Node selection randomness is not secure
///      - No HTX reassignment / timeout

contract NilAVRouter{
    // ------------------------------------------------------------------------
    // Data structures
    // ------------------------------------------------------------------------

    struct Assignment {
        address node;    // nilAV node chosen for this HTX
        bool responded;  // has the node responded?
        bool result;     // True/False from the node
    }

    // List of registered nilAV nodes
    address[] public nodes;
    mapping(address => bool) public isNode;

    // htxId => assignment
    mapping(bytes32 => Assignment) public assignments;
    mapping(bytes32 => bytes) public htxs;

    // ------------------------------------------------------------------------
    // Events
    // ------------------------------------------------------------------------

    /// @dev Only the keccak256 of rawHTX is emitted to avoid storing raw data.
    event HTXSubmitted(
        bytes32 indexed htxId,
        bytes32 indexed rawHTXHash,
        address indexed sender
    );

    event HTXAssigned(bytes32 indexed htxId, address indexed node);

    event HTXResponded(bytes32 indexed htxId, address indexed node, bool result);

    event NodeRegistered(address indexed node);
    event NodeDeregistered(address indexed node);

    // ------------------------------------------------------------------------
    // Node management (public, no access control — stub)
    // ------------------------------------------------------------------------

    /// @notice Register a new nilAV node.
    function registerNode(address node) external {
        require(node != address(0), "NilAV: zero address");
        require(!isNode[node], "NilAV: already registered");

        isNode[node] = true;
        nodes.push(node);

        emit NodeRegistered(node);
    }

    /// @notice Deregister a nilAV node.
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

    /// @notice Returns the total number of registered nodes.
    function nodeCount() external view returns (uint256) {
        return nodes.length;
    }

    /// @notice Returns the full list of registered nodes.
    function getNodes() external view returns (address[] memory) {
        return nodes;
    }

    // ------------------------------------------------------------------------
    // HTX flow
    // ------------------------------------------------------------------------

    /// @notice HTX submitted for verification.
    /// @param rawHTX The raw HTX payload (e.g. JSON bytes).
    /// @return htxId A deterministic ID for this HTX.
    function submitHTX(bytes calldata rawHTX) external returns (bytes32 htxId) {
        require(nodes.length > 0, "NilAV: no nodes registered");

        bytes32 rawHTXHash = keccak256(rawHTX);

        // Derive an ID from the HTX contents + sender + block info.
        htxId = keccak256(abi.encode(rawHTXHash, msg.sender, block.number));

        htxs[htxId] = rawHTX;
        
        Assignment storage existing = assignments[htxId];
        require(existing.node == address(0), "NilAV: HTX already exists");

        address chosenNode = _chooseNode(htxId);

        assignments[htxId] = Assignment({
            node: chosenNode,
            responded: false,
            result: false
        });

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

    function getHTX(bytes32 htxId) external view returns (bytes memory) {
        return htxs[htxId];
    }

    // ------------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------------

    function _chooseNode(bytes32 htxId) internal view returns (address) {
        require(nodes.length > 0, "NilAV: no nodes");

        uint256 rand = uint256(
            keccak256(abi.encode(block.prevrandao, block.timestamp, htxId))
        );
        return nodes[rand % nodes.length];
    }
}