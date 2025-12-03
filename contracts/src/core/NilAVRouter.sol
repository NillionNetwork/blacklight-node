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

    struct NodeResponse {
        bool responded;
        bool result;
    }

    struct Assignment {
        address[] nodes; // Array of nodes assigned to this HTX
        mapping(address => NodeResponse) responses; // Track each node's response
        uint256 requiredStake; // Minimum stake required (10% of total)
        uint256 assignedStake; // Total stake of assigned nodes
        uint256 respondedCount; // Number of nodes that have responded
    }

    // Reference to the StakingOperators contract
    IStakingOperators public immutable stakingOperators;

    // Minimum percentage of total stake required (in basis points: 1000 = 10%)
    uint256 public constant MIN_STAKE_BPS = 50000; // 50%
    uint256 public constant BPS_DENOMINATOR = 100000; // 100%

    // htxId => assignment
    mapping(bytes32 => Assignment) public assignments;

    // ------------------------------------------------------------------------
    // Events
    // ------------------------------------------------------------------------

    /// @dev Only the keccak256 of rawHTX is emitted to avoid storing raw data.
    event HTXSubmitted(bytes32 indexed htxId, bytes32 indexed rawHTXHash, address indexed sender);

    event HTXAssigned(bytes32 indexed htxId, address indexed node);

    event HTXResponded(bytes32 indexed htxId, address indexed node, bool result);

    // ------------------------------------------------------------------------
    // Constructor
    // ------------------------------------------------------------------------

    /// @notice Initialize the router with a reference to the staking contract
    /// @param _stakingOperators Address of the StakingOperators contract
    constructor(address _stakingOperators) {
        require(_stakingOperators != address(0), "NilAV: zero staking address");
        stakingOperators = IStakingOperators(_stakingOperators);
    }

    /// @notice Returns the total number of active operators from staking contract
    function nodeCount() external view returns (uint256) {
        return stakingOperators.getActiveOperators().length;
    }

    /// @notice Returns the full list of active operators from staking contract
    function getNodes() external view returns (address[] memory) {
        return stakingOperators.getActiveOperators();
    }

    // ------------------------------------------------------------------------
    // HTX flow
    // ------------------------------------------------------------------------

    /// @notice HTX submitted for verification.
    /// @dev Selects multiple nodes to ensure at least 10% of total stake is assigned
    /// @param rawHTX The raw HTX payload (e.g. JSON bytes).
    /// @return htxId A deterministic ID for this HTX.
    function submitHTX(bytes calldata rawHTX) external returns (bytes32 htxId) {
        address[] memory activeOperators = stakingOperators.getActiveOperators();
        require(activeOperators.length > 0, "NilAV: no active operators");

        bytes32 rawHTXHash = keccak256(rawHTX);

        // Derive an ID from the HTX contents + sender + block info.
        htxId = keccak256(abi.encode(rawHTXHash, msg.sender, block.number));

        Assignment storage assignment = assignments[htxId];
        require(assignment.nodes.length == 0, "NilAV: HTX already exists");

        // Get total stake and calculate minimum required
        uint256 totalStake = stakingOperators.totalStaked();
        require(totalStake > 0, "NilAV: no stake in system");

        uint256 requiredStake = (totalStake * MIN_STAKE_BPS) / BPS_DENOMINATOR;

        // Select nodes until we reach the required stake
        address[] memory selectedNodes = _selectNodesByStake(htxId, activeOperators, requiredStake);
        require(selectedNodes.length > 0, "NilAV: could not select nodes");

        // Calculate total assigned stake
        uint256 assignedStake = 0;
        for (uint256 i = 0; i < selectedNodes.length; i++) {
            assignedStake += stakingOperators.stakeOf(selectedNodes[i]);
        }

        // Initialize assignment
        assignment.requiredStake = requiredStake;
        assignment.assignedStake = assignedStake;
        assignment.respondedCount = 0;

        // Store nodes and emit events
        for (uint256 i = 0; i < selectedNodes.length; i++) {
            assignment.nodes.push(selectedNodes[i]);
            emit HTXAssigned(htxId, selectedNodes[i]);
        }

        emit HTXSubmitted(htxId, rawHTXHash, msg.sender);
    }

    /// @notice nilAV node responds to an assigned HTX with True/False.
    /// @param htxId The ID of the HTX (from `submitHTX` / events).
    /// @param result True/False result of the verification.
    function respondHTX(bytes32 htxId, bool result) external {
        Assignment storage a = assignments[htxId];
        require(a.nodes.length > 0, "NilAV: unknown HTX");

        // Check if sender is one of the assigned nodes
        bool isAssigned = false;
        for (uint256 i = 0; i < a.nodes.length; i++) {
            if (a.nodes[i] == msg.sender) {
                isAssigned = true;
                break;
            }
        }
        require(isAssigned, "NilAV: not assigned node");

        // Check if already responded
        require(!a.responses[msg.sender].responded, "NilAV: already responded");

        // Record response
        a.responses[msg.sender].responded = true;
        a.responses[msg.sender].result = result;
        a.respondedCount++;

        emit HTXResponded(htxId, msg.sender, result);
    }

    // ------------------------------------------------------------------------
    // Views
    // ------------------------------------------------------------------------

    /// @notice Get the assigned nodes for an HTX
    function getAssignedNodes(bytes32 htxId) external view returns (address[] memory) {
        return assignments[htxId].nodes;
    }

    /// @notice Get assignment details for an HTX
    function getAssignmentInfo(bytes32 htxId)
        external
        view
        returns (address[] memory nodes, uint256 requiredStake, uint256 assignedStake, uint256 respondedCount)
    {
        Assignment storage a = assignments[htxId];
        return (a.nodes, a.requiredStake, a.assignedStake, a.respondedCount);
    }

    /// @notice Check if a specific node has responded to an HTX
    function hasNodeResponded(bytes32 htxId, address node) external view returns (bool responded, bool result) {
        NodeResponse storage response = assignments[htxId].responses[node];
        return (response.responded, response.result);
    }

    /// @notice Check if all assigned nodes have responded
    function allNodesResponded(bytes32 htxId) external view returns (bool) {
        Assignment storage a = assignments[htxId];
        return a.respondedCount == a.nodes.length;
    }

    // ------------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------------

    /// @dev Selects nodes until the combined stake meets the required threshold
    /// @param htxId The HTX ID for randomness
    /// @param activeOperators Array of active operators to choose from
    /// @param requiredStake Minimum total stake needed
    /// @return selectedNodes Array of selected operator addresses
    function _selectNodesByStake(bytes32 htxId, address[] memory activeOperators, uint256 requiredStake)
        internal
        view
        returns (address[] memory selectedNodes)
    {
        uint256 len = activeOperators.length;
        require(len != 0, "NilAV: no active operators");

        // Pseudo-random seed
        uint256 seed = uint256(keccak256(abi.encodePacked(block.prevrandao, htxId)));

        uint256 totalSelectedStake;
        uint256 selectedCount;

        // We will:
        //  - do a Fisher–Yates style shuffle in-place on `activeOperators`
        //  - treat the *front* of the array [0 .. selectedCount-1] as the selected set
        //
        // Loop invariant: the segment [0 .. i-1] has already been processed.
        for (uint256 i; i < len && totalSelectedStake < requiredStake;) {
            // Fisher–Yates: pick random index in [i, len-1]
            uint256 remaining = len - i; // How many elements left to shuffle
            uint256 j = i + (seed % remaining); // Pick a random index in [i, len-1]

            // Swap activeOperators[i] and activeOperators[j]
            (activeOperators[i], activeOperators[j]) = (activeOperators[j], activeOperators[i]);

            // Get operator stake
            uint256 operatorStake = stakingOperators.stakeOf(activeOperators[i]);

            if (operatorStake != 0) { // Just consider in case of active operators
                // Ensure selected operators are packed at the front (e.g., if any operator wasn't selected in the previous iteration of the loop)
                //  - if i > selectedCount, swap to keep selected ones in [0..selectedCount-1]
                if (i != selectedCount) {
                    (activeOperators[i], activeOperators[selectedCount]) =
                    (activeOperators[selectedCount], activeOperators[i]);
                }

                totalSelectedStake += operatorStake;
                unchecked {
                    ++selectedCount; // Safe because we know i < len and len max is 2**256
                }
            }

            // Update seed & increment i
            seed = uint256(keccak256(abi.encodePacked(seed, i)));
            unchecked {
                ++i; // Safe because we know i < len and len max is 2**256
            }
        }

        // Now the first `selectedCount` items of `activeOperators` are our result
        selectedNodes = new address[](selectedCount);
        for (uint256 i; i < selectedCount;) {
            selectedNodes[i] = activeOperators[i];
            unchecked {
                ++i;
            }
        }
    }
}
