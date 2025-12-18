// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";

import "../../src/mocks/MockERC20.sol";
import "../../src/ProtocolConfig.sol";
import "../../src/StakingOperators.sol";
import "../../src/WeightedCommitteeSelector.sol";
import "../../src/HeartbeatManager.sol";
import "../../src/RewardPolicy.sol";
import "../../src/JailingPolicy.sol";

import "./MerkleTestUtils.sol";

abstract contract BlacklightFixture is Test {
    using MerkleTestUtils for bytes32[];
    using MerkleTestUtils for address[];

    MockERC20 internal stakeToken;
    MockERC20 internal rewardToken;

    ProtocolConfig internal config;
    StakingOperators internal stakingOps;
    WeightedCommitteeSelector internal selector;
    HeartbeatManager internal manager;
    RewardPolicy internal rewardPolicy;
    JailingPolicy internal jailingPolicy;

    address internal admin = address(0xA11CE);
    address internal governance = address(this);

    uint256[] internal opPks;
    address[] internal ops;

    function _deploySystem(
        uint256 operatorCount,
        uint256[] memory stakes,
        uint32 baseCommitteeSize,
        uint32 maxCommitteeSize,
        uint16 quorumBps,
        uint16 verificationBps,
        uint256 responseWindow,
        uint256 jailDuration,
        uint8 maxEscalations
    ) internal {
        require(operatorCount == stakes.length, "stakes length");

        stakeToken = new MockERC20("STAKE", "STK");
        rewardToken = new MockERC20("REWARD", "RWD");

        vm.startPrank(admin);
        stakingOps = new StakingOperators(IERC20(address(stakeToken)), admin, 1 days);
        selector = new WeightedCommitteeSelector(stakingOps, admin, 0, maxCommitteeSize);
        vm.stopPrank();

        // Deploy config with placeholder modules (slashing/reward updated after deploy)
        config = new ProtocolConfig(
            governance,
            address(stakingOps),
            address(selector),
            address(0x1111),
            address(0x2222),
            baseCommitteeSize,
            0, // growth bps
            maxCommitteeSize,
            maxEscalations,
            quorumBps,
            verificationBps,
            responseWindow,
            jailDuration,
            100, // maxVoteBatchSize
            1e18 // minOperatorStake
        );

        manager = new HeartbeatManager(config, governance);
        rewardPolicy = new RewardPolicy(IERC20(address(rewardToken)), address(manager), governance, 1 days, 0);
        jailingPolicy = new JailingPolicy(address(manager));

        // wire modules
        config.setModules(address(stakingOps), address(selector), address(jailingPolicy), address(rewardPolicy));

        // wire staking ops (admin)
        vm.startPrank(admin);
        stakingOps.setProtocolConfig(config);
        stakingOps.setHeartbeatManager(address(manager));
        stakingOps.setSnapshotter(address(manager));
        stakingOps.grantRole(stakingOps.SLASHER_ROLE(), address(jailingPolicy));
        vm.stopPrank();

        // create operators
        opPks = new uint256[](operatorCount);
        ops = new address[](operatorCount);

        for (uint256 i = 0; i < operatorCount; i++) {
            uint256 pk = uint256(keccak256(abi.encodePacked("op", i + 1)));
            address op = vm.addr(pk);
            opPks[i] = pk;
            ops[i] = op;

            stakeToken.mint(op, stakes[i]);

            vm.startPrank(op);
            stakeToken.approve(address(stakingOps), type(uint256).max);
            stakingOps.stakeTo(op, stakes[i]);
            stakingOps.registerOperator(string(abi.encodePacked("ipfs://operator/", vm.toString(i))));
            vm.stopPrank();
        }

        // Ensure we can take snapshots (StakingOperators.snapshot() requires block.number > 1).
        vm.roll(block.number + 3);
        vm.warp(block.timestamp + 1);
    }

    function _defaultRawHTX(uint64 id) internal pure returns (bytes memory) {
        return abi.encodePacked("raw-htx-", id);
    }

    function _submitRawHTXAndGetRound()
        internal
        returns (bytes32 heartbeatKey, uint8 round, bytes32 root, uint64 snapshotId, address[] memory members)
    {
        vm.recordLogs();
        bytes memory rawHTX = _defaultRawHTX(1);
        heartbeatKey = manager.deriveHeartbeatKey(rawHTX);
        (snapshotId, members) = _prepareCommittee(heartbeatKey, 1, 0);
        address[] memory expectedMembers = members;
        manager.submitHeartbeat(rawHTX, snapshotId);

        Vm.Log[] memory logs = vm.getRecordedLogs();
        bytes32 sig = keccak256("RoundStarted(bytes32,uint8,bytes32,uint64,uint64,uint64,address[],bytes)");

        for (uint256 i = 0; i < logs.length; i++) {
            if (logs[i].topics.length > 0 && logs[i].topics[0] == sig) {
                bytes32 hbKey = bytes32(logs[i].topics[1]);
                if (hbKey != heartbeatKey) continue;

                bytes memory emittedRaw;
                (round, root, snapshotId, , , members, emittedRaw) =
                    abi.decode(logs[i].data, (uint8, bytes32, uint64, uint64, uint64, address[], bytes));

                assertEq(members.length, expectedMembers.length);
                for (uint256 j = 0; j < members.length; j++) {
                    assertEq(members[j], expectedMembers[j]);
                }
                assertEq(keccak256(emittedRaw), keccak256(rawHTX));
                return (heartbeatKey, round, root, snapshotId, members);
            }
        }
        fail("RoundStarted not found");
    }

    function _submitRawHTXAndGetRound(uint64 id)
        internal
        returns (bytes32 heartbeatKey, uint8 round, bytes32 root, uint64 snapshotId, address[] memory members)
    {
        vm.recordLogs();
        bytes memory rawHTX = _defaultRawHTX(id);
        heartbeatKey = manager.deriveHeartbeatKey(rawHTX);
        (snapshotId, members) = _prepareCommittee(heartbeatKey, 1, 0);
        address[] memory expectedMembers = members;
        manager.submitHeartbeat(rawHTX, snapshotId);

        Vm.Log[] memory logs = vm.getRecordedLogs();
        bytes32 sig = keccak256("RoundStarted(bytes32,uint8,bytes32,uint64,uint64,uint64,address[],bytes)");

        for (uint256 i = 0; i < logs.length; i++) {
            if (logs[i].topics.length > 0 && logs[i].topics[0] == sig) {
                bytes32 hbKey = bytes32(logs[i].topics[1]);
                if (hbKey != heartbeatKey) continue;

                bytes memory emittedRaw;
                (round, root, snapshotId, , , members, emittedRaw) =
                    abi.decode(logs[i].data, (uint8, bytes32, uint64, uint64, uint64, address[], bytes));

                assertEq(members.length, expectedMembers.length);
                for (uint256 j = 0; j < members.length; j++) {
                    assertEq(members[j], expectedMembers[j]);
                }
                assertEq(keccak256(emittedRaw), keccak256(rawHTX));
                return (heartbeatKey, round, root, snapshotId, members);
            }
        }
        fail("RoundStarted not found");
    }

    function _submitPointerAndGetRound()
        internal
        returns (bytes32 heartbeatKey, uint8 round, bytes32 root, uint64 snapshotId, address[] memory members)
    {
        return _submitRawHTXAndGetRound();
    }

    function _prepareCommittee(bytes32 heartbeatKey, uint8 round, uint8 escalationLevel)
        internal
        view
        returns (uint64 snapshotId, address[] memory members)
    {
        snapshotId = uint64(block.number - 1);
        uint32 targetSize = _computeCommitteeSize(escalationLevel);
        members = selector.selectCommittee(heartbeatKey, round, targetSize, snapshotId);
        require(members.length > 0, "empty committee");
        _sortMembers(members);
    }

    function _computeCommitteeSize(uint8 escalationLevel) internal view returns (uint32) {
        uint256 size = uint256(config.baseCommitteeSize());
        uint256 growth = uint256(config.committeeSizeGrowthBps());
        for (uint8 i = 0; i < escalationLevel; ) {
            size = (size * (10_000 + growth)) / 10_000;
            unchecked { ++i; }
        }
        uint256 cap = uint256(config.maxCommitteeSize());
        if (size > cap) size = cap;
        return uint32(size);
    }

    function _sortMembers(address[] memory arr) internal pure {
        uint256 n = arr.length;
        for (uint256 i = 1; i < n; i++) {
            address key = arr[i];
            int256 j = int256(i) - 1;
            while (j >= 0 && arr[uint256(j)] > key) {
                arr[uint256(j + 1)] = arr[uint256(j)];
                j--;
            }
            arr[uint256(j + 1)] = key;
        }
    }

    function _proofForMember(bytes32 heartbeatKey, uint8 round, address[] memory members, address member)
        internal
        view
        returns (bytes32[] memory proof)
    {
        (bool found, uint256 idx) = MerkleTestUtils.indexOf(members, member);
        require(found, "member not found");

        bytes32[] memory leaves = MerkleTestUtils.buildLeaves(address(manager), heartbeatKey, round, members);
        proof = MerkleTestUtils.proofForIndex(leaves, idx);
    }

    function _vote(bytes32 heartbeatKey, uint8 round, address[] memory members, address voter, uint8 verdict) internal {
        bytes32[] memory proof = _proofForMember(heartbeatKey, round, members, voter);
        vm.prank(voter);
        manager.submitVerdict(heartbeatKey, verdict, proof);
    }

    function _batchedVote(bytes32 heartbeatKey, uint8 round, address[] memory members, uint256 pk, address voter, uint8 verdict) internal {
        bytes32[] memory proof = _proofForMember(heartbeatKey, round, members, voter);
        bytes32 digest = manager.voteDigest(heartbeatKey, round, verdict);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(pk, digest);

        HeartbeatManager.SignedBatchedVote[] memory batch = new HeartbeatManager.SignedBatchedVote[](1);
        batch[0] = HeartbeatManager.SignedBatchedVote({
            operator: voter,
            heartbeatKey: heartbeatKey,
            round: round,
            verdict: verdict,
            memberProof: proof,
            sigV: v,
            sigR: r,
            sigS: s
        });

        manager.submitVerdictsBatched(batch);
    }
}
