//! Prometheus metrics registry and metric definitions.
//!
//! This module defines all Prometheus metrics used by the metrics exporter
//! for monitoring HeartbeatManager contract events and operator status.

use lazy_static::lazy_static;
use prometheus::{
    opts, register_counter_vec, register_gauge_vec, CounterVec, Gauge, GaugeVec, Registry,
};

lazy_static! {
    /// Global Prometheus registry
    pub static ref REGISTRY: Registry = Registry::new();

    // ========================================================================
    // HTX Lifecycle Metrics
    // ========================================================================

    /// Counter for total HTX submissions (HeartbeatEnqueued events)
    /// Labels: submitter address
    pub static ref HTX_ENQUEUED_TOTAL: CounterVec = register_counter_vec!(
        opts!("htx_enqueued_total", "Total number of HTX submissions"),
        &["submitter"]
    ).expect("Failed to create htx_enqueued_total metric");

    /// Counter for rounds started (RoundStarted events)
    /// Labels: round number
    pub static ref HTX_ROUNDS_STARTED_TOTAL: CounterVec = register_counter_vec!(
        opts!("htx_rounds_started_total", "Total number of rounds started"),
        &["round"]
    ).expect("Failed to create htx_rounds_started_total metric");

    /// Counter for rounds finalized (RoundFinalized events)
    /// Labels: round number, outcome (Inconclusive=0, ValidThreshold=1, InvalidThreshold=2)
    pub static ref HTX_ROUNDS_FINALIZED_TOTAL: CounterVec = register_counter_vec!(
        opts!("htx_rounds_finalized_total", "Total number of rounds finalized"),
        &["round", "outcome"]
    ).expect("Failed to create htx_rounds_finalized_total metric");

    /// Gauge for active (in-progress) HTXs
    pub static ref HTX_ACTIVE_COUNT: Gauge = Gauge::new(
        "htx_active_count",
        "Number of HTXs currently in progress"
    ).expect("Failed to create htx_active_count metric");

    // ========================================================================
    // Operator Voting Metrics
    // ========================================================================

    /// Counter for operator votes (OperatorVoted events)
    /// Labels: operator address, verdict (1=Valid, 2=Invalid, 3=Error)
    pub static ref OPERATOR_VOTES_TOTAL: CounterVec = register_counter_vec!(
        opts!("operator_votes_total", "Total votes by operator and verdict"),
        &["operator", "verdict"]
    ).expect("Failed to create operator_votes_total metric");

    /// Counter for operator vote weight (OperatorVoted events)
    /// Labels: operator address, verdict
    pub static ref OPERATOR_VOTE_WEIGHT_TOTAL: CounterVec = register_counter_vec!(
        opts!("operator_vote_weight_total", "Total vote weight by operator and verdict"),
        &["operator", "verdict"]
    ).expect("Failed to create operator_vote_weight_total metric");

    // ========================================================================
    // Operator Status Metrics (polled periodically)
    // ========================================================================

    /// Gauge for operator stake in NIL token base units
    /// Labels: operator address
    pub static ref OPERATOR_STAKE_WEI: GaugeVec = register_gauge_vec!(
        opts!("operator_stake_wei", "Operator stake amount in NIL token base units"),
        &["operator"]
    ).expect("Failed to create operator_stake_wei metric");

    /// Gauge for operator ETH balance in wei
    /// Labels: operator address
    pub static ref OPERATOR_ETH_BALANCE_WEI: GaugeVec = register_gauge_vec!(
        opts!("operator_eth_balance_wei", "Operator ETH balance in wei"),
        &["operator"]
    ).expect("Failed to create operator_eth_balance_wei metric");

    /// Gauge for operator NIL token balance in base units
    /// Labels: operator address
    pub static ref OPERATOR_NIL_BALANCE_BASE: GaugeVec = register_gauge_vec!(
        opts!(
            "operator_nil_balance_base",
            "Operator NIL token balance in base units"
        ),
        &["operator"]
    )
    .expect("Failed to create operator_nil_balance_base metric");

    /// Gauge for operator active status (1 = active, 0 = inactive)
    /// Labels: operator address
    pub static ref OPERATOR_IS_ACTIVE: GaugeVec = register_gauge_vec!(
        opts!("operator_is_active", "Whether operator is active (1) or not (0)"),
        &["operator"]
    ).expect("Failed to create operator_is_active metric");

    /// Gauge for total number of registered operators
    pub static ref OPERATORS_TOTAL: Gauge = Gauge::new(
        "operators_total",
        "Total number of registered operators"
    ).expect("Failed to create operators_total metric");

    /// Gauge for total staked amount across all operators in NIL token base units
    pub static ref TOTAL_STAKED_WEI: Gauge = Gauge::new(
        "total_staked_wei",
        "Total amount staked across all operators in NIL token base units"
    ).expect("Failed to create total_staked_wei metric");

    // ========================================================================
    // Reward and Slashing Metrics
    // ========================================================================

    /// Counter for reward distribution events
    pub static ref REWARDS_DISTRIBUTED_TOTAL: CounterVec = register_counter_vec!(
        opts!("rewards_distributed_total", "Total number of reward distributions"),
        &["round"]
    ).expect("Failed to create rewards_distributed_total metric");

    /// Counter for slashing callback failures
    pub static ref SLASHING_CALLBACK_FAILED_TOTAL: Gauge = Gauge::new(
        "slashing_callback_failed_total",
        "Total number of slashing callback failures"
    ).expect("Failed to create slashing_callback_failed_total metric");

    // ========================================================================
    // System Health Metrics
    // ========================================================================

    /// Gauge for current block number
    pub static ref CURRENT_BLOCK_NUMBER: Gauge = Gauge::new(
        "current_block_number",
        "Latest processed block number"
    ).expect("Failed to create current_block_number metric");

    /// Counter for WebSocket reconnections
    pub static ref WEBSOCKET_RECONNECTS_TOTAL: Gauge = Gauge::new(
        "websocket_reconnects_total",
        "Total number of WebSocket reconnection attempts"
    ).expect("Failed to create websocket_reconnects_total metric");
}

/// Register all metrics with the global registry.
/// Call this once at startup.
pub fn register_metrics() -> prometheus::Result<()> {
    REGISTRY.register(Box::new(HTX_ENQUEUED_TOTAL.clone()))?;
    REGISTRY.register(Box::new(HTX_ROUNDS_STARTED_TOTAL.clone()))?;
    REGISTRY.register(Box::new(HTX_ROUNDS_FINALIZED_TOTAL.clone()))?;
    REGISTRY.register(Box::new(HTX_ACTIVE_COUNT.clone()))?;
    REGISTRY.register(Box::new(OPERATOR_VOTES_TOTAL.clone()))?;
    REGISTRY.register(Box::new(OPERATOR_VOTE_WEIGHT_TOTAL.clone()))?;
    REGISTRY.register(Box::new(OPERATOR_STAKE_WEI.clone()))?;
    REGISTRY.register(Box::new(OPERATOR_ETH_BALANCE_WEI.clone()))?;
    REGISTRY.register(Box::new(OPERATOR_NIL_BALANCE_BASE.clone()))?;
    REGISTRY.register(Box::new(OPERATOR_IS_ACTIVE.clone()))?;
    REGISTRY.register(Box::new(OPERATORS_TOTAL.clone()))?;
    REGISTRY.register(Box::new(TOTAL_STAKED_WEI.clone()))?;
    REGISTRY.register(Box::new(REWARDS_DISTRIBUTED_TOTAL.clone()))?;
    REGISTRY.register(Box::new(SLASHING_CALLBACK_FAILED_TOTAL.clone()))?;
    REGISTRY.register(Box::new(CURRENT_BLOCK_NUMBER.clone()))?;
    REGISTRY.register(Box::new(WEBSOCKET_RECONNECTS_TOTAL.clone()))?;
    Ok(())
}

/// Convert a verdict code to a human-readable string for labels
pub fn verdict_to_string(verdict: u8) -> &'static str {
    match verdict {
        1 => "valid",
        2 => "invalid",
        3 => "error",
        _ => "unknown",
    }
}

/// Convert an outcome code to a human-readable string for labels
pub fn outcome_to_string(outcome: u8) -> &'static str {
    match outcome {
        0 => "inconclusive",
        1 => "valid_threshold",
        2 => "invalid_threshold",
        _ => "unknown",
    }
}
