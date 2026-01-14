// SPDX-License-Identifier: MIT
pragma solidity ^0.8.22;

import "forge-std/Test.sol";
import "forge-std/StdInvariant.sol";

import "../../src/mocks/MockERC20.sol";
import "../../src/ProtocolConfig.sol";
import "../../src/StakingOperators.sol";
import "../../src/WeightedCommitteeSelector.sol";
import "../../src/HeartbeatManager.sol";
import "../../src/RewardPolicy.sol";
import "../../src/JailingPolicy.sol";

import "../helpers/MerkleTestUtils.sol";

contract HeartbeatHandler is Test {
    using MerkleTestUtils for bytes32[];
    using MerkleTestUtils for address[];

    HeartbeatManager public manager;
    StakingOperators public stakingOps;

    bytes32 public heartbeatKey;
    uint8 public round;
    uint64 public snapshotId;
    address[] public members;
    bytes public rawHTX;

    constructor(HeartbeatManager _manager, StakingOperators _stakingOps, bytes32 _heartbeatKey, uint8 _round, uint64 _snapshotId, address[] memory _members, bytes memory _rawHTX) {
        manager = _manager;
        stakingOps = _stakingOps;
        heartbeatKey = _heartbeatKey;
        round = _round;
        snapshotId = _snapshotId;
        members = _members;
        rawHTX = _rawHTX;
    }


    function getMembers() external view returns (address[] memory) {
        return members;
    }

    function warp(uint256 secs) external {
        uint256 dt = bound(secs, 0, 3 days);
        vm.warp(block.timestamp + dt);
        vm.roll(block.number + 1);
    }

    function vote(uint256 memberIndex, uint8 verdict) external {
        uint256 idx = memberIndex % members.length;
        address voter = members[idx];

        // read round info
        (, , , , , , , , , , uint64 deadline, bool finalized, , , , , , , , ) = manager.rounds(heartbeatKey, round);
        if (finalized) return;
        if (block.timestamp > deadline) return;
        if (verdict == 0 || verdict > 3) verdict = 1;

        uint256 packed = manager.getVotePacked(heartbeatKey, round, voter);
        if ((packed & (1 << 2)) != 0) return;

        bytes32[] memory proof = _proof(voter);
        vm.prank(voter);
        manager.submitVerdict(heartbeatKey, verdict, proof);
    }

    function escalateIfNeeded() external {
        (, , , , , , , , , , uint64 deadline, bool finalized, , , , , , , , ) = manager.rounds(heartbeatKey, round);
        if (finalized) return;
        if (block.timestamp <= deadline) return;

        // may expire the heartbeat (maxEscalations=0 in this invariant setup)
        manager.escalateOrExpire(heartbeatKey, rawHTX);
    }

    function _proof(address member) internal view returns (bytes32[] memory proof) {
        (bool found, uint256 idx) = MerkleTestUtils.indexOf(members, member);
        require(found, "member not found");
        bytes32[] memory leaves = MerkleTestUtils.buildLeaves(address(manager), heartbeatKey, round, members);
        proof = MerkleTestUtils.proofForIndex(leaves, idx);
    }
}

contract HeartbeatManagerInvariants is StdInvariant, Test {
    using MerkleTestUtils for bytes32[];
    using MerkleTestUtils for address[];

    MockERC20 stakeToken;
    MockERC20 rewardToken;

    ProtocolConfig config;
    StakingOperators stakingOps;
    WeightedCommitteeSelector selector;
    HeartbeatManager manager;
    RewardPolicy rewardPolicy;
    JailingPolicy jailingPolicy;

    HeartbeatHandler handler;

    address admin = address(0xA11CE);

    function setUp() public {
        stakeToken = new MockERC20("STAKE", "STK");
        rewardToken = new MockERC20("REWARD", "RWD");

        vm.startPrank(admin);
        stakingOps = new StakingOperators(IERC20(address(stakeToken)), admin, 1 days);
        selector = new WeightedCommitteeSelector(stakingOps, admin, 1, 50);
        vm.stopPrank();

        config = new ProtocolConfig(
            address(this),
            address(stakingOps),
            address(selector),
            address(stakingOps),
            address(selector),
            20,   // baseCommitteeSize
            0,
            20,
            0,    // maxEscalations (no extra rounds)
            5000, // quorumBps
            5000, // verificationBps
            1 days,
            7 days,
            100,
            1e18,
            1e18,
            1000
        );

        manager = new HeartbeatManager(config, address(this));
        rewardPolicy = new RewardPolicy(IERC20(address(rewardToken)), address(manager), address(this), 1 days, 0);
        jailingPolicy = new JailingPolicy(address(manager));
        config.setModules(address(stakingOps), address(selector), address(jailingPolicy), address(rewardPolicy));

        vm.startPrank(admin);
        stakingOps.setProtocolConfig(config);
        stakingOps.setSnapshotter(address(manager));
        stakingOps.setHeartbeatManager(address(manager));
        stakingOps.grantRole(stakingOps.SLASHER_ROLE(), address(jailingPolicy));
        vm.stopPrank();

        // create 30 operators with equal stake and register
        uint256 n = 30;
        for (uint256 i = 0; i < n; i++) {
            uint256 pk = uint256(keccak256(abi.encodePacked("op", i + 1)));
            address op = vm.addr(pk);

            stakeToken.mint(op, 3e18);
            vm.startPrank(op);
            stakeToken.approve(address(stakingOps), type(uint256).max);
            stakeToken.approve(address(manager), type(uint256).max);
            stakingOps.stakeTo(op, 2e18);
            stakingOps.registerOperator("ipfs://x");
            vm.stopPrank();
        }

        // advance blocks so snapshot works
        vm.roll(block.number + 3);
        vm.warp(block.timestamp + 1);

        // start heartbeat and capture committee list from logs
        bytes memory rawHTX = abi.encodePacked("raw-htx-invariant");
        uint64 submissionBlock = uint64(block.number);
        bytes32 hbKey = manager.deriveHeartbeatKey(rawHTX, submissionBlock);

        vm.recordLogs();
        uint32 targetSize = config.baseCommitteeSize();
        uint64 snap = uint64(block.number - 1);
        address[] memory membersOffchain = selector.selectCommittee(hbKey, 1, targetSize, snap);
        require(membersOffchain.length == targetSize, "empty committee");
        _sortMembers(membersOffchain);
        vm.prank(vm.addr(uint256(keccak256(abi.encodePacked("op", uint256(1))))));
        manager.submitHeartbeat(rawHTX, snap);
        Vm.Log[] memory logs = vm.getRecordedLogs();

        bytes32 sig = keccak256("RoundStarted(bytes32,uint8,bytes32,uint64,uint64,uint64,address[],bytes)");

        address[] memory members;
        uint8 round;
        uint64 snapshotId;
        for (uint256 i = 0; i < logs.length; i++) {
            if (logs[i].topics.length > 0 && logs[i].topics[0] == sig && bytes32(logs[i].topics[1]) == hbKey) {
                bytes memory emittedRaw;
                (round, , snapshotId, , , members, emittedRaw) =
                    abi.decode(logs[i].data, (uint8, bytes32, uint64, uint64, uint64, address[], bytes));
                assertEq(keccak256(emittedRaw), keccak256(rawHTX));
                require(members.length == membersOffchain.length, "missing members");
                for (uint256 j = 0; j < members.length; j++) {
                    require(members[j] == membersOffchain[j], "member mismatch");
                }
                break;
            }
        }
        require(members.length == 20, "missing members");

        handler = new HeartbeatHandler(manager, stakingOps, hbKey, round, snapshotId, members, rawHTX);
        targetContract(address(handler));
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

    function invariant_roundAccountingSums() public {
        bytes32 hbKey = handler.heartbeatKey();
        uint8 r = handler.round();

        (uint256 validStake, uint256 invalidStake, uint256 errorStake, uint256 totalResponded, uint256 committeeTotal,
            uint32 validVotesCount, uint32 committeeSize, uint64 snapshotId, , , , , , , , , , , , ) = manager.rounds(hbKey, r);

        assertEq(validStake + invalidStake + errorStake, totalResponded, "stake buckets don't sum to responded");
        assertLe(totalResponded, committeeTotal, "responded exceeds committee total");
        assertLe(validStake, committeeTotal);
        assertLe(invalidStake, committeeTotal);
        assertLe(errorStake, committeeTotal);

        // committee size and snapshot id are stable
        assertEq(committeeSize, 20);
        assertEq(snapshotId, handler.snapshotId());

        // validVotesCount equals count of verdict==1 responders in committee
        address[] memory members = handler.getMembers();
        uint32 count;
        for (uint256 i = 0; i < members.length; i++) {
            uint256 packed = manager.getVotePacked(hbKey, r, members[i]);
            if ((packed & (1 << 2)) != 0 && uint8(packed & 0x3) == 1) count++;
        }
        assertEq(count, validVotesCount, "validVotesCount mismatch");
    }

    function invariant_voteWeightsMatchSnapshotStake() public {
        bytes32 hbKey = handler.heartbeatKey();
        uint8 r = handler.round();
        uint64 snap = handler.snapshotId();

        address[] memory members = handler.getMembers();

        for (uint256 i = 0; i < members.length; i++) {
            address op = members[i];
            uint256 packed = manager.getVotePacked(hbKey, r, op);

            bool responded = (packed & (1 << 2)) != 0;
            if (!responded) {
                assertEq(packed, 0, "non-responded packed should be zero");
            } else {
                uint256 weight = (packed >> 3) & ((uint256(1) << 224) - 1);
                assertGt(weight, 0, "responded weight zero");
                assertEq(weight, stakingOps.stakeAt(op, snap), "weight != snapshot stake");
            }
        }
    }

    function invariant_committeeTotalStakeMatchesSumStakeAtSnapshot() public {
        bytes32 hbKey = handler.heartbeatKey();
        uint8 r = handler.round();
        uint64 snap = handler.snapshotId();

        ( , , , , uint256 committeeTotal, , , , , , , , , , , , , , , ) = manager.rounds(hbKey, r);

        address[] memory members = handler.getMembers();
        uint256 sum;
        for (uint256 i = 0; i < members.length; i++) {
            sum += stakingOps.stakeAt(members[i], snap);
        }

        assertEq(sum, committeeTotal, "committeeTotalStake mismatch");
    }
}
