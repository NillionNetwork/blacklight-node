use alloy::{primitives::Bytes, sol, sol_types::SolInterface};
use contract_clients_common::errors::DecodedRevert;

// ============================================================================
// All Known Blacklight Contract Errors (deduplicated)
// ============================================================================
//
// Combined error definitions from every Solidity contract in the project.
// Errors that appear in multiple contracts (e.g. ZeroAddress) are listed once
// since they share the same selector. Used as a catch-all decoder so any
// revert from any contract in the call chain can be identified.

sol! {
    #[derive(Debug, PartialEq, Eq)]
    contract Blacklight {
        // ── Shared / common ──────────────────────────────────────
        error ZeroAddress();
        error ZeroAmount();
        error InsufficientStake();
        error NothingToClaim();
        error NotInCommittee();
        error RoundNotFinalized();
        error InvalidProtocolConfig(address candidate);
        error SnapshotBlockUnavailable(uint64 snapshotId);

        // ── NillionToken ─────────────────────────────────────────
        error NotMinter();

        // ── EmissionsController ──────────────────────────────────
        error ZeroEpochDuration();
        error EmptySchedule();
        error EpochNotElapsed(uint256 currentTime, uint256 readyAt);
        error NoRemainingEpochs();
        error GlobalCapExceeded(uint256 requested, uint256 remaining);
        error InvalidEpoch(uint256 epochId);
        error ValueWithZeroEmission();

        // ── HeartbeatManager ─────────────────────────────────────
        error NotPending();
        error RoundClosed();
        error RoundAlreadyFinalized();
        error ZeroStake();
        error BeforeDeadline();
        error AlreadyResponded();
        error InvalidVerdict();
        error CommitteeNotStarted();
        error InvalidRound();
        error EmptyCommittee();
        error InvalidSignature();
        error InvalidBatchSize();
        error RewardsAlreadyDone();
        error InvalidOutcome();
        error UnsortedVoters();
        error InvalidVoterInList();
        error InvalidVoterWeightSum(uint256 got, uint256 expected);
        error RawHTXHashMismatch();
        error UnauthorizedHeartbeatSubmitter(address caller);
        error InvalidCommitteeMember(address member);
        error InvalidSlashingGasLimit();

        // ── JailingPolicy ────────────────────────────────────────
        error NotHeartbeatManager();
        error AlreadyEnforced();
        error NotJailable();
        error ZeroJailDuration();
        error CommitteeRootMismatch();
        error UnsortedMembers();
        error ProofsLengthMismatch(uint256 operators, uint256 proofs);
        error CommitteeSizeMismatch(uint256 got, uint256 expected);

        // ── OptimismMintableERC20 ────────────────────────────────
        error OnlyBridge();

        // ── ProtocolConfig ───────────────────────────────────────
        error InvalidBps(uint256 bps);
        error InvalidCommitteeCap(uint32 base, uint32 max);
        error InvalidMaxVoteBatchSize(uint256 maxBatch);
        error InvalidModuleAddress(address module);
        error ZeroQuorumBps();
        error ZeroVerificationBps();
        error ZeroResponseWindow();
        error DurationTooLarge(uint256 duration);

        // ── WeightedCommitteeSelector ────────────────────────────
        error ZeroMaxSize();
        error NoOperators();
        error NotAdmin();
        error EmptyCommitteeRequested();
        error InsufficientCommitteeVP(uint256 selectedVP, uint256 requiredVP);
        error ZeroTotalVotingPower();
        error ZeroMinCommitteeVP();

        // ── RewardPolicy ─────────────────────────────────────────
        error AlreadyProcessed();
        error LengthMismatch();
        error UnsortedRecipients();
        error CommitmentMismatch();
        error InsufficientBudget();
        error InsufficientWithdrawable();
        error AccountingFrozen();
        error Insolvent(uint256 balance, uint256 reserved);

        // ── StakingOperators ─────────────────────────────────────
        error DifferentStaker();
        error NotStaker();
        error InsufficientStakeForActivation();
        error OperatorJailed();
        error NoUnbonding();
        error PendingUnbonding();
        error UnbondingExists();
        error NoStake();
        error NotReady();
        error NotActive();
        error NotSnapshotter();
        error TooManyTranches();
        error InvalidAddress();
        error CannotReactivateWhileJailed();
        error OperatorDoesNotExist();
        error StakeOverflow();
        error BatchTooLarge();
        error InvalidUnstakeDelay();
        error UnauthorizedStaker();
        error StakerAlreadyBound();
        error InvalidMaxActiveOperators();
        error TooManyActiveOperators();

        // ── NodeOperator ─────────────────────────────────────────
        error ContractNotConfigured();
        error BelowMinimumStake();
        error FeeTooHigh();
        error FactoryOnly();
        error InvalidUserAssignment();

        // ── NodeOperatorFactory ──────────────────────────────────
        error NoBoundNodeOperator();
        error InvalidNodeOperator();
        error NoFreeNodeOperator();
        error NodeAlreadyRegistered();
        error FactoryNotConfigured();
        error InsufficientFees();
        error TokenMismatch();
        error StakerNotPreapproved();
        error StakingOperatorsQueryFailed();
    }
}

pub use Blacklight::BlacklightErrors;

/// Decoder for all known Blacklight contract errors.
///
/// Can be passed to `decode_revert_with_custom` and similar `_with_custom` functions
/// from `contract_clients_common::errors`.
pub fn blacklight_error_decoder(data: &Bytes) -> Option<DecodedRevert> {
    BlacklightErrors::abi_decode(data)
        .ok()
        .map(|err| DecodedRevert::CustomError(format!("{err:?}")))
}

