// SPDX-License-Identifier: MIT
pragma solidity ^0.8.22;

import "@openzeppelin/contracts/utils/Pausable.sol";
import "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import "@openzeppelin/contracts/access/Ownable.sol";
import "@openzeppelin/contracts/utils/cryptography/MerkleProof.sol";
import "@openzeppelin/contracts/utils/cryptography/EIP712.sol";
import "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";
import "@openzeppelin/contracts/utils/math/Math.sol";
import "@openzeppelin/contracts/utils/math/SafeCast.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import "./Interfaces.sol";

/// @title HeartbeatManager
/// @notice Manages heartbeat verification rounds with stake-weighted committees and Merkle proofs.
/// @dev Rounds start automatically on heartbeat submission (grindable selection surface accepted for this version).
contract HeartbeatManager is Pausable, ReentrancyGuard, Ownable, EIP712 {
    using ECDSA for bytes32;
    using SafeERC20 for IERC20;

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
    error ZeroHeartbeatBond();

    // Committee
    error InvalidCommitteeMember(address member);
    error InvalidSlashingGasLimit();
    error InvalidProtocolConfig(address candidate);

    uint256 private constant RESPONDED_BIT = 1 << 2;
    uint256 private constant WEIGHT_SHIFT = 3;
    uint256 private constant WEIGHT_MASK_224 = (uint256(1) << 224) - 1;

    uint256 private constant BPS_DENOMINATOR = 10_000;
    uint256 private constant MAX_VOTE_BATCH_HARD_LIMIT = 500;
    uint256 private constant DEFAULT_SLASHING_GAS_LIMIT = 200_000;
    address private constant BURN_ADDRESS = address(0xdead);

    bytes32 private constant VOTE_TYPEHASH =
        keccak256("Vote(bytes32 heartbeatKey,uint8 round,uint8 verdict,uint64 snapshotId,bytes32 committeeRoot)");

    enum HeartbeatStatus { None, Pending, Verified, Invalid, Expired }

    struct Heartbeat {
        HeartbeatStatus status;
        uint8 currentRound;
        uint8 escalationLevel;
        uint8 maxEscalationsSnapshot;
        uint64 createdAt;
        bytes32 rawHTXHash;
        address submitter;
        address bondToken;
        uint16 bondBurnBps;
        uint256 bondAmount;
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
        bytes32 heartbeatKey;
        uint8 round;
        uint8 verdict;
        bytes32[] memberProof;
        uint8 sigV;
        bytes32 sigR;
        bytes32 sigS;
    }

    IProtocolConfig public config;
    uint256 public slashingGasLimit;

    mapping(bytes32 => Heartbeat) public heartbeats;
    mapping(bytes32 => mapping(uint8 => RoundInfo)) public rounds;
    mapping(bytes32 => mapping(uint8 => mapping(address => uint256))) internal votePacked;

    mapping(bytes32 => mapping(uint8 => bool)) public rewardsDone;
    mapping(bytes32 => mapping(uint8 => ISlashingPolicy.Outcome)) public roundOutcome;
    mapping(bytes32 => mapping(uint8 => bool)) public slashingNotified;

    event ConfigUpdated(address config);
    event HeartbeatEnqueued(bytes32 indexed heartbeatKey, bytes rawHTX, address indexed submitter);
    event RoundStarted(bytes32 indexed heartbeatKey, uint8 round, bytes32 committeeRoot, uint64 snapshotId, uint64 startedAt, uint64 deadline, address[] members, bytes rawHTX);
    event OperatorVoted(bytes32 indexed heartbeatKey, uint8 round, address indexed operator, uint8 verdict, uint256 weight);
    event HeartbeatStatusChanged(bytes32 indexed heartbeatKey, HeartbeatStatus oldStatus, HeartbeatStatus newStatus, uint8 round);
    event RoundFinalized(bytes32 indexed heartbeatKey, uint8 round, ISlashingPolicy.Outcome outcome);
    event SlashingCallbackFailed(bytes32 indexed heartbeatKey, uint8 indexed round, bytes lowLevelData);
    event RewardDistributionAbandoned(bytes32 indexed heartbeatKey, uint8 indexed round);
    event RewardsDistributed(bytes32 indexed heartbeatKey, uint8 indexed round, uint256 voterCount, uint256 totalWeight);
    event SlashingGasLimitUpdated(uint256 oldLimit, uint256 newLimit);
    event HeartbeatBonded(bytes32 indexed heartbeatKey, address indexed submitter, uint256 amount);
    event HeartbeatBondRefunded(bytes32 indexed heartbeatKey, address indexed submitter, uint256 amount);
    event HeartbeatBondBurned(bytes32 indexed heartbeatKey, uint256 amount);

    constructor(IProtocolConfig _config, address _owner)
        Ownable(_owner)
        EIP712("HeartbeatManager", "1")
    {
        if (address(_config) == address(0)) revert ZeroAddress();
        if (!_isProtocolConfig(address(_config))) revert InvalidProtocolConfig(address(_config));
        config = _config;
        slashingGasLimit = DEFAULT_SLASHING_GAS_LIMIT;
        emit ConfigUpdated(address(_config));
    }

    function setConfig(IProtocolConfig newConfig) external onlyOwner {
        if (address(newConfig) == address(0)) revert ZeroAddress();
        if (!_isProtocolConfig(address(newConfig))) revert InvalidProtocolConfig(address(newConfig));
        config = newConfig;
        emit ConfigUpdated(address(newConfig));
    }

    function setSlashingGasLimit(uint256 newLimit) external onlyOwner {
        if (newLimit == 0) revert InvalidSlashingGasLimit();
        emit SlashingGasLimitUpdated(slashingGasLimit, newLimit);
        slashingGasLimit = newLimit;
    }

    function pause() external onlyOwner { _pause(); }
    function unpause() external onlyOwner { _unpause(); }

    function _deriveHeartbeatKey(bytes calldata rawHTX, uint64 blockNumber) internal pure returns (bytes32) {
        bytes32 rawHash = keccak256(rawHTX);
        return keccak256(abi.encodePacked("HEARTBEAT_KEY_V1", rawHash, blockNumber));
    }

    function deriveHeartbeatKey(bytes calldata rawHTX, uint64 blockNumber) external pure returns (bytes32) {
        return _deriveHeartbeatKey(rawHTX, blockNumber);
    }

    function _computeCommitteeSize(uint8 escalationLevel) internal view returns (uint32) {
        uint256 size = uint256(config.baseCommitteeSize());
        uint256 growth = uint256(config.committeeSizeGrowthBps());
        for (uint8 i = 0; i < escalationLevel; ) {
            size = (size * (BPS_DENOMINATOR + growth)) / BPS_DENOMINATOR;
            ++i;
        }
        uint256 cap = uint256(config.maxCommitteeSize());
        if (size > cap) size = cap;
        return uint32(size);
    }

    function _snapshotRoundConfig(bytes32 heartbeatKey, uint8 round) internal {
        RoundInfo storage r = rounds[heartbeatKey][round];
        if (r.stakingOps != address(0)) return;

        r.stakingOps = config.stakingOps();
        r.selector   = config.committeeSelector();
        r.slashing   = config.slashingPolicy();
        r.reward     = config.rewardPolicy();

        r.quorumBps         = config.quorumBps();
        r.verificationBps   = config.verificationBps();
        r.responseWindowSec = SafeCast.toUint64(config.responseWindow());
        r.jailDurationSec   = SafeCast.toUint64(config.jailDuration());
    }

    function submitHeartbeat(bytes calldata rawHTX, uint64 snapshotId)
        external
        whenNotPaused
        nonReentrant
        returns (bytes32 heartbeatKey)
    {
        heartbeatKey = _deriveHeartbeatKey(rawHTX, uint64(block.number));
        if (snapshotId == 0) revert SnapshotBlockUnavailable(snapshotId);

        Heartbeat storage w = heartbeats[heartbeatKey];
        bytes32 rawHash = keccak256(rawHTX);
        if (w.status == HeartbeatStatus.None) {
            uint256 bond = config.heartbeatBond();
            if (bond == 0) revert ZeroHeartbeatBond();
            address bondToken = IStakingOperators(config.stakingOps()).stakingToken();
            IERC20(bondToken).safeTransferFrom(msg.sender, address(this), bond);

            w.submitter = msg.sender;
            w.bondToken = bondToken;
            w.bondBurnBps = config.heartbeatBondBurnBps();
            w.bondAmount = bond;
            emit HeartbeatBonded(heartbeatKey, msg.sender, bond);

            w.status = HeartbeatStatus.Pending;
            w.createdAt = uint64(block.timestamp);
            w.currentRound = 1;
            w.escalationLevel = 0;
            w.maxEscalationsSnapshot = config.maxEscalations();
            w.rawHTXHash = rawHash;

            _startRound(heartbeatKey, 1, snapshotId, rawHTX);

            emit HeartbeatStatusChanged(heartbeatKey, HeartbeatStatus.None, HeartbeatStatus.Pending, 1);
        } else {
            if (w.rawHTXHash != rawHash) revert RawHTXHashMismatch();
        }

        emit HeartbeatEnqueued(heartbeatKey, rawHTX, msg.sender);
    }

    function _startRound(bytes32 heartbeatKey, uint8 round, uint64 explicitSnapshotId, bytes calldata rawHTX) internal returns (address[] memory members) {
        Heartbeat storage w = heartbeats[heartbeatKey];
        if (w.status != HeartbeatStatus.Pending) revert NotPending();

        _snapshotRoundConfig(heartbeatKey, round);

        RoundInfo storage r = rounds[heartbeatKey][round];

        IStakingOperators stakingOps = IStakingOperators(r.stakingOps);
        ICommitteeSelector selector  = ICommitteeSelector(r.selector);

        uint64 snapshotId = explicitSnapshotId;
        if (snapshotId == 0) {
            snapshotId = stakingOps.snapshot();
        } else {
            if (snapshotId >= block.number) revert SnapshotBlockUnavailable(snapshotId);
            if (blockhash(uint256(snapshotId)) == bytes32(0)) revert SnapshotBlockUnavailable(snapshotId);
        }
        r.snapshotId = snapshotId;

        uint32 targetSize = _computeCommitteeSize(w.escalationLevel);
        members = selector.selectCommittee(heartbeatKey, round, targetSize, snapshotId);
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

            leaves[i] = keccak256(abi.encodePacked(bytes1(0xA1), address(this), heartbeatKey, round, op));
            ++i;
        }

        r.committeeRoot = _computeMerkleRoot(leaves);
        r.committeeTotalStake = totalStake;

        r.committeeSize = uint32(len);
        r.startedAt = uint64(block.timestamp);
        r.deadline = uint64(block.timestamp + uint256(r.responseWindowSec));

        emit RoundStarted(heartbeatKey, round, r.committeeRoot, r.snapshotId, r.startedAt, r.deadline, members, rawHTX);
    }

    function submitVerdict(bytes32 heartbeatKey, uint8 verdict, bytes32[] calldata memberProof)
        external
        whenNotPaused
        nonReentrant
    {
        _submitVerdict(msg.sender, heartbeatKey, verdict, memberProof, 0);
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
            _submitVerdict(v.operator, v.heartbeatKey, v.verdict, v.memberProof, v.round);
            ++i;
        }
    }

    function _verifyVoteSig(SignedBatchedVote calldata v) internal view {
        RoundInfo storage r = rounds[v.heartbeatKey][v.round];
        if (r.committeeRoot == bytes32(0) || r.snapshotId == 0) revert CommitteeNotStarted();

        bytes32 digest = _voteDigest(v.heartbeatKey, v.round, v.verdict);
        address signer = ECDSA.recover(digest, v.sigV, v.sigR, v.sigS);
        if (signer == address(0) || signer != v.operator) revert InvalidSignature();
    }

    function voteDigest(bytes32 heartbeatKey, uint8 round, uint8 verdict) external view returns (bytes32) {
        return _voteDigest(heartbeatKey, round, verdict);
    }

    function _voteDigest(bytes32 heartbeatKey, uint8 round, uint8 verdict) internal view returns (bytes32) {
        RoundInfo storage r = rounds[heartbeatKey][round];
        if (r.committeeRoot == bytes32(0) || r.snapshotId == 0) revert CommitteeNotStarted();
        bytes32 structHash = keccak256(abi.encode(VOTE_TYPEHASH, heartbeatKey, round, verdict, r.snapshotId, r.committeeRoot));
        return _hashTypedDataV4(structHash);
    }

    function _submitVerdict(
        address operator,
        bytes32 heartbeatKey,
        uint8 verdict,
        bytes32[] calldata memberProof,
        uint8 explicitRound
    ) internal {
        if (verdict == 0 || verdict > 3) revert InvalidVerdict();

        Heartbeat storage w = heartbeats[heartbeatKey];
        if (w.status != HeartbeatStatus.Pending) revert NotPending();

        uint8 round = explicitRound == 0 ? w.currentRound : explicitRound;
        if (round != w.currentRound) revert InvalidRound();

        RoundInfo storage r = rounds[heartbeatKey][round];
        if (r.committeeRoot == bytes32(0)) revert CommitteeNotStarted();
        if (block.timestamp > r.deadline) revert RoundClosed();
        if (r.finalized) revert RoundAlreadyFinalized();

        bytes32 leaf = keccak256(abi.encodePacked(bytes1(0xA1), address(this), heartbeatKey, round, operator));
        if (!MerkleProof.verifyCalldata(memberProof, r.committeeRoot, leaf)) revert NotInCommittee();

        uint256 packed = votePacked[heartbeatKey][round][operator];
        if (_responded(packed)) revert AlreadyResponded();

        IStakingOperators stakingOps = IStakingOperators(r.stakingOps);
        uint256 weight = stakingOps.stakeAt(operator, r.snapshotId);
        if (weight == 0 || weight > WEIGHT_MASK_224) revert ZeroStake();

        uint256 newPacked = uint256(verdict) | RESPONDED_BIT | (weight << WEIGHT_SHIFT);
        votePacked[heartbeatKey][round][operator] = newPacked;

        r.totalRespondedStake += weight;

        if (verdict == 1) {
            r.validStake += weight;
            r.validVotesCount += 1;
        } else if (verdict == 2) {
            r.invalidStake += weight;
        } else {
            r.errorStake += weight;
        }

        emit OperatorVoted(heartbeatKey, round, operator, verdict, weight);
    }

    function escalateOrExpire(bytes32 heartbeatKey, bytes calldata rawHTX) external whenNotPaused nonReentrant {
        Heartbeat storage w = heartbeats[heartbeatKey];
        if (w.status != HeartbeatStatus.Pending) revert NotPending();
        if (w.rawHTXHash == bytes32(0) || keccak256(rawHTX) != w.rawHTXHash) revert RawHTXHashMismatch();

        uint8 round = w.currentRound;
        RoundInfo storage r = rounds[heartbeatKey][round];
        if (r.finalized) revert RoundAlreadyFinalized();
        if (r.committeeRoot == bytes32(0)) revert CommitteeNotStarted();
        if (block.timestamp <= r.deadline) revert BeforeDeadline();

        uint256 total = r.committeeTotalStake;
        ISlashingPolicy.Outcome outcome = ISlashingPolicy.Outcome.Inconclusive;

        if (total > 0) {
            uint256 quorum = Math.mulDiv(r.totalRespondedStake, BPS_DENOMINATOR, total);
            if (quorum >= r.quorumBps) {
                uint256 validBps   = Math.mulDiv(r.validStake, BPS_DENOMINATOR, total);
                uint256 invalidBps = Math.mulDiv(r.invalidStake, BPS_DENOMINATOR, total);
                if (validBps >= r.verificationBps) outcome = ISlashingPolicy.Outcome.ValidThreshold;
                else if (invalidBps >= r.verificationBps) outcome = ISlashingPolicy.Outcome.InvalidThreshold;
            }
        }

        if (outcome == ISlashingPolicy.Outcome.ValidThreshold || outcome == ISlashingPolicy.Outcome.InvalidThreshold) {
            _finalizeRound(heartbeatKey, round, w, r, outcome);
            return;
        }

        _finalizeRound(heartbeatKey, round, w, r, ISlashingPolicy.Outcome.Inconclusive);
        if (w.escalationLevel < w.maxEscalationsSnapshot) {
            ++w.escalationLevel;
            ++w.currentRound;
            _startRound(heartbeatKey, w.currentRound, 0, rawHTX);
        } else {
            HeartbeatStatus old = w.status;
            w.status = HeartbeatStatus.Expired;
            emit HeartbeatStatusChanged(heartbeatKey, old, w.status, round);
            _settleExpiredBond(heartbeatKey, w);
        }
    }

    function _finalizeRound(
        bytes32 heartbeatKey,
        uint8 round,
        Heartbeat storage w,
        RoundInfo storage r,
        ISlashingPolicy.Outcome outcome
    ) internal {
        if (r.finalized) revert RoundAlreadyFinalized();

        r.finalized = true;
        roundOutcome[heartbeatKey][round] = outcome;

        if (outcome == ISlashingPolicy.Outcome.ValidThreshold) {
            HeartbeatStatus old = w.status;
            w.status = HeartbeatStatus.Verified;
            emit HeartbeatStatusChanged(heartbeatKey, old, w.status, round);
            _refundBond(heartbeatKey, w);
        } else if (outcome == ISlashingPolicy.Outcome.InvalidThreshold) {
            HeartbeatStatus old2 = w.status;
            w.status = HeartbeatStatus.Invalid;
            emit HeartbeatStatusChanged(heartbeatKey, old2, w.status, round);
            _burnBond(heartbeatKey, w, w.bondAmount);
        }

        emit RoundFinalized(heartbeatKey, round, outcome);

        _notifySlashing(heartbeatKey, round, r, outcome);
    }

    function _notifySlashing(bytes32 heartbeatKey, uint8 round, RoundInfo storage r, ISlashingPolicy.Outcome outcome) internal {
        if (slashingNotified[heartbeatKey][round]) return;
        bytes memory payload =
            abi.encodeWithSelector(ISlashingPolicy.onRoundFinalized.selector, heartbeatKey, round, outcome, r.committeeRoot, r.committeeSize);
        bool ok = _safeSlashingCall(r.slashing, payload);
        if (ok) {
            slashingNotified[heartbeatKey][round] = true;
        } else {
            emit SlashingCallbackFailed(heartbeatKey, round, "");
        }
    }

    function _safeSlashingCall(address target, bytes memory payload) internal returns (bool ok) {
        uint256 gasLimit = slashingGasLimit;
        assembly ("memory-safe") {
            let ptr := add(payload, 0x20)
            let len := mload(payload)
            ok := call(gasLimit, target, 0, ptr, len, 0, 0)
        }
    }

    function _settleExpiredBond(bytes32 heartbeatKey, Heartbeat storage w) internal {
        uint256 remaining = w.bondAmount;
        if (remaining == 0) return;
        uint256 burnAmount = Math.mulDiv(remaining, w.bondBurnBps, BPS_DENOMINATOR);
        if (burnAmount > 0) _burnBond(heartbeatKey, w, burnAmount);
        _refundBond(heartbeatKey, w);
    }

    function _refundBond(bytes32 heartbeatKey, Heartbeat storage w) internal {
        uint256 amount = w.bondAmount;
        if (amount == 0) return;
        w.bondAmount = 0;
        IERC20(w.bondToken).safeTransfer(w.submitter, amount);
        emit HeartbeatBondRefunded(heartbeatKey, w.submitter, amount);
    }

    function _burnBond(bytes32 heartbeatKey, Heartbeat storage w, uint256 amount) internal {
        if (amount == 0) return;
        if (amount > w.bondAmount) amount = w.bondAmount;
        w.bondAmount -= amount;
        if (!_tryBurn(w.bondToken, amount)) {
            IERC20(w.bondToken).safeTransfer(BURN_ADDRESS, amount);
        }
        emit HeartbeatBondBurned(heartbeatKey, amount);
    }

    function _tryBurn(address token, uint256 amount) internal returns (bool) {
        (bool ok, ) = token.call(abi.encodeWithSignature("burn(uint256)", amount));
        return ok;
    }

    function retrySlashing(bytes32 heartbeatKey, uint8 round) external whenNotPaused nonReentrant {
        RoundInfo storage r = rounds[heartbeatKey][round];
        if (!r.finalized) revert RoundNotFinalized();
        if (slashingNotified[heartbeatKey][round]) return;

        ISlashingPolicy.Outcome outcome = roundOutcome[heartbeatKey][round];
        _notifySlashing(heartbeatKey, round, r, outcome);
    }

    function distributeRewards(bytes32 heartbeatKey, uint8 round, address[] calldata sortedVoters)
        external
        whenNotPaused
        nonReentrant
    {
        RoundInfo storage r = rounds[heartbeatKey][round];
        if (!r.finalized) revert RoundNotFinalized();
        if (rewardsDone[heartbeatKey][round]) revert RewardsAlreadyDone();

        ISlashingPolicy.Outcome outcome = roundOutcome[heartbeatKey][round];
        if (outcome != ISlashingPolicy.Outcome.ValidThreshold && outcome != ISlashingPolicy.Outcome.InvalidThreshold) {
            revert InvalidOutcome();
        }

        uint8 expectedVerdict = outcome == ISlashingPolicy.Outcome.ValidThreshold ? 1 : 2;
        uint256 expectedStake = outcome == ISlashingPolicy.Outcome.ValidThreshold ? r.validStake : r.invalidStake;

        uint256 n = sortedVoters.length;
        if (outcome == ISlashingPolicy.Outcome.ValidThreshold && n != uint256(r.validVotesCount)) {
            revert InvalidVoterCount(n, r.validVotesCount);
        }

        address last = address(0);
        uint256 sumWeights;
        uint256[] memory weights = new uint256[](n);

        for (uint256 i = 0; i < n; ) {
            address op = sortedVoters[i];
            if (op <= last) revert UnsortedVoters();
            last = op;

            uint256 packed = votePacked[heartbeatKey][round][op];
            if ((packed & RESPONDED_BIT) == 0) revert InvalidVoterInList();
            if (uint8(packed & 0x3) != expectedVerdict) revert InvalidVoterInList();

            uint256 wgt = _weight(packed);
            weights[i] = wgt;
            sumWeights += wgt;

            ++i;
        }

        if (sumWeights != expectedStake) revert InvalidVoterWeightSum(sumWeights, expectedStake);

        IRewardPolicy(r.reward).accrueWeights(heartbeatKey, round, sortedVoters, weights);
        rewardsDone[heartbeatKey][round] = true;
        emit RewardsDistributed(heartbeatKey, round, n, sumWeights);
    }

    function abandonRewardDistribution(bytes32 heartbeatKey, uint8 round) external onlyOwner {
        rewardsDone[heartbeatKey][round] = true;
        emit RewardDistributionAbandoned(heartbeatKey, round);
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
                ++i;
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

    function _isProtocolConfig(address candidate) internal view returns (bool) {
        if (candidate.code.length == 0) return false;
        (bool ok, ) = candidate.staticcall(abi.encodeWithSelector(IProtocolConfig.quorumBps.selector));
        return ok;
    }

    // --- Views for policies/off-chain ---

    function getVotePacked(bytes32 heartbeatKey, uint8 round, address operator) external view returns (uint256) {
        return votePacked[heartbeatKey][round][operator];
    }

    struct RoundView {
        HeartbeatStatus status;
        uint8 round;
        bool finalized;
        uint64 startedAt;
        uint64 deadline;
        uint64 snapshotId;
        uint32 committeeSize;
        uint256 committeeTotalStake;
        uint256 totalRespondedStake;
        uint256 validStake;
        uint256 invalidStake;
        uint256 errorStake;
        uint16 quorumBps;
        uint16 verificationBps;
    }

    /// @notice Lightweight view for keepers to decide whether to call escalateOrExpire.
    function getCurrentRoundView(bytes32 heartbeatKey) external view returns (RoundView memory v) {
        Heartbeat storage w = heartbeats[heartbeatKey];
        uint8 round = w.currentRound;
        RoundInfo storage r = rounds[heartbeatKey][round];
        v = RoundView({
            status: w.status,
            round: round,
            finalized: r.finalized,
            startedAt: r.startedAt,
            deadline: r.deadline,
            snapshotId: r.snapshotId,
            committeeSize: r.committeeSize,
            committeeTotalStake: r.committeeTotalStake,
            totalRespondedStake: r.totalRespondedStake,
            validStake: r.validStake,
            invalidStake: r.invalidStake,
            errorStake: r.errorStake,
            quorumBps: r.quorumBps,
            verificationBps: r.verificationBps
        });
    }

    /// @notice Returns whether the current round is past deadline and still pending.
    function isPastDeadline(bytes32 heartbeatKey) external view returns (bool) {
        Heartbeat storage w = heartbeats[heartbeatKey];
        if (w.status != HeartbeatStatus.Pending) return false;
        RoundInfo storage r = rounds[heartbeatKey][w.currentRound];
        if (r.finalized || r.deadline == 0) return false;
        return block.timestamp > r.deadline;
    }

    function getRoundForPolicy(bytes32 heartbeatKey, uint8 round)
        external
        view
        returns (bool, ISlashingPolicy.Outcome, bytes32, address, uint64, uint32)
    {
        RoundInfo storage r = rounds[heartbeatKey][round];
        return (r.finalized, roundOutcome[heartbeatKey][round], r.committeeRoot, r.stakingOps, r.jailDurationSec, r.committeeSize);
    }

    function nodeCount() external view returns (uint256) {
        address[] memory active = IStakingOperators(config.stakingOps()).getActiveOperators();
        return active.length;
    }

    function getNodes() external view returns (address[] memory) {
        return IStakingOperators(config.stakingOps()).getActiveOperators();
    }
}
