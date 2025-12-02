// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "src/core/NilAVRouter.sol";

/// @title NilAVRouter Test Suite
/// @notice Comprehensive tests for the NilAVRouter contract
contract NilAVRouterTest is Test {
    NilAVRouter public router;

    // Test addresses
    address public node1 = address(0x1);
    address public node2 = address(0x2);
    address public node3 = address(0x3);
    address public user1 = address(0x4);
    address public user2 = address(0x5);

    // Events to test
    event NodeRegistered(address indexed node);
    event NodeDeregistered(address indexed node);
    event HTXSubmitted(bytes32 indexed htxId, bytes32 indexed rawHTXHash, address indexed sender);
    event HTXAssigned(bytes32 indexed htxId, address indexed node);
    event HTXResponded(bytes32 indexed htxId, address indexed node, bool result);

    function setUp() public {
        router = new NilAVRouter();
    }

    // ========================================================================
    // Node Registration Tests
    // ========================================================================

    function testRegisterNode() public {
        // Register node and verify
        router.registerNode(node1);

        assertTrue(router.isNode(node1), "Node should be registered");
        assertEq(router.nodeCount(), 1, "Node count should be 1");

        // Verify node appears in list
        address[] memory nodes = router.getNodes();
        assertEq(nodes.length, 1, "Nodes array length should be 1");
        assertEq(nodes[0], node1, "First node should be node1");
    }

    function testRegisterMultipleNodes() public {
        router.registerNode(node1);
        router.registerNode(node2);
        router.registerNode(node3);

        assertEq(router.nodeCount(), 3, "Should have 3 nodes");
        assertTrue(router.isNode(node1), "node1 should be registered");
        assertTrue(router.isNode(node2), "node2 should be registered");
        assertTrue(router.isNode(node3), "node3 should be registered");

        address[] memory nodes = router.getNodes();
        assertEq(nodes.length, 3, "Nodes array should have 3 elements");
    }

    function testCannotRegisterZeroAddress() public {
        vm.expectRevert("NilAV: zero address");
        router.registerNode(address(0));
    }

    function testCannotRegisterDuplicateNode() public {
        router.registerNode(node1);

        vm.expectRevert("NilAV: already registered");
        router.registerNode(node1);
    }

    function testRegisterNodeEmitsEvent() public {
        vm.expectEmit(true, false, false, false);
        emit NodeRegistered(node1);

        router.registerNode(node1);
    }

    // ========================================================================
    // Node Deregistration Tests
    // ========================================================================

    function testDeregisterNode() public {
        // Register then deregister
        router.registerNode(node1);
        router.deregisterNode(node1);

        assertFalse(router.isNode(node1), "Node should not be registered");
        assertEq(router.nodeCount(), 0, "Node count should be 0");

        address[] memory nodes = router.getNodes();
        assertEq(nodes.length, 0, "Nodes array should be empty");
    }

    function testDeregisterNodeFromMultiple() public {
        // Register multiple nodes
        router.registerNode(node1);
        router.registerNode(node2);
        router.registerNode(node3);

        // Deregister middle node
        router.deregisterNode(node2);

        assertEq(router.nodeCount(), 2, "Should have 2 nodes");
        assertTrue(router.isNode(node1), "node1 should still be registered");
        assertFalse(router.isNode(node2), "node2 should not be registered");
        assertTrue(router.isNode(node3), "node3 should still be registered");
    }

    function testCannotDeregisterUnregisteredNode() public {
        vm.expectRevert("NilAV: not registered");
        router.deregisterNode(node1);
    }

    function testDeregisterNodeEmitsEvent() public {
        router.registerNode(node1);

        vm.expectEmit(true, false, false, false);
        emit NodeDeregistered(node1);

        router.deregisterNode(node1);
    }

    // ========================================================================
    // HTX Submission Tests
    // ========================================================================

    function testSubmitHTX() public {
        router.registerNode(node1);

        bytes memory htxData = bytes('{"workload_id":{"current":1,"previous":0}}');
        bytes32 htxId = router.submitHTX(htxData);

        // Verify HTX ID is not zero
        assertTrue(htxId != bytes32(0), "HTX ID should not be zero");

        // Verify assignment was created
        (address assignedNode, bool responded, bool result) = router.assignments(htxId);
        assertTrue(assignedNode != address(0), "Node should be assigned");
        assertEq(assignedNode, node1, "Should be assigned to node1");
        assertFalse(responded, "Should not have responded yet");
        assertFalse(result, "Result should be false initially");
    }

    function testSubmitHTXWithMultipleNodes() public {
        router.registerNode(node1);
        router.registerNode(node2);
        router.registerNode(node3);

        bytes memory htxData = bytes('{"test": "data"}');
        bytes32 htxId = router.submitHTX(htxData);

        // Get assignment
        (address assignedNode,,) = router.assignments(htxId);

        // Should be assigned to one of the registered nodes
        assertTrue(
            assignedNode == node1 || assignedNode == node2 || assignedNode == node3,
            "Should be assigned to a registered node"
        );
    }

    function testCannotSubmitHTXWithNoNodes() public {
        vm.expectRevert("NilAV: no nodes registered");
        router.submitHTX(bytes("test"));
    }

    function testCannotSubmitDuplicateHTX() public {
        router.registerNode(node1);

        bytes memory htxData = bytes('{"test": "data"}');

        // Submit once from user1
        vm.prank(user1);
        router.submitHTX(htxData);

        // Try to submit the same data from same address in same block
        vm.prank(user1);
        vm.expectRevert("NilAV: HTX already exists");
        router.submitHTX(htxData);
    }

    function testSubmitHTXEmitsEvents() public {
        router.registerNode(node1);

        bytes memory htxData = bytes('{"test": "data"}');
        bytes32 rawHTXHash = keccak256(htxData);

        // Calculate expected htxId
        bytes32 expectedHtxId = keccak256(abi.encode(rawHTXHash, address(this), block.number));

        // Expect HTXSubmitted event
        vm.expectEmit(true, true, true, false);
        emit HTXSubmitted(expectedHtxId, rawHTXHash, address(this));

        // Expect HTXAssigned event
        vm.expectEmit(true, true, false, false);
        emit HTXAssigned(expectedHtxId, node1);

        router.submitHTX(htxData);
    }

    function testHTXIDIsDeterministic() public {
        router.registerNode(node1);

        bytes memory htxData = bytes('{"test": "data"}');
        bytes32 rawHTXHash = keccak256(htxData);

        // Calculate expected htxId
        bytes32 expectedHtxId = keccak256(abi.encode(rawHTXHash, address(this), block.number));

        bytes32 actualHtxId = router.submitHTX(htxData);

        assertEq(actualHtxId, expectedHtxId, "HTX ID should match expected value");
    }

    function testSubmitHTXFromDifferentSenders() public {
        router.registerNode(node1);

        bytes memory htxData = bytes('{"test": "data"}');

        // Submit from user1
        vm.prank(user1);
        bytes32 htxId1 = router.submitHTX(htxData);

        // Submit same data from user2 - should succeed with different htxId
        vm.prank(user2);
        bytes32 htxId2 = router.submitHTX(htxData);

        assertTrue(htxId1 != htxId2, "HTX IDs should be different for different senders");
    }

    // ========================================================================
    // HTX Response Tests
    // ========================================================================

    function testRespondHTXTrue() public {
        router.registerNode(node1);

        bytes memory htxData = bytes('{"test": "data"}');
        bytes32 htxId = router.submitHTX(htxData);

        // Node responds with true
        vm.prank(node1);
        router.respondHTX(htxId, true);

        // Verify response
        (address assignedNode, bool responded, bool result) = router.assignments(htxId);
        assertEq(assignedNode, node1, "Assigned node should be node1");
        assertTrue(responded, "Should have responded");
        assertTrue(result, "Result should be true");
    }

    function testRespondHTXFalse() public {
        router.registerNode(node1);

        bytes memory htxData = bytes('{"test": "data"}');
        bytes32 htxId = router.submitHTX(htxData);

        // Node responds with false
        vm.prank(node1);
        router.respondHTX(htxId, false);

        // Verify response
        (, bool responded, bool result) = router.assignments(htxId);
        assertTrue(responded, "Should have responded");
        assertFalse(result, "Result should be false");
    }

    function testCannotRespondToUnknownHTX() public {
        router.registerNode(node1);

        bytes32 fakeHtxId = keccak256("fake");

        vm.prank(node1);
        vm.expectRevert("NilAV: unknown HTX");
        router.respondHTX(fakeHtxId, true);
    }

    function testCannotRespondIfNotAssignedNode() public {
        router.registerNode(node1);
        router.registerNode(node2);

        bytes memory htxData = bytes('{"test": "data"}');
        bytes32 htxId = router.submitHTX(htxData);

        // Get assigned node
        (address assignedNode,,) = router.assignments(htxId);

        // Try to respond from non-assigned node
        address nonAssignedNode = assignedNode == node1 ? node2 : node1;

        vm.prank(nonAssignedNode);
        vm.expectRevert("NilAV: not assigned node");
        router.respondHTX(htxId, true);
    }

    function testCannotRespondTwice() public {
        router.registerNode(node1);

        bytes memory htxData = bytes('{"test": "data"}');
        bytes32 htxId = router.submitHTX(htxData);

        // First response
        vm.prank(node1);
        router.respondHTX(htxId, true);

        // Second response attempt
        vm.prank(node1);
        vm.expectRevert("NilAV: already responded");
        router.respondHTX(htxId, false);
    }

    function testRespondHTXEmitsEvent() public {
        router.registerNode(node1);

        bytes memory htxData = bytes('{"test": "data"}');
        bytes32 htxId = router.submitHTX(htxData);

        vm.expectEmit(true, true, false, true);
        emit HTXResponded(htxId, node1, true);

        vm.prank(node1);
        router.respondHTX(htxId, true);
    }

    // ========================================================================
    // View Function Tests
    // ========================================================================

    function testGetAssignment() public {
        router.registerNode(node1);

        bytes memory htxData = bytes('{"test": "data"}');
        bytes32 htxId = router.submitHTX(htxData);

        NilAVRouter.Assignment memory assignment = router.getAssignment(htxId);

        assertEq(assignment.node, node1, "Assignment node should be node1");
        assertFalse(assignment.responded, "Assignment should not have responded");
        assertFalse(assignment.result, "Assignment result should be false");
    }

    function testGetNodesReturnsCorrectList() public {
        router.registerNode(node1);
        router.registerNode(node2);

        address[] memory nodes = router.getNodes();

        assertEq(nodes.length, 2, "Should have 2 nodes");

        // Check both nodes are in the list (order doesn't matter)
        bool foundNode1 = false;
        bool foundNode2 = false;

        for (uint256 i = 0; i < nodes.length; i++) {
            if (nodes[i] == node1) foundNode1 = true;
            if (nodes[i] == node2) foundNode2 = true;
        }

        assertTrue(foundNode1, "node1 should be in the list");
        assertTrue(foundNode2, "node2 should be in the list");
    }

    function testNodeAtIndex() public {
        router.registerNode(node1);
        router.registerNode(node2);

        address firstNode = router.nodes(0);
        address secondNode = router.nodes(1);

        assertTrue(
            (firstNode == node1 && secondNode == node2) || (firstNode == node2 && secondNode == node1),
            "Nodes should be retrievable by index"
        );
    }

    // ========================================================================
    // Complex Workflow Tests
    // ========================================================================

    function testCompleteWorkflow() public {
        // 1. Register multiple nodes
        router.registerNode(node1);
        router.registerNode(node2);
        router.registerNode(node3);
        assertEq(router.nodeCount(), 3, "Should have 3 nodes");

        // 2. Submit HTX
        bytes memory htxData = bytes('{"workload_id":{"current":1,"previous":0}}');
        bytes32 htxId = router.submitHTX(htxData);

        // 3. Get assignment
        (address assignedNode, bool responded, bool result) = router.assignments(htxId);
        assertTrue(assignedNode != address(0), "Should have assigned node");

        // 4. Respond to HTX
        vm.prank(assignedNode);
        router.respondHTX(htxId, true);

        // 5. Verify response was recorded
        (, responded, result) = router.assignments(htxId);
        assertTrue(responded, "Should have responded");
        assertTrue(result, "Result should be true");

        // 6. Deregister a node that wasn't assigned
        address nodeToRemove = assignedNode == node1 ? node2 : node1;
        router.deregisterNode(nodeToRemove);
        assertEq(router.nodeCount(), 2, "Should have 2 nodes after deregistration");
    }

    function testMultipleHTXSubmissions() public {
        router.registerNode(node1);
        router.registerNode(node2);

        // Submit multiple HTXs
        bytes32[] memory htxIds = new bytes32[](5);

        for (uint256 i = 0; i < 5; i++) {
            bytes memory htxData = abi.encodePacked('{"htx":', i, "}");
            htxIds[i] = router.submitHTX(htxData);
        }

        // Verify all HTXs have assignments
        for (uint256 i = 0; i < 5; i++) {
            (address assignedNode,,) = router.assignments(htxIds[i]);
            assertTrue(assignedNode != address(0), "HTX should have assigned node");
            assertTrue(router.isNode(assignedNode), "Assigned node should be registered");
        }
    }

    // ========================================================================
    // Fuzz Tests
    // ========================================================================

    function testFuzzRegisterNode(address nodeAddr) public {
        vm.assume(nodeAddr != address(0));
        vm.assume(!router.isNode(nodeAddr));

        router.registerNode(nodeAddr);
        assertTrue(router.isNode(nodeAddr), "Node should be registered");
    }

    function testFuzzSubmitHTX(bytes calldata htxData) public {
        vm.assume(htxData.length > 0);

        router.registerNode(node1);
        bytes32 htxId = router.submitHTX(htxData);

        (address assignedNode,,) = router.assignments(htxId);
        assertEq(assignedNode, node1, "Should be assigned to node1");
    }

    function testFuzzRespondHTX(bool response) public {
        router.registerNode(node1);

        bytes memory htxData = bytes('{"test": "data"}');
        bytes32 htxId = router.submitHTX(htxData);

        vm.prank(node1);
        router.respondHTX(htxId, response);

        (,, bool result) = router.assignments(htxId);
        assertEq(result, response, "Response should match input");
    }

    // ========================================================================
    // Edge Case Tests
    // ========================================================================

    function testEmptyHTXData() public {
        router.registerNode(node1);

        bytes memory emptyData = bytes("");
        bytes32 htxId = router.submitHTX(emptyData);

        assertTrue(htxId != bytes32(0), "Should accept empty HTX data");
    }

    function testLargeHTXData() public {
        router.registerNode(node1);

        // Create large HTX data (1KB)
        bytes memory largeData = new bytes(1024);
        for (uint256 i = 0; i < 1024; i++) {
            largeData[i] = bytes1(uint8(i % 256));
        }

        bytes32 htxId = router.submitHTX(largeData);
        assertTrue(htxId != bytes32(0), "Should accept large HTX data");
    }

    function testDeregisterLastNode() public {
        router.registerNode(node1);
        router.deregisterNode(node1);

        assertEq(router.nodeCount(), 0, "Should have no nodes");

        address[] memory nodes = router.getNodes();
        assertEq(nodes.length, 0, "Nodes array should be empty");
    }

    function testRegisterAfterDeregister() public {
        router.registerNode(node1);
        router.deregisterNode(node1);
        router.registerNode(node1);

        assertTrue(router.isNode(node1), "Node should be registered again");
        assertEq(router.nodeCount(), 1, "Should have 1 node");
    }
}
