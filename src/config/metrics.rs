//! Configuration for the Prometheus metrics exporter service.

use crate::config::consts::DEFAULT_LOOKBACK_BLOCKS;
use alloy::primitives::Address;
use anyhow::Result;
use clap::Parser;
use tracing::info;

/// Default HTTP port for exposing Prometheus metrics
pub const DEFAULT_METRICS_PORT: u16 = 9090;

/// Default interval in seconds for polling operator data (stake, balance, active status)
pub const DEFAULT_OPERATOR_POLL_INTERVAL_SECS: u64 = 30;

/// CLI arguments for the metrics exporter
#[derive(Parser, Debug)]
#[command(name = "metrics_exporter")]
#[command(
    about = "Blacklight Metrics Exporter - Prometheus metrics for HeartbeatManager",
    long_about = None
)]
pub struct CliArgs {
    /// RPC endpoint (will be converted to WebSocket for event streaming)
    #[arg(long, env = "RPC_URL")]
    pub rpc_url: String,

    /// HeartbeatManager contract address
    #[arg(long, env = "MANAGER_CONTRACT_ADDRESS")]
    pub manager_contract_address: Address,

    /// StakingOperators contract address
    #[arg(long, env = "STAKING_CONTRACT_ADDRESS")]
    pub staking_contract_address: Address,

    /// NilToken contract address
    #[arg(long, env = "TOKEN_CONTRACT_ADDRESS")]
    pub token_contract_address: Address,

    /// HTTP port for exposing Prometheus metrics
    #[arg(long, env = "METRICS_PORT", default_value_t = DEFAULT_METRICS_PORT)]
    pub metrics_port: u16,

    /// Interval in seconds for polling operator data
    #[arg(long, env = "OPERATOR_POLL_INTERVAL_SECS", default_value_t = DEFAULT_OPERATOR_POLL_INTERVAL_SECS)]
    pub operator_poll_interval_secs: u64,

    /// Number of blocks to look back for historical events on startup
    #[arg(long, env = "LOOKBACK_BLOCKS", default_value_t = DEFAULT_LOOKBACK_BLOCKS)]
    pub lookback_blocks: u64,
}

/// Metrics exporter configuration with all required values resolved
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    pub rpc_url: String,
    pub manager_contract_address: Address,
    pub staking_contract_address: Address,
    pub token_contract_address: Address,
    pub metrics_port: u16,
    pub operator_poll_interval_secs: u64,
    pub lookback_blocks: u64,
}

impl MetricsConfig {
    /// Load configuration from CLI arguments and environment variables
    pub fn load(cli_args: CliArgs) -> Result<Self> {
        let config = MetricsConfig {
            rpc_url: cli_args.rpc_url,
            manager_contract_address: cli_args.manager_contract_address,
            staking_contract_address: cli_args.staking_contract_address,
            token_contract_address: cli_args.token_contract_address,
            metrics_port: cli_args.metrics_port,
            operator_poll_interval_secs: cli_args.operator_poll_interval_secs,
            lookback_blocks: cli_args.lookback_blocks,
        };

        info!(
            "Loaded MetricsConfig: rpc_url={}, manager={}, staking={}, port={}",
            config.rpc_url,
            config.manager_contract_address,
            config.staking_contract_address,
            config.metrics_port
        );

        Ok(config)
    }
}
