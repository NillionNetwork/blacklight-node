# Blacklight Contract System – Contract Overview

Reference overview of the Blacklight contracts: system shape, module boundaries, and critical behaviors to keep in mind for reviewers and integrators.

## System at a Glance
- Stake-weighted, committee-based verification of heartbeats/workloads that progresses submit → voting rounds → verified/invalid/escalated/expired.
- Operator staking + registry (ERC20 stake, metadata, activation gated by min stake/jail state).
- Deterministic committee selection from active operators (grindable randomness surface noted).
- Slashing/jailing hooks on round finalization; permissionless jailing of non-voters/mismatched voters when policy supports it.
- Reward streaming into a spendable budget; valid voters are paid pro-rata; owner can abandon a distribution.
- Separate L1 emissions controller that mints on schedule and bridges to L2.

## Architecture
- **HeartbeatManager (`contracts/src/HeartbeatManager.sol`)** – Orchestrates heartbeat verification rounds, committee creation, vote tallying, escalation, reward distribution trigger, and slashing callbacks. Ownable + pausable + non-reentrant.
- **ProtocolConfig (`contracts/src/ProtocolConfig.sol`)** – Governance-owned registry for module addresses (staking, selector, slashing, rewards) and runtime parameters (committee sizing, quorum/verification thresholds, response window, jail duration, batching caps, minimum operator stake).
- **StakingOperators (`contracts/src/StakingOperators.sol`)** – ERC20 staking with operator registry, minimum stake enforcement, active-set maintenance, stake checkpoints for snapshots, unbonding with delay, slashing + jailing hooks. AccessControl (DEFAULT_ADMIN_ROLE + SLASHER_ROLE), pausable + non-reentrant.
- **WeightedCommitteeSelector (`contracts/src/WeightedCommitteeSelector.sol`)** – Stake-weighted committee sampling without replacement using snapshot voting power; optional trimming to top-N active operators. Admin-upgradable caps/thresholds.
- **RewardPolicy (`contracts/src/RewardPolicy.sol`)** – Streaming reward budget manager; converts unlocked budget into stake-weighted payouts for valid voters; user claims pull rewards. Ownable + non-reentrant.
- **Slashing policies** – `JailingPolicy` implements non-voting/wrong-vote jailing (no slashing); `NoOpSlashingPolicy` is a stub. Policies are invoked by HeartbeatManager post-finalization.
- **EmissionsController (`contracts/src/EmissionsController.sol`)** – L1 token minter on fixed schedule; bridges emissions to L2 recipient via standard bridge. Ownable + non-reentrant.

## Roles / Trust Model
- ProtocolConfig owner: can change module addresses and all protocol parameters.
- HeartbeatManager owner: can swap config address, pause/unpause, abandon reward distribution for a round.
- StakingOperators admin: sets protocol config, heartbeat manager, snapshotter, unstake delay, pause; SLASHER_ROLE can slash/jail; operatorStaker is authorized to stake/unstake for that operator.
- WeightedCommitteeSelector admin: adjusts min committee voting power, max committee size, active-operator cap.
- RewardPolicy owner: funds/withdraws (subject to reserving rules), sets epoch duration, max payout per finalize, clears accounting freeze.
- EmissionsController owner: adjusts L2 gas limit; all mint/bridge calls are permissionless once epoch ready.
- Any user: can submit heartbeats, escalate/expire after deadlines, submit batched signed votes, trigger reward distribution (must supply full sorted voter list), and enforce jailing via policy proofs where supported.
- Core assumption: governance/admin keys are secure (ideally multisig + timelock); misconfiguration or key loss can break the protocol.

## Heartbeat Lifecycle (HeartbeatManager)
1) **Submit** – `submitHeartbeat(rawHTX, snapshotId)` derives `heartbeatKey` from the raw HTX hash + submission block number, snapshots config, seeds round 1, and emits `HeartbeatEnqueued`/`RoundStarted`. Snapshot ID must exist (either provided or via `stakingOps.snapshot()`); raw HTX hash must match across retries.
2) **Committee selection** – Pulls active operators from StakingOperators, uses selector to sample committee of size derived from `baseCommitteeSize * (1+growthBps)^escalations` capped by `maxCommitteeSize`. Committee root built from sorted member list + snapshot stake weights (`leaf = keccak256(0xA1 || manager || heartbeatKey || round || operator)`); empty committee reverts.
3) **Voting** – Committee members submit verdict (1=valid, 2=invalid, 3=error) via single or batched path (`submitVerdictsBatched` uses EIP-712 signatures over heartbeatKey/round/verdict/snapshotId/committeeRoot). Merkle proof required; voting weight is stake at snapshot block. Each operator can vote once per round; duplicate voting rejected.
4) **Finalize / escalation dependency** – When quorum (`quorumBps` of committee stake responded) reached and either valid/invalid stake crosses `verificationBps`, `_finalizeRound` sets heartbeat status (Verified/Invalid) and notifies slashing policy. Otherwise, `escalateOrExpire` can be called after deadline (keeper/off-chain monitor required to ensure progress):
   - If quorum + threshold met post-deadline, finalize with that outcome.
   - Else mark round inconclusive; if escalations remain (`maxEscalations` snapshotted at submission) start next round with larger committee; otherwise heartbeat expires.
   - Finalization of stale rounds depends on someone calling `escalateOrExpire`; without that call rounds can stall until manually triggered.
5) **Rewards** – For valid threshold outcomes, anyone calls `distributeRewards` with sorted list of valid voters. Enforces sorted uniqueness and weight consistency, then calls RewardPolicy `accrueWeights`. Owner can abandon distribution.
6) **Slashing/Jailing callbacks** – `_notifySlashing` invokes policy; failures are logged and can be retried (`retrySlashing`).

## Staking / Operator Mechanics (StakingOperators)
- **Staking & ownership** – Single staker address per operator (`operatorStaker`). `stakeTo` deposits ERC20; checkpoints recorded per block for snapshot queries. Minimum stake for activation comes from ProtocolConfig (`minOperatorStake`, 0 disables).
- **Active set** – Operator considered active if registered, not jailed, and meets min stake. Active set maintained with O(1) swap/pop; `pokeActiveMany` bounded by `MAX_BATCH_POKE`.
- **Unbonding** – `requestUnstake` moves amount into timed tranches (max 32) respecting `unstakeDelay` (configurable within [1 day, 365 days]); withdrawals available after release. If stake+tranches drop to zero, staker link cleared.
- **Registration** – `registerOperator(metadataURI)`, `deactivateOperator`, `reactivateOperator` gated by stake/jail status.
- **Slashing/Jailing** – `slash` reduces active stake then unbonding tranches; burned to `0xdead`. `jail` sets timestamp and deactivates operator. Both gated by SLASHER_ROLE or slashingPolicy address.
- **Snapshots** – `snapshot()` callable by snapshotter or heartbeat manager; returns previous block number for consistent stakeAt queries. `stakeAt` binary searches checkpoints; voting weights are read from the snapshot block.

## Committee Selection (WeightedCommitteeSelector)
- Uses active operators from StakingOperators at provided snapshot. Optional cap to top-N by stake (`maxActiveOperators`, default 1000). Samples without replacement using Fenwick tree weighted by stake; randomness seed uses `blockhash(snapshotId)` or `prevrandao` fallback plus `heartbeatKey/round/snapshotId/picked` (grindable surface acknowledged).
- Enforces non-zero committee size, `maxCommitteeSize` cap, non-empty operator set, and minimum cumulative voting power (`minCommitteeVP` if set). Base size + growth must stay <= `maxCommitteeSize`; oversizing risks OOG during proof checks or batched voting.

## Reward Flow (RewardPolicy)
- **Budgeting** – Tracks `accountedBalance`; new deposits streamed over `epochDuration` into `_spendableBudget` via linear unlock (`streamRemaining`, `streamRatePerSecondWad`, `streamEnd`). `maxPayoutPerFinalize` caps spend per heartbeat finalize (0 = no cap).
- **Accrual** – HeartbeatManager (only caller) submits sorted recipients + weights; commitment hash prevents inconsistent calls. Budget must be available; distributed proportionally. If rounding zeroes all payouts, sends 1 wei to highest weight (ties by lowest address).
- **Claiming** – Users pull rewards; reserves `totalOutstandingRewards`. `sync()` detects underflows; if contract balance drops below reserved totals, `accountingFrozen` blocks accrual/claims until owner clears after restoring solvency.
- **Owner powers** – fund/withdraw (subject to reserved coverage), adjust epoch duration/payout cap, clear freeze.

## Slashing Policies
- **JailingPolicy** – Records finalized round data via HeartbeatManager callback or `recordRound`. Anyone can enforce jailing for non-voters or voters disagreeing with finalized outcome (valid threshold: jail verdicts != valid; invalid threshold: jail verdicts != invalid; inconclusive: only non-voters jailable). Requires Merkle membership proof or full sorted committee list (verifies root). Jail duration from HeartbeatManager snapshot; enforcement idempotent.
- **NoOpSlashingPolicy** – Placeholder that performs no action.

## EmissionsController
- Fixed emission schedule per epoch (immutable array). Anyone can call `mintAndBridgeNextEpoch` once `startTime + epochDuration * mintedEpochs` elapses. Enforces global cap (optional) and remaining epochs. Mints L1 token and bridges to L2 recipient via StandardBridge with configurable `l2GasLimit`; emits `EpochMinted`. Owner can update gas limit and refresh allowance.

## Key Parameters & Guardrails
- `ProtocolConfig`: committee sizing (`baseCommitteeSize`, `committeeSizeGrowthBps`, `maxCommitteeSize`), escalation limit, quorum/verification BPS (capped at 10_000), response window seconds, jail duration seconds, vote batch size cap (0 = unlimited up to hard limit 500), min operator stake. Ensure `baseCommitteeSize*(1+growthBps)^escalations` cannot exceed `maxCommitteeSize` to avoid OOG in voting/verification paths; contract does not enforce “safe” relationships (e.g., positive BPS, verification > 50%), so governance must set sane values.
- Heartbeat voting batch hard limit: 500; configurable soft cap via config.
- RewardPolicy: `epochDuration` non-zero, `maxPayoutPerFinalize` optional cap, accounting freeze if balance < reserved.
- StakingOperators: `unstakeDelay` bounded; max 32 unbonding tranches; `MAX_BATCH_POKE` = 50; stake fits in uint224.
- CommitteeSelector: default `maxActiveOperators` 1000; requires non-zero total voting power.
- EmissionsController: epoch schedule immutable; enforces global cap and readiness time.

## Operational Notes
- **Keeper dependency** – Progress after deadlines is not automatic; `escalateOrExpire` must be called externally. Deployments need monitoring/automation to avoid stuck rounds.
- **Committee sizing sanity** – Governance parameters should keep calculated committee size <= `maxCommitteeSize`; oversized committees increase gas and can cause OOG in batched verification.
- **Monitoring hooks** – Deadlines, escalation count, and reward budget drift are observable; dashboards/alerts can track missed finalizations, accounting freeze state, and paused flags.

## Operational Invariants
- One vote per operator per heartbeat/round; attempts to re-vote revert.
- Voting weight is fixed at snapshotId = roundStartBlock - 1; later stake changes do not affect the round.
- Outcomes: valid threshold → Verified (no further votes/escalations); invalid threshold → Invalid; inconclusive only via `escalateOrExpire`; exhaustion of escalations → Expired.
- For ValidThreshold rounds, `distributeRewards` must include every valid voter exactly once in sorted order and the summed weights must match `validStake`.
- Reward solvency when healthy: token balance should cover `outstandingRewards + streamRemaining + spendableBudget`; otherwise accountingFrozen engages.

## Assumptions and Risks
- **Randomness/grinding** – Committee selection seed uses `blockhash(snapshotId)` or `prevrandao`; submitters can time `rawHTX` and snapshot selection to influence outcomes unless constrained by governance.
- **Off-chain honesty** – Reward distribution relies on a supplied sorted valid-voter list; weight checks prevent inconsistencies but assume the caller includes all valid votes.
- **Config trust** – ProtocolConfig owner can swap modules and loosen thresholds/min stake; deployments rely on strong governance.
- **Streaming solvency** – RewardPolicy freezes if balance dips below obligations; owner intervention is needed to restore and clear the freeze.
- **Bridge dependency** – EmissionsController depends on L1 bridge behavior; minting/bridging are atomic per call but rely on L2 recipient correctness.
