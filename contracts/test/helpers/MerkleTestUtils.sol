// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @notice Test-only Merkle utilities compatible with OpenZeppelin MerkleProof.
/// @dev Pair hashing is commutative (sorted), matching OZ MerkleProof and the on-chain contracts.
library MerkleTestUtils {
    function leaf(address workloadManager, bytes32 workloadKey, uint8 round, address member) internal pure returns (bytes32) {
        return keccak256(abi.encodePacked(bytes1(0xA1), workloadManager, workloadKey, round, member));
    }

    function hashPair(bytes32 a, bytes32 b) internal pure returns (bytes32) {
        return a < b ? keccak256(abi.encodePacked(a, b)) : keccak256(abi.encodePacked(b, a));
    }

    function computeRoot(bytes32[] memory leaves) internal pure returns (bytes32) {
        uint256 len = leaves.length;
        if (len == 0) return bytes32(0);
        while (len > 1) {
            uint256 nextLen = (len + 1) / 2;
            for (uint256 i = 0; i < nextLen; ) {
                uint256 idx = i * 2;
                bytes32 left = leaves[idx];
                bytes32 right = idx + 1 < len ? leaves[idx + 1] : left;
                leaves[i] = hashPair(left, right);
                unchecked { ++i; }
            }
            len = nextLen;
        }
        return leaves[0];
    }

    function buildLeaves(address workloadManager, bytes32 workloadKey, uint8 round, address[] memory members)
        internal
        pure
        returns (bytes32[] memory leaves)
    {
        uint256 n = members.length;
        leaves = new bytes32[](n);
        for (uint256 i = 0; i < n; ) {
            leaves[i] = leaf(workloadManager, workloadKey, round, members[i]);
            unchecked { ++i; }
        }
    }

    function proofForIndex(bytes32[] memory leaves, uint256 index) internal pure returns (bytes32[] memory proof) {
        require(index < leaves.length, "bad index");

        // compute depth
        uint256 len = leaves.length;
        uint256 depth;
        while (len > 1) {
            depth++;
            len = (len + 1) / 2;
        }

        proof = new bytes32[](depth);

        bytes32[] memory level = leaves;
        uint256 idx = index;
        uint256 p;

        while (level.length > 1) {
            uint256 pair = idx ^ 1;
            bytes32 sibling = pair < level.length ? level[pair] : level[idx];
            proof[p++] = sibling;

            uint256 nextLen = (level.length + 1) / 2;
            bytes32[] memory next = new bytes32[](nextLen);
            for (uint256 i = 0; i < nextLen; ) {
                uint256 li = i * 2;
                bytes32 left = level[li];
                bytes32 right = li + 1 < level.length ? level[li + 1] : left;
                next[i] = hashPair(left, right);
                unchecked { ++i; }
            }
            level = next;
            idx = idx / 2;
        }

        return proof;
    }

    function indexOf(address[] memory arr, address target) internal pure returns (bool found, uint256 index) {
        for (uint256 i = 0; i < arr.length; ) {
            if (arr[i] == target) return (true, i);
            unchecked { ++i; }
        }
        return (false, 0);
    }
}
