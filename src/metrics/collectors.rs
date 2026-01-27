//! Event collectors for populating Prometheus metrics.
//!
//! This module provides functions to:
//! - Listen to blockchain events via WebSocket and update metrics
//! - Poll operator data periodically (stake, ETH balance, active status)
//! - Load historical events on startup for counter backfill

use crate::contract_client::{BlacklightClient, ContractConfig, HeartbeatManagerClient};
use crate::metrics::registry::{
    outcome_to_string, verdict_to_string, CURRENT_BLOCK_NUMBER, HTX_ACTIVE_COUNT,
    HTX_ENQUEUED_TOTAL, HTX_ROUNDS_FINALIZED_TOTAL, HTX_ROUNDS_STARTED_TOTAL, OPERATORS_TOTAL,
    OPERATOR_ETH_BALANCE_WEI, OPERATOR_IS_ACTIVE, OPERATOR_STAKE_WEI, OPERATOR_VOTES_TOTAL,
    OPERATOR_VOTE_WEIGHT_TOTAL, REWARDS_DISTRIBUTED_TOTAL, SLASHING_CALLBACK_FAILED_TOTAL,
    TOTAL_STAKED_WEI, WEBSOCKET_RECONNECTS_TOTAL,
};
use alloy::primitives::U256;
use alloy::providers::DynProvider;
use anyhow::Result;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

/// Shared state for tracking active HTXs
pub struct MetricsState {
    /// Set of heartbeat keys that are currently active (started but not finalized)
    pub active_htxs: HashSet<String>,
}

impl MetricsState {
    pub fn new() -> Self {
        Self {
            active_htxs: HashSet::new(),
        }
    }
}

impl Default for MetricsState {
    fn default() -> Self {
        Self::new()
    }
}

/// Load historical events and backfill counters.
/// Called once on startup before switching to live streaming.
pub async fn load_historical_events(
    client: &BlacklightClient,
    lookback_blocks: u64,
    state: Arc<RwLock<MetricsState>>,
) -> Result<()> {
    info!(
        "Loading historical events from last {} blocks",
        lookback_blocks
    );

    // Load HeartbeatEnqueued events
    let enqueued_events = client
        .manager
        .get_htx_submitted_events_with_lookback(lookback_blocks)
        .await?;
    info!("Loaded {} HeartbeatEnqueued events", enqueued_events.len());
    for event in &enqueued_events {
        let submitter = format!("{:?}", event.submitter);
        HTX_ENQUEUED_TOTAL.with_label_values(&[&submitter]).inc();
    }

    // Load RoundStarted events
    let round_started_events = client
        .manager
        .get_htx_assigned_events_with_lookback(lookback_blocks)
        .await?;
    info!("Loaded {} RoundStarted events", round_started_events.len());
    {
        let mut state_guard = state.write().await;
        for event in &round_started_events {
            let round = event.round.to_string();
            HTX_ROUNDS_STARTED_TOTAL.with_label_values(&[&round]).inc();
            // Track as active
            let htx_key = format!("{:?}", event.heartbeatKey);
            state_guard.active_htxs.insert(htx_key);
        }
        HTX_ACTIVE_COUNT.set(state_guard.active_htxs.len() as f64);
    }

    // Load OperatorVoted events
    let voted_events = client
        .manager
        .get_htx_responded_events_with_lookback(lookback_blocks)
        .await?;
    info!("Loaded {} OperatorVoted events", voted_events.len());
    for event in &voted_events {
        let operator = format!("{:?}", event.operator);
        let verdict = verdict_to_string(event.verdict).to_string();
        OPERATOR_VOTES_TOTAL
            .with_label_values(&[&operator, &verdict])
            .inc();
        // Weight is stored as U256, convert to f64 for counter
        let weight_f64 = u256_to_f64(event.weight);
        OPERATOR_VOTE_WEIGHT_TOTAL
            .with_label_values(&[&operator, &verdict])
            .inc_by(weight_f64);
    }

    info!("Historical event loading complete");
    Ok(())
}

/// Collect HeartbeatEnqueued events (live streaming)
pub async fn collect_htx_enqueued(
    manager: Arc<HeartbeatManagerClient<DynProvider>>,
) -> Result<()> {
    info!("Starting HeartbeatEnqueued event listener");
    manager
        .listen_htx_submitted_events(|event| async move {
            let submitter = format!("{:?}", event.submitter);
            HTX_ENQUEUED_TOTAL.with_label_values(&[&submitter]).inc();
            debug!("HeartbeatEnqueued: submitter={}", submitter);
            Ok(())
        })
        .await
}

/// Collect RoundStarted events (live streaming)
pub async fn collect_round_started(
    manager: Arc<HeartbeatManagerClient<DynProvider>>,
    state: Arc<RwLock<MetricsState>>,
) -> Result<()> {
    info!("Starting RoundStarted event listener");
    manager
        .listen_htx_assigned_events(move |event| {
            let state = state.clone();
            async move {
                let round = event.round.to_string();
                HTX_ROUNDS_STARTED_TOTAL.with_label_values(&[&round]).inc();

                // Track as active
                let htx_key = format!("{:?}", event.heartbeatKey);
                {
                    let mut state_guard = state.write().await;
                    state_guard.active_htxs.insert(htx_key.clone());
                    HTX_ACTIVE_COUNT.set(state_guard.active_htxs.len() as f64);
                }

                debug!(
                    "RoundStarted: htx={}, round={}, members={}",
                    htx_key,
                    round,
                    event.members.len()
                );
                Ok(())
            }
        })
        .await
}

/// Collect OperatorVoted events (live streaming)
pub async fn collect_operator_voted(
    manager: Arc<HeartbeatManagerClient<DynProvider>>,
) -> Result<()> {
    info!("Starting OperatorVoted event listener");
    manager
        .listen_htx_responded_events(|event| async move {
            let operator = format!("{:?}", event.operator);
            let verdict = verdict_to_string(event.verdict).to_string();
            OPERATOR_VOTES_TOTAL
                .with_label_values(&[&operator, &verdict])
                .inc();

            let weight_f64 = u256_to_f64(event.weight);
            OPERATOR_VOTE_WEIGHT_TOTAL
                .with_label_values(&[&operator, &verdict])
                .inc_by(weight_f64);

            debug!(
                "OperatorVoted: operator={}, verdict={}, weight={}",
                operator, verdict, event.weight
            );
            Ok(())
        })
        .await
}

/// Poll operator data periodically (stake, ETH balance, active status)
pub async fn poll_operator_data(
    config: ContractConfig,
    private_key: String,
    poll_interval_secs: u64,
) -> Result<()> {
    info!(
        "Starting operator data poller (interval: {}s)",
        poll_interval_secs
    );

    let mut ticker = interval(Duration::from_secs(poll_interval_secs));

    loop {
        ticker.tick().await;

        // Create a fresh client for each poll to handle any connection issues
        let client = match BlacklightClient::new(config.clone(), private_key.clone()).await {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to create client for operator polling: {}", e);
                WEBSOCKET_RECONNECTS_TOTAL.inc();
                continue;
            }
        };

        // Update block number
        match client.manager.get_block_number().await {
            Ok(block) => {
                CURRENT_BLOCK_NUMBER.set(block as f64);
            }
            Err(e) => {
                warn!("Failed to get block number: {}", e);
            }
        }

        // Get active operators
        let operators = match client.staking.get_active_operators().await {
            Ok(ops) => ops,
            Err(e) => {
                error!("Failed to get active operators: {}", e);
                continue;
            }
        };

        OPERATORS_TOTAL.set(operators.len() as f64);
        let mut total_staked = U256::ZERO;

        for operator in &operators {
            let operator_str = format!("{:?}", operator);

            // Get stake
            match client.staking.stake_of(*operator).await {
                Ok(stake) => {
                    let stake_f64 = u256_to_f64(stake);
                    OPERATOR_STAKE_WEI
                        .with_label_values(&[&operator_str])
                        .set(stake_f64);
                    total_staked = total_staked.saturating_add(stake);
                }
                Err(e) => {
                    warn!("Failed to get stake for {}: {}", operator_str, e);
                }
            }

            // Get ETH balance
            match client.get_balance_of(*operator).await {
                Ok(balance) => {
                    let balance_f64 = u256_to_f64(balance);
                    OPERATOR_ETH_BALANCE_WEI
                        .with_label_values(&[&operator_str])
                        .set(balance_f64);
                }
                Err(e) => {
                    warn!("Failed to get ETH balance for {}: {}", operator_str, e);
                }
            }

            // Get active status
            match client.staking.is_active_operator(*operator).await {
                Ok(is_active) => {
                    OPERATOR_IS_ACTIVE
                        .with_label_values(&[&operator_str])
                        .set(if is_active { 1.0 } else { 0.0 });
                }
                Err(e) => {
                    warn!("Failed to get active status for {}: {}", operator_str, e);
                }
            }
        }

        TOTAL_STAKED_WEI.set(u256_to_f64(total_staked));
        debug!(
            "Polled {} operators, total staked: {}",
            operators.len(),
            total_staked
        );
    }
}

/// Helper to convert U256 to f64 for Prometheus metrics.
/// Note: This loses precision for very large values, but is acceptable for metrics.
fn u256_to_f64(value: U256) -> f64 {
    // For values that fit in u64, use direct conversion
    if value <= U256::from(u64::MAX) {
        let low: u64 = value.try_into().unwrap_or(0);
        return low as f64;
    }
    // For larger values, convert via string and parse (lossy but better than overflow)
    value.to_string().parse::<f64>().unwrap_or(f64::MAX)
}

/// Record a RoundFinalized event
pub fn record_round_finalized(round: u8, outcome: u8) {
    let round_str = round.to_string();
    let outcome_str = outcome_to_string(outcome).to_string();
    HTX_ROUNDS_FINALIZED_TOTAL
        .with_label_values(&[&round_str, &outcome_str])
        .inc();
}

/// Record a RewardsDistributed event
pub fn record_rewards_distributed(round: u8) {
    let round_str = round.to_string();
    REWARDS_DISTRIBUTED_TOTAL
        .with_label_values(&[&round_str])
        .inc();
}

/// Record a SlashingCallbackFailed event
pub fn record_slashing_callback_failed() {
    SLASHING_CALLBACK_FAILED_TOTAL.inc();
}

/// Mark an HTX as finalized (remove from active set)
pub async fn mark_htx_finalized(state: Arc<RwLock<MetricsState>>, htx_key: &str) {
    let mut state_guard = state.write().await;
    state_guard.active_htxs.remove(htx_key);
    HTX_ACTIVE_COUNT.set(state_guard.active_htxs.len() as f64);
}
