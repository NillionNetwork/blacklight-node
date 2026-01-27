//! Prometheus metrics exporter for HeartbeatManager monitoring.
//!
//! This binary:
//! 1. Loads configuration from CLI/environment
//! 2. Registers Prometheus metrics
//! 3. Loads historical events to backfill counters
//! 4. Spawns event collectors for live streaming
//! 5. Spawns operator data poller
//! 6. Starts HTTP server on /metrics endpoint

use anyhow::Result;
use blacklight::config::{MetricsCliArgs, MetricsConfig};
use blacklight::contract_client::{BlacklightClient, ContractConfig, HeartbeatManagerClient};
use blacklight::metrics::{
    collect_htx_enqueued, collect_operator_voted, collect_round_started, load_historical_events,
    poll_operator_data, register_metrics, start_http_server, MetricsState,
};
use clap::Parser;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinSet;
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Dummy private key for read-only operations.
/// The metrics exporter only reads data, never signs transactions.
const READ_ONLY_PRIVATE_KEY: &str =
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    // Parse CLI arguments
    let cli_args = MetricsCliArgs::parse();
    let config = MetricsConfig::load(cli_args)?;

    info!("Blacklight Metrics Exporter starting...");
    info!("HeartbeatManager: {}", config.manager_contract_address);
    info!("StakingOperators: {}", config.staking_contract_address);
    info!("Metrics port: {}", config.metrics_port);

    // Register Prometheus metrics
    register_metrics()?;
    info!("Prometheus metrics registered");

    // Create contract config
    let contract_config = ContractConfig::new(
        config.rpc_url.clone(),
        config.manager_contract_address,
        config.staking_contract_address,
        config.token_contract_address,
    );

    // Create initial client for historical data loading
    let client =
        BlacklightClient::new(contract_config.clone(), READ_ONLY_PRIVATE_KEY.to_string()).await?;
    info!("Connected to blockchain");

    // Shared state for tracking active HTXs
    let state = Arc::new(RwLock::new(MetricsState::new()));

    // Load historical events to backfill counters
    if let Err(e) = load_historical_events(&client, config.lookback_blocks, state.clone()).await {
        warn!("Failed to load historical events: {}", e);
        // Continue anyway - live streaming will work
    }

    // Create task set for spawned collectors
    let mut tasks = JoinSet::new();

    // Spawn HTTP server
    let metrics_port = config.metrics_port;
    tasks.spawn(async move {
        if let Err(e) = start_http_server(metrics_port).await {
            error!("HTTP server error: {}", e);
        }
    });

    // Create Arc-wrapped HeartbeatManagerClient for event listeners
    // We need to create a new manager client that can be wrapped in Arc
    let tx_lock = Arc::new(Mutex::new(()));
    let provider = client.provider();
    let manager_enqueued = Arc::new(HeartbeatManagerClient::new(
        provider.clone(),
        contract_config.clone(),
        tx_lock.clone(),
    ));
    let manager_started = Arc::new(HeartbeatManagerClient::new(
        provider.clone(),
        contract_config.clone(),
        tx_lock.clone(),
    ));
    let manager_voted = Arc::new(HeartbeatManagerClient::new(
        provider.clone(),
        contract_config.clone(),
        tx_lock.clone(),
    ));

    // HeartbeatEnqueued collector
    tasks.spawn(async move {
        loop {
            if let Err(e) = collect_htx_enqueued(manager_enqueued.clone()).await {
                error!("HeartbeatEnqueued collector error: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }
    });

    // RoundStarted collector
    let state_for_started = state.clone();
    tasks.spawn(async move {
        loop {
            if let Err(e) = collect_round_started(manager_started.clone(), state_for_started.clone())
                .await
            {
                error!("RoundStarted collector error: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }
    });

    // OperatorVoted collector
    tasks.spawn(async move {
        loop {
            if let Err(e) = collect_operator_voted(manager_voted.clone()).await {
                error!("OperatorVoted collector error: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }
    });

    // Operator data poller
    let poller_config = contract_config.clone();
    let poll_interval = config.operator_poll_interval_secs;
    tasks.spawn(async move {
        if let Err(e) =
            poll_operator_data(poller_config, READ_ONLY_PRIVATE_KEY.to_string(), poll_interval)
                .await
        {
            error!("Operator data poller error: {}", e);
        }
    });

    info!(
        "Metrics exporter running - HTTP server at http://0.0.0.0:{}",
        config.metrics_port
    );

    // Wait for any task to complete (they should run forever)
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(_) => {
                warn!("A task completed unexpectedly");
            }
            Err(e) => {
                error!("Task panicked: {}", e);
            }
        }
    }

    Ok(())
}
