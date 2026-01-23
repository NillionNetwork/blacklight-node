use crate::config::consts::{DEFAULT_LOOKBACK_BLOCKS, MIN_ETH_BALANCE};
use crate::wallet::{check_balance, load_wallet};
use alloy::primitives::{Address, U256};
use anyhow::{Context, Result};
use clap::Parser;
use tracing::info;

/// CLI arguments for the keeper
#[derive(Parser, Debug)]
#[command(name = "keeper")]
#[command(
    about = "Blacklight Keeper - round escalations, rewards, jailing, and emissions",
    long_about = None
)]
pub struct CliArgs {
    /// L2 RPC endpoint (will be converted to WebSocket)
    #[arg(long, env = "L2_RPC_URL")]
    pub l2_rpc_url: String,

    /// L1 RPC endpoint (will be converted to WebSocket)
    #[arg(long, env = "L1_RPC_URL")]
    pub l1_rpc_url: String,

    /// L2 HeartbeatManager contract address
    #[arg(long, env = "L2_HEARTBEAT_MANAGER_ADDRESS")]
    pub l2_heartbeat_manager_address: Address,

    /// L2 JailingPolicy contract address.
    #[arg(long, env = "L2_JAILING_POLICY_ADDRESS")]
    pub l2_jailing_policy_address: Option<Address>,

    /// Disable all jailing actions even if a JailingPolicy address is configured
    #[arg(long, env = "DISABLE_JAILING")]
    pub disable_jailing: bool,

    /// L1 EmissionsController contract address
    #[arg(long, env = "L1_EMISSIONS_CONTROLLER_ADDRESS")]
    pub l1_emissions_controller_address: Address,

    /// Private key for contract interactions
    #[arg(long, env = "PRIVATE_KEY")]
    pub private_key: String,

    /// ETH value (wei) to forward for L1 -> L2 bridge messages
    #[arg(long, env = "L1_BRIDGE_VALUE_WEI", default_value_t = Default::default())]
    pub l1_bridge_value_wei: U256,

    /// Lookback blocks for historical event queries
    #[arg(long, env = "LOOKBACK_BLOCKS", default_value_t = DEFAULT_LOOKBACK_BLOCKS)]
    pub lookback_blocks: u64,

    /// Keeper tick interval in seconds (L2 rounds/rewards/jailing)
    #[arg(long, env = "TICK_INTERVAL_SECS", default_value_t = 5)]
    pub tick_interval_secs: u64,

    /// Emissions check interval in seconds (L1)
    #[arg(long, env = "EMISSIONS_INTERVAL_SECS", default_value_t = 30)]
    pub emissions_interval_secs: u64,
}

/// Keeper configuration with all required values resolved
#[derive(Debug, Clone)]
pub struct KeeperConfig {
    pub l2_rpc_url: String,
    pub l1_rpc_url: String,
    pub l2_heartbeat_manager_address: Address,
    pub l2_jailing_policy_address: Option<Address>,
    pub l1_emissions_controller_address: Address,
    pub private_key: String,
    pub l1_bridge_value: U256,
    pub lookback_blocks: u64,
    pub tick_interval_secs: u64,
    pub emissions_interval_secs: u64,
    pub disable_jailing: bool,
}

impl KeeperConfig {
    /// Load configuration with priority: CLI/env -> state file -> defaults
    /// Generates a new wallet if none exists and checks balances before proceeding
    pub async fn load(cli_args: CliArgs) -> Result<Self> {
        let l2_rpc_url = cli_args.l2_rpc_url;
        let l1_rpc_url = cli_args.l1_rpc_url;
        let l2_heartbeat_manager_address = cli_args.l2_heartbeat_manager_address;
        let l1_emissions_controller_address = cli_args.l1_emissions_controller_address;
        let l2_jailing_policy_address = cli_args.l2_jailing_policy_address;
        let disable_jailing = cli_args.disable_jailing;
        let private_key = cli_args.private_key;
        let l2_jailing_policy_address = if disable_jailing {
            None
        } else {
            l2_jailing_policy_address
        };
        let l1_bridge_value = cli_args.l1_bridge_value_wei;
        let lookback_blocks = cli_args.lookback_blocks;
        let tick_interval_secs = cli_args.tick_interval_secs;
        let emissions_interval_secs = cli_args.emissions_interval_secs;

        let wallet = load_wallet(&private_key)?;
        let address = wallet.address();

        info!("Checking L2 balance for address: {:?}", address);
        let l2_balance = check_balance(&l2_rpc_url, address)
            .await
            .context("Failed to check L2 balance")?;

        info!("Checking L1 balance for address: {:?}", address);
        let l1_balance = check_balance(&l1_rpc_url, address)
            .await
            .context("Failed to check L1 balance")?;

        if l2_balance < MIN_ETH_BALANCE || l1_balance < MIN_ETH_BALANCE {
            anyhow::bail!(
                "Insufficient funds. Keeper requires at least {} ETH on both L1 and L2.",
                alloy::primitives::utils::format_ether(MIN_ETH_BALANCE)
            );
        }

        info!(
            "Loaded KeeperConfig: l2_rpc_url={}, l1_rpc_url={}, heartbeat_manager={}, emissions_controller={}",
            l2_rpc_url, l1_rpc_url, l2_heartbeat_manager_address, l1_emissions_controller_address
        );

        Ok(KeeperConfig {
            l2_rpc_url,
            l1_rpc_url,
            l2_heartbeat_manager_address,
            l2_jailing_policy_address,
            l1_emissions_controller_address,
            private_key,
            l1_bridge_value,
            lookback_blocks,
            tick_interval_secs,
            emissions_interval_secs,
            disable_jailing,
        })
    }
}
