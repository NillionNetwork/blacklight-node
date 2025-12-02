// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";
import "src/core/NilAVRouter.sol";
import "src/core/StakingOperators.sol";
import "src/core/TESTToken.sol";
import "src/interfaces/Interfaces.sol";

/// @title NilAVRouter Test Suite
/// @notice Comprehensive tests for the NilAVRouter contract with stake-based multi-node assignment
contract NilAVRouterTest is Test {
    NilAVRouter public router;
    StakingOperators public stakingOps;
    TESTToken public token;

    // Test addresses
    address public deployer = address(this);
    address public operator1 = address(0x1);
    address public operator2 = address(0x2);
    address public operator3 = address(0x3);
    address public operator4 = address(0x4);
    address public user1 = address(0x5);
    address public user2 = address(0x6);

    // Events to test
    event HTXSubmitted(bytes32 indexed htxId, bytes32 indexed rawHTXHash, address indexed sender);
    event HTXAssigned(bytes32 indexed htxId, address indexed node);
    event HTXResponded(bytes32 indexed htxId, address indexed node, bool result);

    function setUp() public {
        // Deploy token
        token = new TESTToken(deployer);

        // Deploy staking contract
        stakingOps = new StakingOperators(IERC20(address(token)), deployer, 7 days);

        // Deploy router with staking contract
        router = new NilAVRouter(address(stakingOps));

        // Mint tokens for testing
        token.mint(deployer, 1000 ether);

        // Register operators and stake tokens
        _registerAndStakeOperator(operator1, 10 ether);
        _registerAndStakeOperator(operator2, 20 ether);
        _registerAndStakeOperator(operator3, 30 ether);
        _registerAndStakeOperator(operator4, 40 ether);
    }

    function _registerAndStakeOperator(address operator, uint256 amount) internal {
        token.approve(address(stakingOps), amount);
        stakingOps.stakeTo(operator, amount);

        vm.startPrank(operator);
        stakingOps.registerOperator("");
        vm.stopPrank();
    }

    // ========================================================================
    // Constructor & Initialization Tests
    // ========================================================================

    function testConstructorWithValidStakingAddress() public {
        NilAVRouter newRouter = new NilAVRouter(address(stakingOps));
        assertEq(address(newRouter.stakingOperators()), address(stakingOps));
    }

    function testConstructorRevertsWithZeroAddress() public {
        vm.expectRevert("NilAV: zero staking address");
        new NilAVRouter(address(0));
    }

    function testConstantsAreSetCorrectly() public view {
        assertEq(router.MIN_STAKE_BPS(), 1000, "MIN_STAKE_BPS should be 1000 (10%)");
        assertEq(router.BPS_DENOMINATOR(), 10000, "BPS_DENOMINATOR should be 10000");
    }

    // ========================================================================
    // Node Count & Getter Tests
    // ========================================================================

    function testNodeCountReturnsActiveOperators() public view {
        assertEq(router.nodeCount(), 4, "Should have 4 active operators");
    }

    function testGetNodesReturnsActiveOperators() public view {
        address[] memory nodes = router.getNodes();
        assertEq(nodes.length, 4, "Should return 4 operators");
        assertEq(nodes[0], operator1);
        assertEq(nodes[1], operator2);
        assertEq(nodes[2], operator3);
        assertEq(nodes[3], operator4);
    }

    function testNodeCountUpdatesWhenOperatorDeactivates() public {
        vm.prank(operator1);
        stakingOps.deactivateOperator();

        assertEq(router.nodeCount(), 3, "Should have 3 active operators after deactivation");
    }

    // ========================================================================
    // HTX Submission Tests - Multi-Node Assignment
    // ========================================================================

    function testSubmitHTXAssignsMultipleNodes() public {
        bytes memory htxData = bytes('{"workload_id":{"current":1,"previous":0}}');
        bytes32 htxId = router.submitHTX(htxData);

        address[] memory assignedNodes = router.getAssignedNodes(htxId);
        assertTrue(assignedNodes.length > 0, "Should assign at least one node");
        assertTrue(assignedNodes.length <= 4, "Should not assign more nodes than available");
    }

    function testSubmitHTXMeets10PercentStakeRequirement() public {
        bytes memory htxData = bytes('{"workload_id":{"current":1,"previous":0}}');
        bytes32 htxId = router.submitHTX(htxData);

        (address[] memory nodes, uint256 requiredStake, uint256 assignedStake, uint256 respondedCount) =
            router.getAssignmentInfo(htxId);

        uint256 totalStake = stakingOps.totalStaked(); // 100 ether
        uint256 expectedMinStake = (totalStake * 1000) / 10000; // 10 ether

        assertEq(requiredStake, expectedMinStake, "Required stake should be 10% of total");
        assertGe(assignedStake, requiredStake, "Assigned stake should meet requirement");
        assertEq(respondedCount, 0, "No responses yet");
        assertTrue(nodes.length > 0, "Should have assigned nodes");
    }

    function testSubmitHTXEmitsMultipleAssignedEvents() public {
        bytes memory htxData = bytes('{"workload_id":{"current":1,"previous":0}}');

        // We can't predict exact number of nodes, but at least 1 should be assigned
        vm.recordLogs();
        bytes32 htxId = router.submitHTX(htxData);

        Vm.Log[] memory logs = vm.getRecordedLogs();

        // Count HTXAssigned events
        uint256 assignedEventCount = 0;
        for (uint256 i = 0; i < logs.length; i++) {
            if (logs[i].topics[0] == keccak256("HTXAssigned(bytes32,address)")) {
                assignedEventCount++;
            }
        }

        assertTrue(assignedEventCount > 0, "Should emit at least one HTXAssigned event");

        // Verify assignment count matches events
        address[] memory assignedNodes = router.getAssignedNodes(htxId);
        assertEq(assignedEventCount, assignedNodes.length, "Event count should match assigned nodes");
    }

    function testSubmitHTXEmitsHTXSubmittedEvent() public {
        bytes memory htxData = bytes('{"workload_id":{"current":1,"previous":0}}');
        bytes32 expectedHash = keccak256(htxData);

        vm.expectEmit(false, true, true, true);
        emit HTXSubmitted(bytes32(0), expectedHash, address(this));

        router.submitHTX(htxData);
    }

    function testSubmitHTXGeneratesUniqueIds() public {
        bytes memory htxData1 = bytes('{"workload_id":{"current":1}}');
        bytes memory htxData2 = bytes('{"workload_id":{"current":2}}');

        bytes32 htxId1 = router.submitHTX(htxData1);
        bytes32 htxId2 = router.submitHTX(htxData2);

        assertTrue(htxId1 != htxId2, "HTX IDs should be unique");
    }

    function testSubmitHTXRevertsWithNoActiveOperators() public {
        // Deactivate all operators
        vm.prank(operator1);
        stakingOps.deactivateOperator();
        vm.prank(operator2);
        stakingOps.deactivateOperator();
        vm.prank(operator3);
        stakingOps.deactivateOperator();
        vm.prank(operator4);
        stakingOps.deactivateOperator();

        bytes memory htxData = bytes('{"test":"data"}');

        vm.expectRevert("NilAV: no active operators");
        router.submitHTX(htxData);
    }

    function testSubmitHTXRevertsWithNoActiveOperators_NoStake() public {
        // Deploy new staking contract with no stake
        StakingOperators emptyStaking = new StakingOperators(IERC20(address(token)), deployer, 7 days);
        NilAVRouter emptyRouter = new NilAVRouter(address(emptyStaking));

        // Try to submit HTX without any operators
        bytes memory htxData = bytes('{"test":"data"}');

        vm.expectRevert("NilAV: no active operators");
        emptyRouter.submitHTX(htxData);
    }

    function testCannotSubmitDuplicateHTX() public {
        bytes memory htxData = bytes('{"workload_id":{"current":1}}');

        router.submitHTX(htxData);

        vm.expectRevert("NilAV: HTX already exists");
        router.submitHTX(htxData);
    }

    // ========================================================================
    // HTX Response Tests - Multi-Node
    // ========================================================================

    function testRespondHTXByAssignedNode() public {
        bytes memory htxData = bytes('{"test":"data"}');
        bytes32 htxId = router.submitHTX(htxData);

        address[] memory assignedNodes = router.getAssignedNodes(htxId);
        address firstNode = assignedNodes[0];

        vm.prank(firstNode);
        router.respondHTX(htxId, true);

        (bool responded, bool result) = router.hasNodeResponded(htxId, firstNode);
        assertTrue(responded, "Node should have responded");
        assertTrue(result, "Result should be true");
    }

    function testMultipleNodesCanRespond() public {
        bytes memory htxData = bytes('{"test":"data"}');
        bytes32 htxId = router.submitHTX(htxData);

        address[] memory assignedNodes = router.getAssignedNodes(htxId);

        // Have each node respond
        for (uint256 i = 0; i < assignedNodes.length; i++) {
            vm.prank(assignedNodes[i]);
            router.respondHTX(htxId, i % 2 == 0); // Alternate true/false
        }

        // Verify all responded
        assertTrue(router.allNodesResponded(htxId), "All nodes should have responded");

        (,,, uint256 respondedCount) = router.getAssignmentInfo(htxId);
        assertEq(respondedCount, assignedNodes.length, "Responded count should match node count");
    }

    function testRespondHTXEmitsEvent() public {
        bytes memory htxData = bytes('{"test":"data"}');
        bytes32 htxId = router.submitHTX(htxData);

        address[] memory assignedNodes = router.getAssignedNodes(htxId);
        address node = assignedNodes[0];

        vm.expectEmit(true, true, false, true);
        emit HTXResponded(htxId, node, true);

        vm.prank(node);
        router.respondHTX(htxId, true);
    }

    function testCannotRespondToUnknownHTX() public {
        bytes32 fakeHtxId = keccak256("fake");

        vm.prank(operator1);
        vm.expectRevert("NilAV: unknown HTX");
        router.respondHTX(fakeHtxId, true);
    }

    function testCannotRespondIfNotAssigned() public {
        bytes memory htxData = bytes('{"test":"data"}');
        bytes32 htxId = router.submitHTX(htxData);

        address[] memory assignedNodes = router.getAssignedNodes(htxId);

        // Find an operator that wasn't assigned
        address unassignedNode;
        address[] memory allOperators = new address[](4);
        allOperators[0] = operator1;
        allOperators[1] = operator2;
        allOperators[2] = operator3;
        allOperators[3] = operator4;

        for (uint256 i = 0; i < allOperators.length; i++) {
            bool isAssigned = false;
            for (uint256 j = 0; j < assignedNodes.length; j++) {
                if (allOperators[i] == assignedNodes[j]) {
                    isAssigned = true;
                    break;
                }
            }
            if (!isAssigned) {
                unassignedNode = allOperators[i];
                break;
            }
        }

        if (unassignedNode != address(0)) {
            vm.prank(unassignedNode);
            vm.expectRevert("NilAV: not assigned node");
            router.respondHTX(htxId, true);
        }
    }

    function testCannotRespondTwice() public {
        bytes memory htxData = bytes('{"test":"data"}');
        bytes32 htxId = router.submitHTX(htxData);

        address[] memory assignedNodes = router.getAssignedNodes(htxId);
        address node = assignedNodes[0];

        vm.startPrank(node);
        router.respondHTX(htxId, true);

        vm.expectRevert("NilAV: already responded");
        router.respondHTX(htxId, false);
        vm.stopPrank();
    }

    // ========================================================================
    // View Function Tests
    // ========================================================================

    function testGetAssignedNodes() public {
        bytes memory htxData = bytes('{"test":"data"}');
        bytes32 htxId = router.submitHTX(htxData);

        address[] memory nodes = router.getAssignedNodes(htxId);
        assertTrue(nodes.length > 0, "Should return assigned nodes");

        // Verify all nodes are active operators
        address[] memory activeOps = stakingOps.getActiveOperators();
        for (uint256 i = 0; i < nodes.length; i++) {
            bool found = false;
            for (uint256 j = 0; j < activeOps.length; j++) {
                if (nodes[i] == activeOps[j]) {
                    found = true;
                    break;
                }
            }
            assertTrue(found, "Assigned node should be an active operator");
        }
    }

    function testAllNodesRespondedReturnsFalseInitially() public {
        bytes memory htxData = bytes('{"test":"data"}');
        bytes32 htxId = router.submitHTX(htxData);

        assertFalse(router.allNodesResponded(htxId), "Should return false before responses");
    }

    function testAllNodesRespondedReturnsTrueAfterAllRespond() public {
        bytes memory htxData = bytes('{"test":"data"}');
        bytes32 htxId = router.submitHTX(htxData);

        address[] memory assignedNodes = router.getAssignedNodes(htxId);

        // All nodes respond
        for (uint256 i = 0; i < assignedNodes.length; i++) {
            vm.prank(assignedNodes[i]);
            router.respondHTX(htxId, true);
        }

        assertTrue(router.allNodesResponded(htxId), "Should return true after all respond");
    }

    // ========================================================================
    // Stake Distribution Tests
    // ========================================================================

    function testDifferentStakeDistributionsSelectCorrectly() public {
        // Test with operators having vastly different stakes
        // operator1: 10 ether, operator2: 20 ether, operator3: 30 ether, operator4: 40 ether
        // Total: 100 ether, Required: 10 ether

        bytes memory htxData = bytes('{"test":"data"}');
        bytes32 htxId = router.submitHTX(htxData);

        (address[] memory nodes, uint256 requiredStake, uint256 assignedStake,) = router.getAssignmentInfo(htxId);

        assertEq(requiredStake, 10 ether, "Required should be 10 ether");
        assertGe(assignedStake, 10 ether, "Assigned should be >= 10 ether");

        // With these stakes, could be 1 node (operator4) or multiple smaller nodes
        assertTrue(nodes.length >= 1 && nodes.length <= 4, "Should select appropriate number of nodes");
    }

    function testSingleLargeStakerIsAssigned() public {
        // Deploy new setup where one operator has >10% stake
        TESTToken newToken = new TESTToken(deployer);
        StakingOperators newStaking = new StakingOperators(IERC20(address(newToken)), deployer, 7 days);
        NilAVRouter newRouter = new NilAVRouter(address(newStaking));

        newToken.mint(deployer, 1000 ether);

        // Register operator with large stake
        newToken.approve(address(newStaking), 100 ether);
        newStaking.stakeTo(operator1, 100 ether);

        vm.prank(operator1);
        newStaking.registerOperator("");

        bytes memory htxData = bytes('{"test":"data"}');
        bytes32 htxId = newRouter.submitHTX(htxData);

        address[] memory nodes = newRouter.getAssignedNodes(htxId);
        assertEq(nodes.length, 1, "Should only need one operator");
        assertEq(nodes[0], operator1, "Should select the large staker");
    }

    // ========================================================================
    // Edge Cases
    // ========================================================================

    function testSubmitHTXWithEmptyData() public {
        bytes memory emptyData = bytes("");
        bytes32 htxId = router.submitHTX(emptyData);

        assertTrue(htxId != bytes32(0), "Should generate valid HTX ID even with empty data");
        address[] memory nodes = router.getAssignedNodes(htxId);
        assertTrue(nodes.length > 0, "Should still assign nodes");
    }

    function testSubmitHTXWithLargeData() public {
        bytes memory largeData = new bytes(10000);
        for (uint256 i = 0; i < largeData.length; i++) {
            largeData[i] = bytes1(uint8(i % 256));
        }

        bytes32 htxId = router.submitHTX(largeData);
        assertTrue(htxId != bytes32(0), "Should handle large data");
    }

    function testMultipleHTXSubmissionsAssignDifferently() public {
        bytes32[] memory htxIds = new bytes32[](5);

        for (uint256 i = 0; i < 5; i++) {
            bytes memory htxData = abi.encodePacked('{"index":', i, "}");
            htxIds[i] = router.submitHTX(htxData);
        }

        // Check that assignments vary (randomness working)
        address[] memory firstAssignment = router.getAssignedNodes(htxIds[0]);
        bool foundDifferent = false;

        for (uint256 i = 1; i < 5; i++) {
            address[] memory assignment = router.getAssignedNodes(htxIds[i]);
            if (assignment.length != firstAssignment.length) {
                foundDifferent = true;
                break;
            }
            for (uint256 j = 0; j < assignment.length; j++) {
                if (assignment[j] != firstAssignment[j]) {
                    foundDifferent = true;
                    break;
                }
            }
            if (foundDifferent) break;
        }

        // Note: This might occasionally fail due to randomness, but unlikely with 5 submissions
        assertTrue(foundDifferent, "Should have some variation in assignments");
    }
}
