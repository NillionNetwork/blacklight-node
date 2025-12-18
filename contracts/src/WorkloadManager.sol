// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/utils/Pausable.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/utils/cryptography/MerkleProof.sol";
import "@openzeppelin/contracts/utils/cryptography/EIP712.sol";
import "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";
import "@openzeppelin/contracts/utils/math/Math.sol";
import "./Interfaces.sol";

/// @title WorkloadManager
/// @notice Manages workload verification rounds with stake-weighted committees and Merkle proofs.
/// @dev Rounds start automatically on workload submission (grindable selection surface accepted for this version).
contract WorkloadManager is Pausable, ReentrancyGuard, Ownable, EIP712 {
    using ECDSA for bytes32;

    error ZeroAddress();
    error NotPending();
    error RoundClosed();
    error RoundAlreadyFinalized();
    error NotInCommittee();
    error ZeroStake();
    error BeforeDeadline();
    error AlreadyResponded();
    error InvalidVerdict();
    error CommitteeNotStarted();
    error InvalidRound();
    error EmptyCommittee();
    error InvalidSignature();
    error InvalidBatchSize();
    error RoundNotFinalized();
    error SnapshotBlockUnavailable(uint64 snapshotId);
    // Rewards distribution
    error RewardsAlreadyDone();
    error InvalidOutcome();
    error UnsortedVoters();
    error InvalidVoterInList();
    error InvalidVoterCount(uint256 got, uint256 expected);
    error InvalidVoterWeightSum(uint256 got, uint256 expected);
    error RawHTXHashMismatch();

    // Committee
    error InvalidCommitteeMember(address member);

    uint256 private constant RESPONDED_BIT = 1 << 2;
    uint256 private constant WEIGHT_SHIFT = 3;
    uint256 private constant WEIGHT_MASK_224 = (uint256(1) << 224) - 1;

    uint256 private constant MAX_VOTE_BATCH_HARD_LIMIT = 500;

    bytes32 private constant VOTE_TYPEHASH =
        keccak256("Vote(bytes32 workloadKey,uint8 round,uint8 verdict,uint64 snapshotId,bytes32 committeeRoot)");

    enum WorkloadStatus { None, Pending, Verified, Invalid, Expired }

    struct Workload {
        WorkloadStatus status;
        uint8 currentRound;
        uint8 escalationLevel;
        uint8 maxEscalationsSnapshot;
        uint64 createdAt;
        bytes32 rawHTXHash;
    }

    struct RoundInfo {
        uint256 validStake;
        uint256 invalidStake;
        uint256 errorStake;
        uint256 totalRespondedStake;
        uint256 committeeTotalStake;

        uint32 validVotesCount;

        uint32 committeeSize;
        uint64 snapshotId;
        bytes32 committeeRoot;

        uint64 startedAt;
        uint64 deadline;

        bool finalized;

        address stakingOps;
        address selector;
        address slashing;
        address reward;

        uint16 quorumBps;
        uint16 verificationBps;
        uint64 responseWindowSec;
        uint64 jailDurationSec;
    }

    struct SignedBatchedVote {
        address operator;
        bytes32 workloadKey;
        uint8 round;
        uint8 verdict;
        bytes32[] memberProof;
        uint8 sigV;
        bytes32 sigR;
        bytes32 sigS;
    }

    IProtocolConfig public config;

    mapping(bytes32 => Workload) public workloads;
    mapping(bytes32 => mapping(uint8 => RoundInfo)) public rounds;
    mapping(bytes32 => mapping(uint8 => mapping(address => uint256))) internal votePacked;

    mapping(bytes32 => mapping(uint8 => bool)) public rewardsDone;
    mapping(bytes32 => mapping(uint8 => ISlashingPolicy.Outcome)) public roundOutcome;
    mapping(bytes32 => mapping(uint8 => bool)) public slashingNotified;

    event ConfigUpdated(address config);
    event WorkloadEnqueued(bytes32 indexed workloadKey, bytes rawHTX, address indexed submitter);
    event RoundStarted(bytes32 indexed workloadKey, uint8 round, bytes32 committeeRoot, uint64 snapshotId, uint64 startedAt, uint64 deadline, address[] members, bytes rawHTX);
    event OperatorVoted(bytes32 indexed workloadKey, uint8 round, address indexed operator, uint8 verdict, uint256 weight);
    event WorkloadStatusChanged(bytes32 indexed workloadKey, WorkloadStatus oldStatus, WorkloadStatus newStatus, uint8 round);
    event RoundFinalized(bytes32 indexed workloadKey, uint8 round, ISlashingPolicy.Outcome outcome);
    event SlashingCallbackFailed(bytes32 indexed workloadKey, uint8 indexed round, bytes lowLevelData);
    event RewardDistributionAbandoned(bytes32 indexed workloadKey, uint8 indexed round);

    constructor(IProtocolConfig _config, address _owner)
        Ownable(_owner)
        EIP712("WorkloadManager", "1")
    {
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

    function _deriveWorkloadKey(bytes calldata rawHTX) internal pure returns (bytes32) {
        bytes32 rawHash = keccak256(rawHTX);
        return keccak256(abi.encodePacked("WORKLOAD_KEY_V1", rawHash));
    }

    function deriveWorkloadKey(bytes calldata rawHTX) external pure returns (bytes32) {
        return _deriveWorkloadKey(rawHTX);
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

    function _snapshotRoundConfig(bytes32 workloadKey, uint8 round) internal {
        RoundInfo storage r = rounds[workloadKey][round];
        if (r.stakingOps != address(0)) return;

        r.stakingOps = config.stakingOps();
        r.selector   = config.committeeSelector();
        r.slashing   = config.slashingPolicy();
        r.reward     = config.rewardPolicy();

        r.quorumBps         = config.quorumBps();
        r.verificationBps   = config.verificationBps();
        r.responseWindowSec = uint64(config.responseWindow());
        r.jailDurationSec   = uint64(config.jailDuration());
    }

    function submitWorkload(bytes calldata rawHTX, uint64 snapshotId)
        external
        whenNotPaused
        nonReentrant
        returns (bytes32 workloadKey)
    {
        workloadKey = _deriveWorkloadKey(rawHTX);
        if (snapshotId == 0) revert SnapshotBlockUnavailable(snapshotId);

        Workload storage w = workloads[workloadKey];
        bytes32 rawHash = keccak256(rawHTX);
        if (w.status == WorkloadStatus.None) {
            w.status = WorkloadStatus.Pending;
            w.createdAt = uint64(block.timestamp);
            w.currentRound = 1;
            w.escalationLevel = 0;
            w.maxEscalationsSnapshot = config.maxEscalations();
            w.rawHTXHash = rawHash;

            _startRound(workloadKey, 1, snapshotId, rawHTX);

            emit WorkloadStatusChanged(workloadKey, WorkloadStatus.None, WorkloadStatus.Pending, 1);
        } else {
            if (w.rawHTXHash != rawHash) revert RawHTXHashMismatch();
        }

        emit WorkloadEnqueued(workloadKey, rawHTX, msg.sender);
    }

    function _startRound(bytes32 workloadKey, uint8 round, uint64 explicitSnapshotId, bytes calldata rawHTX) internal returns (address[] memory members) {
        Workload storage w = workloads[workloadKey];
        if (w.status != WorkloadStatus.Pending) revert NotPending();

        _snapshotRoundConfig(workloadKey, round);

        RoundInfo storage r = rounds[workloadKey][round];

        IStakingOperators stakingOps = IStakingOperators(r.stakingOps);
        ICommitteeSelector selector  = ICommitteeSelector(r.selector);

        uint64 snapshotId = explicitSnapshotId;
        if (snapshotId == 0) {
            snapshotId = stakingOps.snapshot();
        } else {
            if (snapshotId == 0 || snapshotId >= block.number) revert SnapshotBlockUnavailable(snapshotId);
            if (blockhash(uint256(snapshotId)) == bytes32(0)) revert SnapshotBlockUnavailable(snapshotId);
        }
        r.snapshotId = snapshotId;

        uint32 targetSize = _computeCommitteeSize(w.escalationLevel);
        members = selector.selectCommittee(workloadKey, round, targetSize, snapshotId);
        uint256 len = members.length;
        if (len == 0) revert EmptyCommittee();

        _sortMembersInsertion(members);

        // Validate sorted list (no zero / no duplicates).
        address last = address(0);
        bytes32[] memory leaves = new bytes32[](len);
        uint256 totalStake;

        for (uint256 i = 0; i < len; ) {
            address op = members[i];
            if (op == address(0) || op <= last) revert InvalidCommitteeMember(op);
            last = op;

            uint256 stake = stakingOps.stakeAt(op, snapshotId);
            if (stake == 0) revert InvalidCommitteeMember(op); // unexpected given weighted selection; safer to fail-fast
            totalStake += stake;

            leaves[i] = keccak256(abi.encodePacked(bytes1(0xA1), address(this), workloadKey, round, op));
            unchecked { ++i; }
        }

        r.committeeRoot = _computeMerkleRoot(leaves);
        r.committeeTotalStake = totalStake;

        r.committeeSize = uint32(len);
        r.startedAt = uint64(block.timestamp);
        r.deadline = uint64(block.timestamp + uint256(r.responseWindowSec));

        emit RoundStarted(workloadKey, round, r.committeeRoot, r.snapshotId, r.startedAt, r.deadline, members, rawHTX);
    }

    function submitVerdict(bytes32 workloadKey, uint8 verdict, bytes32[] calldata memberProof)
        external
        whenNotPaused
        nonReentrant
    {
        _submitVerdict(msg.sender, workloadKey, verdict, memberProof, 0);
    }

    function submitVerdictsBatched(SignedBatchedVote[] calldata votes)
        external
        whenNotPaused
        nonReentrant
    {
        uint256 len = votes.length;
        if (len == 0) return;

        uint256 maxBatch = config.maxVoteBatchSize();
        if ((maxBatch != 0 && len > maxBatch) || len > MAX_VOTE_BATCH_HARD_LIMIT) revert InvalidBatchSize();

        for (uint256 i = 0; i < len; ) {
            SignedBatchedVote calldata v = votes[i];
            _verifyVoteSig(v);
            _submitVerdict(v.operator, v.workloadKey, v.verdict, v.memberProof, v.round);
            unchecked { ++i; }
        }
    }

    function _verifyVoteSig(SignedBatchedVote calldata v) internal view {
        RoundInfo storage r = rounds[v.workloadKey][v.round];
        if (r.committeeRoot == bytes32(0) || r.snapshotId == 0) revert CommitteeNotStarted();

        bytes32 structHash = keccak256(abi.encode(VOTE_TYPEHASH, v.workloadKey, v.round, v.verdict, r.snapshotId, r.committeeRoot));
        bytes32 digest = _hashTypedDataV4(structHash);

        address signer = ECDSA.recover(digest, v.sigV, v.sigR, v.sigS);
        if (signer == address(0) || signer != v.operator) revert InvalidSignature();
    }

    function voteDigest(bytes32 workloadKey, uint8 round, uint8 verdict) external view returns (bytes32) {
        RoundInfo storage r = rounds[workloadKey][round];
        if (r.committeeRoot == bytes32(0) || r.snapshotId == 0) revert CommitteeNotStarted();
        bytes32 structHash = keccak256(abi.encode(VOTE_TYPEHASH, workloadKey, round, verdict, r.snapshotId, r.committeeRoot));
        return _hashTypedDataV4(structHash);
    }

    function _submitVerdict(
        address operator,
        bytes32 workloadKey,
        uint8 verdict,
        bytes32[] calldata memberProof,
        uint8 explicitRound
    ) internal {
        if (verdict == 0 || verdict > 3) revert InvalidVerdict();

        Workload storage w = workloads[workloadKey];
        if (w.status != WorkloadStatus.Pending) revert NotPending();

        uint8 round = explicitRound == 0 ? w.currentRound : explicitRound;
        if (round != w.currentRound) revert InvalidRound();

        RoundInfo storage r = rounds[workloadKey][round];
        if (r.committeeRoot == bytes32(0)) revert CommitteeNotStarted();
        if (block.timestamp > r.deadline) revert RoundClosed();
        if (r.finalized) revert RoundAlreadyFinalized();

        bytes32 leaf = keccak256(abi.encodePacked(bytes1(0xA1), address(this), workloadKey, round, operator));
        if (!MerkleProof.verifyCalldata(memberProof, r.committeeRoot, leaf)) revert NotInCommittee();

        uint256 packed = votePacked[workloadKey][round][operator];
        if (_responded(packed)) revert AlreadyResponded();

        IStakingOperators stakingOps = IStakingOperators(r.stakingOps);
        uint256 weight = stakingOps.stakeAt(operator, r.snapshotId);
        if (weight == 0 || weight > WEIGHT_MASK_224) revert ZeroStake();

        uint256 newPacked = uint256(verdict) | RESPONDED_BIT | (weight << WEIGHT_SHIFT);
        votePacked[workloadKey][round][operator] = newPacked;

        r.totalRespondedStake += weight;

        if (verdict == 1) {
            r.validStake += weight;
            unchecked { r.validVotesCount += 1; }
        } else if (verdict == 2) {
            r.invalidStake += weight;
        } else {
            r.errorStake += weight;
        }

        emit OperatorVoted(workloadKey, round, operator, verdict, weight);

        _maybeFinalizeRound(workloadKey, round, w, r);
    }

    function _maybeFinalizeRound(bytes32 workloadKey, uint8 round, Workload storage w, RoundInfo storage r) internal {
        uint256 total = r.committeeTotalStake;
        if (total == 0) return;

        uint256 quorum = Math.mulDiv(r.totalRespondedStake, 10_000, total);
        if (quorum < r.quorumBps) return;

        uint256 validBps   = Math.mulDiv(r.validStake, 10_000, total);
        uint256 invalidBps = Math.mulDiv(r.invalidStake, 10_000, total);

        if (validBps >= r.verificationBps) {
            _finalizeRound(workloadKey, round, w, r, ISlashingPolicy.Outcome.ValidThreshold);
        } else if (invalidBps >= r.verificationBps) {
            _finalizeRound(workloadKey, round, w, r, ISlashingPolicy.Outcome.InvalidThreshold);
        }
    }

    function escalateOrExpire(bytes32 workloadKey, bytes calldata rawHTX) external whenNotPaused nonReentrant {
        Workload storage w = workloads[workloadKey];
        if (w.status != WorkloadStatus.Pending) revert NotPending();
        if (w.rawHTXHash == bytes32(0) || keccak256(rawHTX) != w.rawHTXHash) revert RawHTXHashMismatch();

        uint8 round = w.currentRound;
        RoundInfo storage r = rounds[workloadKey][round];
        if (r.finalized) revert RoundAlreadyFinalized();
        if (r.committeeRoot == bytes32(0)) revert CommitteeNotStarted();
        if (block.timestamp <= r.deadline) revert BeforeDeadline();

        uint256 total = r.committeeTotalStake;
        ISlashingPolicy.Outcome outcome = ISlashingPolicy.Outcome.Inconclusive;

        if (total > 0) {
            uint256 quorum = Math.mulDiv(r.totalRespondedStake, 10_000, total);
            if (quorum >= r.quorumBps) {
                uint256 validBps   = Math.mulDiv(r.validStake, 10_000, total);
                uint256 invalidBps = Math.mulDiv(r.invalidStake, 10_000, total);
                if (validBps >= r.verificationBps) outcome = ISlashingPolicy.Outcome.ValidThreshold;
                else if (invalidBps >= r.verificationBps) outcome = ISlashingPolicy.Outcome.InvalidThreshold;
            }
        }

        if (outcome == ISlashingPolicy.Outcome.ValidThreshold || outcome == ISlashingPolicy.Outcome.InvalidThreshold) {
            _finalizeRound(workloadKey, round, w, r, outcome);
            return;
        }

        if (w.escalationLevel < w.maxEscalationsSnapshot) {
            _finalizeRound(workloadKey, round, w, r, ISlashingPolicy.Outcome.Inconclusive);
            unchecked { ++w.escalationLevel; ++w.currentRound; }
            _startRound(workloadKey, w.currentRound, 0, rawHTX);
        } else {
            _finalizeRound(workloadKey, round, w, r, ISlashingPolicy.Outcome.Inconclusive);
            WorkloadStatus old = w.status;
            w.status = WorkloadStatus.Expired;
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
        roundOutcome[workloadKey][round] = outcome;

        if (outcome == ISlashingPolicy.Outcome.ValidThreshold) {
            WorkloadStatus old = w.status;
            w.status = WorkloadStatus.Verified;
            emit WorkloadStatusChanged(workloadKey, old, w.status, round);
        } else if (outcome == ISlashingPolicy.Outcome.InvalidThreshold) {
            WorkloadStatus old2 = w.status;
            w.status = WorkloadStatus.Invalid;
            emit WorkloadStatusChanged(workloadKey, old2, w.status, round);
        }

        emit RoundFinalized(workloadKey, round, outcome);

        _notifySlashing(workloadKey, round, r, outcome);
    }

    function _notifySlashing(bytes32 workloadKey, uint8 round, RoundInfo storage r, ISlashingPolicy.Outcome outcome) internal {
        if (slashingNotified[workloadKey][round]) return;
        try ISlashingPolicy(r.slashing).onRoundFinalized(workloadKey, round, outcome, r.committeeRoot, r.committeeSize) {
            slashingNotified[workloadKey][round] = true;
        } catch (bytes memory err) {
            emit SlashingCallbackFailed(workloadKey, round, err);
        }
    }

    function retrySlashing(bytes32 workloadKey, uint8 round) external whenNotPaused nonReentrant {
        RoundInfo storage r = rounds[workloadKey][round];
        if (!r.finalized) revert RoundNotFinalized();
        if (slashingNotified[workloadKey][round]) return;

        ISlashingPolicy.Outcome outcome = roundOutcome[workloadKey][round];
        _notifySlashing(workloadKey, round, r, outcome);
    }

    function distributeRewards(bytes32 workloadKey, uint8 round, address[] calldata sortedVoters)
        external
        whenNotPaused
        nonReentrant
    {
        RoundInfo storage r = rounds[workloadKey][round];
        if (!r.finalized) revert RoundNotFinalized();
        if (rewardsDone[workloadKey][round]) revert RewardsAlreadyDone();
        if (roundOutcome[workloadKey][round] != ISlashingPolicy.Outcome.ValidThreshold) revert InvalidOutcome();

        uint256 n = sortedVoters.length;
        if (n != uint256(r.validVotesCount)) revert InvalidVoterCount(n, r.validVotesCount);

        address last = address(0);
        uint256 sumWeights;
        uint256[] memory weights = new uint256[](n);

        for (uint256 i = 0; i < n; ) {
            address op = sortedVoters[i];
            if (op <= last) revert UnsortedVoters();
            last = op;

            uint256 packed = votePacked[workloadKey][round][op];
            if ((packed & RESPONDED_BIT) == 0) revert InvalidVoterInList();
            if (uint8(packed & 0x3) != 1) revert InvalidVoterInList();

            uint256 wgt = _weight(packed);
            weights[i] = wgt;
            sumWeights += wgt;

            unchecked { ++i; }
        }

        if (sumWeights != r.validStake) revert InvalidVoterWeightSum(sumWeights, r.validStake);

        IRewardPolicy(r.reward).accrueWeights(workloadKey, round, sortedVoters, weights);
        rewardsDone[workloadKey][round] = true;
    }

    function abandonRewardDistribution(bytes32 workloadKey, uint8 round) external onlyOwner {
        rewardsDone[workloadKey][round] = true;
        emit RewardDistributionAbandoned(workloadKey, round);
    }

    // --- Merkle helpers (commutative pair hashing, compatible with OZ MerkleProof) ---

    function _hashPair(bytes32 a, bytes32 b) internal pure returns (bytes32) {
        return a < b ? keccak256(abi.encodePacked(a, b)) : keccak256(abi.encodePacked(b, a));
    }

    function _computeMerkleRoot(bytes32[] memory leaves) internal pure returns (bytes32) {
        uint256 len = leaves.length;
        if (len == 0) return bytes32(0);

        while (len > 1) {
            uint256 nextLen = (len + 1) / 2;
            for (uint256 i = 0; i < nextLen; ) {
                uint256 idx = i * 2;
                bytes32 left = leaves[idx];
                bytes32 right = idx + 1 < len ? leaves[idx + 1] : left;
                leaves[i] = _hashPair(left, right);
                unchecked { ++i; }
            }
            len = nextLen;
        }
        return leaves[0];
    }

    // Insertion sort (committee sizes expected to be relatively small)
    function _sortMembersInsertion(address[] memory arr) internal pure {
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

    function _responded(uint256 p) internal pure returns (bool) { return (p & RESPONDED_BIT) != 0; }
    function _weight(uint256 p) internal pure returns (uint256) { return (p >> WEIGHT_SHIFT) & WEIGHT_MASK_224; }

    // --- Views for policies/off-chain ---

    function getVotePacked(bytes32 workloadKey, uint8 round, address operator) external view returns (uint256) {
        return votePacked[workloadKey][round][operator];
    }

    function getRoundForPolicy(bytes32 workloadKey, uint8 round)
        external
        view
        returns (bool, ISlashingPolicy.Outcome, bytes32, address, uint64)
    {
        RoundInfo storage r = rounds[workloadKey][round];
        return (r.finalized, roundOutcome[workloadKey][round], r.committeeRoot, r.stakingOps, r.jailDurationSec);
    }

    function nodeCount() external view returns (uint256) {
        address[] memory active = IStakingOperators(config.stakingOps()).getActiveOperators();
        return active.length;
    }

    function getNodes() external view returns (address[] memory) {
        return IStakingOperators(config.stakingOps()).getActiveOperators();
    }
}
