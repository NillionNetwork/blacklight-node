use alloy::primitives::{Address, U256};
use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;
use clap::Parser;
use std::time::Duration;
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
    #[arg(long, env = "LOOKBACK_BLOCKS", default_value_t = 50)]
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
    pub tick_interval: Duration,
    pub emissions_interval: Duration,
    pub disable_jailing: bool,
}

impl KeeperConfig {
    /// Load configuration with priority: CLI/env -> state file -> defaults
    /// Generates a new wallet if none exists and checks balances before proceeding
    pub async fn load(args: CliArgs) -> Result<Self> {
        let l2_rpc_url = args.l2_rpc_url;
        let l1_rpc_url = args.l1_rpc_url;
        let l2_heartbeat_manager_address = args.l2_heartbeat_manager_address;
        let l1_emissions_controller_address = args.l1_emissions_controller_address;
        let l2_jailing_policy_address = args.l2_jailing_policy_address;
        let disable_jailing = args.disable_jailing;
        let private_key = args.private_key;
        let l2_jailing_policy_address = if disable_jailing {
            None
        } else {
            l2_jailing_policy_address
        };
        let l1_bridge_value = args.l1_bridge_value_wei;
        let lookback_blocks = args.lookback_blocks;
        let tick_interval = Duration::from_secs(args.tick_interval_secs);
        let emissions_interval = Duration::from_secs(args.emissions_interval_secs);

        let wallet: PrivateKeySigner = private_key.parse()?;
        let address = wallet.address();

        info!(
            "Loaded KeeperConfig: l2_rpc_url={l2_rpc_url}, l1_rpc_url={l1_rpc_url}, heartbeat_manager={l2_heartbeat_manager_address}, emissions_controller={l1_emissions_controller_address}, wallet_address={address}"
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
            tick_interval,
            emissions_interval,
            disable_jailing,
        })
    }
}
