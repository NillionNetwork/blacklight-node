// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "forge-std/Test.sol";

import "../../src/mocks/MockERC20.sol";
import "../../src/ProtocolConfig.sol";
import "../../src/StakingOperators.sol";
import "../../src/WeightedCommitteeSelector.sol";
import "../../src/WorkloadManager.sol";
import "../../src/RewardPolicy.sol";
import "../../src/JailingPolicy.sol";

import "./MerkleTestUtils.sol";

abstract contract RCFixture is Test {
    using MerkleTestUtils for bytes32[];
    using MerkleTestUtils for address[];

    MockERC20 internal stakeToken;
    MockERC20 internal rewardToken;

    ProtocolConfig internal config;
    StakingOperators internal stakingOps;
    WeightedCommitteeSelector internal selector;
    WorkloadManager internal manager;
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

        manager = new WorkloadManager(config, governance);
        rewardPolicy = new RewardPolicy(IERC20(address(rewardToken)), address(manager), governance, 1 days, 0);
        jailingPolicy = new JailingPolicy(address(manager));

        // wire modules
        config.setModules(address(stakingOps), address(selector), address(jailingPolicy), address(rewardPolicy));

        // wire staking ops (admin)
        vm.startPrank(admin);
        stakingOps.setProtocolConfig(config);
        stakingOps.setWorkloadManager(address(manager));
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

    function _defaultPointer(uint64 id) internal pure returns (WorkloadManager.WorkloadPointer memory p) {
        p.currentId = id;
        p.previousId = id == 0 ? 0 : id - 1;
        p.contentHash = keccak256(abi.encodePacked("content", id));
        p.blobIndex = uint256(id);
    }

    function _submitPointerAndGetRound()
        internal
        returns (bytes32 workloadKey, uint8 round, bytes32 root, uint32 snapshotId, address[] memory members)
    {
        vm.recordLogs();
        WorkloadManager.WorkloadPointer memory p = _defaultPointer(1);
        workloadKey = manager.deriveWorkloadKey(p);
        (snapshotId, members) = _prepareCommittee(workloadKey, 1, 0);
        manager.submitWorkload(p, snapshotId, members);

        Vm.Log[] memory logs = vm.getRecordedLogs();
        bytes32 sig = keccak256("RoundStarted(bytes32,uint8,bytes32,uint32,uint32,uint64,uint64,address[])");

        for (uint256 i = 0; i < logs.length; i++) {
            if (logs[i].topics.length > 0 && logs[i].topics[0] == sig) {
                bytes32 wk = bytes32(logs[i].topics[1]);
                if (wk != workloadKey) continue;

                (round, root, , snapshotId, , , members) =
                    abi.decode(logs[i].data, (uint8, bytes32, uint32, uint32, uint64, uint64, address[]));
                return (workloadKey, round, root, snapshotId, members);
            }
        }
        fail("RoundStarted not found");
    }

    function _prepareCommittee(bytes32 workloadKey, uint8 round, uint8 escalationLevel)
        internal
        view
        returns (uint32 snapshotId, address[] memory members)
    {
        snapshotId = uint32(block.number - 1);
        uint32 targetSize = _computeCommitteeSize(escalationLevel);
        members = selector.selectCommittee(workloadKey, round, targetSize, snapshotId);
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

    function _proofForMember(bytes32 workloadKey, uint8 round, address[] memory members, address member)
        internal
        view
        returns (bytes32[] memory proof)
    {
        (bool found, uint256 idx) = MerkleTestUtils.indexOf(members, member);
        require(found, "member not found");

        bytes32[] memory leaves = MerkleTestUtils.buildLeaves(address(manager), workloadKey, round, members);
        proof = MerkleTestUtils.proofForIndex(leaves, idx);
    }

    function _vote(bytes32 workloadKey, uint8 round, address[] memory members, address voter, uint8 verdict) internal {
        bytes32[] memory proof = _proofForMember(workloadKey, round, members, voter);
        vm.prank(voter);
        manager.submitVerdict(workloadKey, verdict, proof);
    }

    function _batchedVote(bytes32 workloadKey, uint8 round, address[] memory members, uint256 pk, address voter, uint8 verdict) internal {
        bytes32[] memory proof = _proofForMember(workloadKey, round, members, voter);
        bytes32 digest = manager.voteDigest(workloadKey, round, verdict);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(pk, digest);

        WorkloadManager.SignedBatchedVote[] memory batch = new WorkloadManager.SignedBatchedVote[](1);
        batch[0] = WorkloadManager.SignedBatchedVote({
            operator: voter,
            workloadKey: workloadKey,
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
