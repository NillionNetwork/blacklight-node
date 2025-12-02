// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/utils/Pausable.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/access/Ownable.sol";

import "./Interfaces.sol";

contract WorkloadManager is Pausable, ReentrancyGuard, Ownable {
    error ZeroAddress();
    error NotPending();
    error EmptyVerdict();
    error RoundClosed();
    error RoundAlreadyFinalized();
    error NotActiveOperator();
    error NotInCommittee();
    error ZeroStake();
    error BeforeDeadline();
    error AlreadyResponded();

    enum WorkloadStatus {
        None,
        Pending,
        Verified,
        Invalid,
        Expired
    }

    enum Verdict {
        None,
        Valid,
        Invalid,
        Error
    }

    struct WorkloadPointer {
        uint64  currentId;
        uint64  previousId;
        bytes32 contentHash;
        uint256 blobIndex;
    }

    struct Workload {
        WorkloadStatus status;
        uint8          currentRound;
        uint8          escalationLevel;
        uint64         createdAt;
        uint64         finalizedAt;
    }

    struct RoundInfo {
        uint128 validStake;
        uint128 invalidStake;
        uint128 errorStake;
        uint256 totalRespondedStake;
        uint32  committeeSize;
        uint64  startedAt;
        uint64  deadline;
        bool    finalized;
    }

    IProtocolConfig public config;

    mapping(bytes32 => Workload) public workloads;
    mapping(bytes32 => mapping(uint8 => RoundInfo)) public rounds;
    mapping(bytes32 => mapping(uint8 => mapping(address => Verdict))) public verdicts;
    mapping(bytes32 => mapping(uint8 => mapping(address => bool))) public hasResponded;
    mapping(bytes32 => mapping(uint8 => address[])) private _committees;
    mapping(bytes32 => mapping(uint8 => mapping(address => bool))) private _isInCommittee;

    mapping(address => uint64) public assignments;
    mapping(address => uint64) public responses;

    event ConfigUpdated(address config);
    event WorkloadEnqueued(
        bytes32 indexed workloadKey,
        uint64  currentId,
        uint64  previousId,
        bytes32 contentHash,
        uint256 blobIndex,
        address indexed submitter
    );
    event WorkloadRoundStarted(
        bytes32 indexed workloadKey,
        uint8   round,
        uint32  committeeSize,
        uint64  startedAt,
        uint64  deadline
    );
    event VerdictSubmitted(
        bytes32 indexed workloadKey,
        uint8   round,
        address indexed operator,
        Verdict verdict,
        uint256 stake
    );
    event WorkloadStatusChanged(
        bytes32 indexed workloadKey,
        WorkloadStatus oldStatus,
        WorkloadStatus newStatus,
        uint8          round
    );
    event RoundFinalized(
        bytes32 indexed workloadKey,
        uint8   round,
        ISlashingPolicy.Outcome outcome
    );

    constructor(IProtocolConfig _config, address _owner) Ownable(_owner) {
        if (address(_config) == address(0)) revert ZeroAddress();
        config = _config;
        emit ConfigUpdated(address(_config));
    }

    function setConfig(IProtocolConfig newConfig) external onlyOwner {
        if (address(newConfig) == address(0)) revert ZeroAddress();
        config = newConfig;
        emit ConfigUpdated(address(newConfig));
    }

    function pause() external onlyOwner { _pause(); }
    function unpause() external onlyOwner { _unpause(); }

    function _deriveWorkloadKey(WorkloadPointer calldata pointer) internal pure returns (bytes32) {
        return keccak256(
            abi.encode(
                pointer.currentId,
                pointer.previousId,
                pointer.contentHash,
                pointer.blobIndex
            )
        );
    }

    function deriveWorkloadKey(WorkloadPointer calldata pointer) external pure returns (bytes32) {
        return _deriveWorkloadKey(pointer);
    }

    function _computeCommitteeSize(uint8 escalationLevel) internal view returns (uint32) {
        uint32 size = config.baseCommitteeSize();
        uint32 growth = config.committeeSizeGrowthBps();
        for (uint8 i = 0; i < escalationLevel; ) {
            size = uint32(uint256(size) * (10_000 + growth) / 10_000);
            unchecked { ++i; }
        }
        return size;
    }

    function _getModules()
        internal
        view
        returns (
            IStakingOperators stakingOps,
            ICommitteeSelector selector,
            ISlashingPolicy slashing,
            IRewardPolicy reward
        )
    {
        stakingOps = IStakingOperators(config.stakingOps());
        selector   = ICommitteeSelector(config.committeeSelector());
        slashing   = ISlashingPolicy(config.slashingPolicy());
        reward     = IRewardPolicy(config.rewardPolicy());
    }

    function _getCommittee(bytes32 workloadKey, uint8 round)
        internal
        view
        returns (address[] storage)
    {
        return _committees[workloadKey][round];
    }

    function getCommittee(bytes32 workloadKey, uint8 round)
        external
        view
        returns (address[] memory)
    {
        return _committees[workloadKey][round];
    }

    // Workload lifecycle

    function submitWorkload(WorkloadPointer calldata pointer)
        external
        whenNotPaused
        nonReentrant
        returns (bytes32 workloadKey)
    {
        workloadKey = _deriveWorkloadKey(pointer);

        Workload storage w = workloads[workloadKey];

        if (w.status == WorkloadStatus.None) {
            w.status = WorkloadStatus.Pending;
            w.createdAt = uint64(block.timestamp);
            w.currentRound = 1;
            w.escalationLevel = 0;

            _startRound(workloadKey, 1);
            emit WorkloadStatusChanged(workloadKey, WorkloadStatus.None, WorkloadStatus.Pending, 1);
        }

        emit WorkloadEnqueued(
            workloadKey,
            pointer.currentId,
            pointer.previousId,
            pointer.contentHash,
            pointer.blobIndex,
            msg.sender
        );
    }

    function _startRound(bytes32 workloadKey, uint8 round) internal {
        Workload storage w = workloads[workloadKey];
        if (w.status != WorkloadStatus.Pending) revert NotPending();

        (
            ,
            ICommitteeSelector selector,
            ,
        ) = _getModules();

        uint32 targetSize = _computeCommitteeSize(w.escalationLevel);
        address[] memory members = selector.selectCommittee(workloadKey, round, targetSize);
        uint256 len = members.length;
        if (len == 0) revert NotPending();

        RoundInfo storage r = rounds[workloadKey][round];
        r.committeeSize = uint32(len);
        r.startedAt = uint64(block.timestamp);
        r.deadline = uint64(block.timestamp + config.responseWindow());

        address[] storage stored = _committees[workloadKey][round];
        for (uint256 i = 0; i < len; ) {
            address op = members[i];
            stored.push(op);
            _isInCommittee[workloadKey][round][op] = true;
            assignments[op] += 1;
            unchecked { ++i; }
        }

        emit WorkloadRoundStarted(
            workloadKey,
            round,
            r.committeeSize,
            r.startedAt,
            r.deadline
        );
    }

    function submitVerdict(bytes32 workloadKey, Verdict verdict)
        external
        whenNotPaused
        nonReentrant
    {
        if (verdict == Verdict.None) revert EmptyVerdict();

        Workload storage w = workloads[workloadKey];
        if (w.status != WorkloadStatus.Pending) revert NotPending();

        uint8 round = w.currentRound;
        RoundInfo storage r = rounds[workloadKey][round];
        if (block.timestamp > r.deadline) revert RoundClosed();
        if (r.finalized) revert RoundAlreadyFinalized();

        (
            IStakingOperators stakingOps,
            ,
            ,
        ) = _getModules();

        if (!stakingOps.isActiveOperator(msg.sender)) revert NotActiveOperator();
        if (!_isInCommittee[workloadKey][round][msg.sender]) revert NotInCommittee();
        if (hasResponded[workloadKey][round][msg.sender]) revert AlreadyResponded();

        uint256 stake = stakingOps.stakeOf(msg.sender);
        if (stake == 0) revert ZeroStake();

        verdicts[workloadKey][round][msg.sender] = verdict;
        hasResponded[workloadKey][round][msg.sender] = true;

        responses[msg.sender] += 1;
        r.totalRespondedStake += stake;

        if (verdict == Verdict.Valid) {
            r.validStake += uint128(stake);
        } else if (verdict == Verdict.Invalid) {
            r.invalidStake += uint128(stake);
        } else if (verdict == Verdict.Error) {
            r.errorStake += uint128(stake);
        }

        emit VerdictSubmitted(workloadKey, round, msg.sender, verdict, stake);

        _maybeFinalizeRound(workloadKey, round, w, r);
    }

    function _maybeFinalizeRound(
        bytes32 workloadKey,
        uint8 round,
        Workload storage w,
        RoundInfo storage r
    ) internal {
        uint256 total = r.totalRespondedStake;
        if (total == 0) return;
        if (block.timestamp > r.deadline) return;

        uint16 threshold = config.verificationBps();
        uint256 validBps = (uint256(r.validStake) * 10_000) / total;
        uint256 invalidBps = (uint256(r.invalidStake) * 10_000) / total;

        if (validBps >= threshold) {
            _finalizeRound(workloadKey, round, w, r, ISlashingPolicy.Outcome.ValidThreshold);
        } else if (invalidBps >= threshold) {
            _finalizeRound(workloadKey, round, w, r, ISlashingPolicy.Outcome.InvalidThreshold);
        }
    }

    function escalateOrExpire(bytes32 workloadKey)
        external
        whenNotPaused
        nonReentrant
    {
        Workload storage w = workloads[workloadKey];
        if (w.status != WorkloadStatus.Pending) revert NotPending();

        uint8 round = w.currentRound;
        RoundInfo storage r = rounds[workloadKey][round];
        if (r.finalized) revert RoundAlreadyFinalized();
        if (block.timestamp <= r.deadline) revert BeforeDeadline();

        uint256 total = r.totalRespondedStake;
        ISlashingPolicy.Outcome outcome;

        if (total > 0) {
            uint16 threshold = config.verificationBps();
            uint256 validBps = (uint256(r.validStake) * 10_000) / total;
            uint256 invalidBps = (uint256(r.invalidStake) * 10_000) / total;

            if (validBps >= threshold) {
                outcome = ISlashingPolicy.Outcome.ValidThreshold;
            } else if (invalidBps >= threshold) {
                outcome = ISlashingPolicy.Outcome.InvalidThreshold;
            } else {
                outcome = ISlashingPolicy.Outcome.Inconclusive;
            }
        } else {
            outcome = ISlashingPolicy.Outcome.Inconclusive;
        }

        if (outcome == ISlashingPolicy.Outcome.ValidThreshold ||
            outcome == ISlashingPolicy.Outcome.InvalidThreshold)
        {
            _finalizeRound(workloadKey, round, w, r, outcome);
            return;
        }

        if (w.escalationLevel < config.maxEscalations()) {
            _finalizeRound(workloadKey, round, w, r, ISlashingPolicy.Outcome.Inconclusive);
            unchecked { ++w.escalationLevel; ++w.currentRound; }
            _startRound(workloadKey, w.currentRound);
        } else {
            _finalizeRound(workloadKey, round, w, r, ISlashingPolicy.Outcome.Inconclusive);
            WorkloadStatus old = w.status;
            w.status = WorkloadStatus.Expired;
            w.finalizedAt = uint64(block.timestamp);
            emit WorkloadStatusChanged(workloadKey, old, w.status, round);
        }
    }

    function _finalizeRound(
        bytes32 workloadKey,
        uint8 round,
        Workload storage w,
        RoundInfo storage r,
        ISlashingPolicy.Outcome outcome
    ) internal {
        if (r.finalized) revert RoundAlreadyFinalized();
        r.finalized = true;

        (
            ,
            ,
            ISlashingPolicy slashing,
            IRewardPolicy reward
        ) = _getModules();

        address[] storage members = _getCommittee(workloadKey, round);

        slashing.onRoundFinalized(workloadKey, round, outcome, members);

        if (outcome == ISlashingPolicy.Outcome.ValidThreshold) {
            reward.onWorkloadValidated(workloadKey, round, members);
            WorkloadStatus old = w.status;
            w.status = WorkloadStatus.Verified;
            w.finalizedAt = uint64(block.timestamp);
            emit WorkloadStatusChanged(workloadKey, old, w.status, round);
        } else if (outcome == ISlashingPolicy.Outcome.InvalidThreshold) {
            WorkloadStatus old = w.status;
            w.status = WorkloadStatus.Invalid;
            w.finalizedAt = uint64(block.timestamp);
            emit WorkloadStatusChanged(workloadKey, old, w.status, round);
        }

        emit RoundFinalized(workloadKey, round, outcome);
    }
}