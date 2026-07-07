use alloy::primitives::{Address, U256};
use alloy::signers::local::PrivateKeySigner;
use anyhow::Result;
use clap::Parser;
use contract_clients_common::chain_profile::{ChainProfile, FeeStrategy};
use std::env;
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

    /// L2 ValidationRegistry contract address for ERC-8004 validation responses
    #[arg(long, env = "L2_VALIDATION_REGISTRY_ADDRESS")]
    pub l2_validation_registry_address: Option<Address>,

    /// Enable ERC-8004 keeper functionality
    #[arg(long, env = "ENABLE_ERC8004_KEEPER", default_value_t = false)]
    pub enable_erc8004: bool,

    /// L1 EmissionsController contract address
    #[arg(long, env = "L1_EMISSIONS_CONTROLLER_ADDRESS")]
    pub l1_emissions_controller_address: Address,

    /// L2 staking operators contract address.
    #[arg(long, env = "L2_STAKING_OPERATORS_ADDRESS")]
    pub l2_staking_operators_address: Address,

    /// Private key for contract interactions
    #[arg(long, env = "PRIVATE_KEY")]
    pub private_key: String,

    /// ETH value (wei) to forward for L1 -> L2 bridge messages
    #[arg(long, env = "L1_BRIDGE_VALUE_WEI", default_value_t = Default::default())]
    pub l1_bridge_value_wei: U256,

    /// Lookback blocks for historical event queries
    #[arg(long, env = "LOOKBACK_BLOCKS", default_value_t = 50)]
    pub lookback_blocks: u64,

    // --- Chain profiles (N7, L1 port). Defaults keep today's behaviour bit-identical. ---
    /// Single-chain mode: the L2 and L1 legs are the same chain (L1 deployment) and share
    /// one provider/wallet/balance-check (consumed by the WP8 keeper merge)
    #[arg(long, env = "L1_SINGLE_CHAIN", default_value_t = false)]
    pub l1_single_chain: bool,

    /// Fee strategy for the round-lifecycle (today: L2) leg: "l2-min-priority" (default)
    /// or "eip1559"
    #[arg(long, env = "L2_FEE_STRATEGY")]
    pub l2_fee_strategy: Option<String>,

    /// Fee strategy for the emissions (L1) leg: "l2-min-priority" (default) or "eip1559"
    #[arg(long, env = "L1_FEE_STRATEGY")]
    pub l1_fee_strategy: Option<String>,

    /// Max fee cap in gwei for eip1559 legs (unset = uncapped)
    #[arg(long, env = "MAX_FEE_CAP_GWEI")]
    pub max_fee_cap_gwei: Option<u64>,

    /// Percent fee bump per stuck-tx replacement for eip1559 legs (default 15)
    #[arg(long, env = "FEE_BUMP_PERCENT")]
    pub fee_bump_percent: Option<u8>,

    /// Blocks without a receipt before a stuck tx is re-priced for eip1559 legs (default 3)
    #[arg(long, env = "FEE_BUMP_AFTER_BLOCKS")]
    pub fee_bump_after_blocks: Option<u64>,

    /// Keeper tick interval in seconds (L2 rounds/rewards/jailing)
    #[arg(long, env = "TICK_INTERVAL_SECS", default_value_t = 5)]
    pub tick_interval_secs: u64,

    /// Emissions check interval in seconds (L1)
    #[arg(long, env = "EMISSIONS_INTERVAL_SECS", default_value_t = 30)]
    pub emissions_interval_secs: u64,

    /// The OTEL collector endpoint.
    #[arg(long, env = "OTEL_ENDPOINT")]
    pub otel_endpoint: Option<String>,

    /// The OTEL export interval in seconds.
    #[arg(long, env = "OTEL_EXPORT_INTERVAL_SECS", default_value_t = 15)]
    pub otel_export_interval_secs: u64,

    /// The OTEL export timeout in seconds.
    #[arg(long, env = "OTEL_EXPORT_TIMEOUT_SECS", default_value_t = 30)]
    pub otel_export_timeout_secs: u64,
}

/// Keeper configuration with all required values resolved
#[derive(Debug, Clone)]
pub struct KeeperConfig {
    pub l2_rpc_url: String,
    pub l1_rpc_url: String,
    pub l2_heartbeat_manager_address: Address,
    pub l2_jailing_policy_address: Option<Address>,
    pub l2_validation_registry_address: Option<Address>,
    pub l1_emissions_controller_address: Address,
    pub l2_staking_operators_address: Address,
    pub private_key: String,
    pub l1_bridge_value: U256,
    pub lookback_blocks: u64,
    pub tick_interval: Duration,
    pub emissions_interval: Duration,
    pub disable_jailing: bool,
    pub enable_erc8004: bool,
    pub otel: Option<OtelConfig>,
    /// Chain profile for the round-lifecycle leg (today: the L2). Default = today's rule.
    pub l2_profile: ChainProfile,
    /// Chain profile for the emissions leg (the L1). Default = today's rule.
    pub l1_profile: ChainProfile,
    /// Both legs are one chain, sharing one provider/wallet (L1 deployments; WP8).
    pub l1_single_chain: bool,
}

impl KeeperConfig {
    /// Load configuration with priority: CLI/env -> state file -> defaults
    /// Generates a new wallet if none exists and checks balances before proceeding
    pub async fn load(args: CliArgs) -> Result<Self> {
        let l2_rpc_url = args.l2_rpc_url;
        let l1_rpc_url = args.l1_rpc_url;
        let l2_heartbeat_manager_address = args.l2_heartbeat_manager_address;
        let l1_emissions_controller_address = args.l1_emissions_controller_address;
        let l2_staking_operators_address = args.l2_staking_operators_address;
        let l2_jailing_policy_address = args.l2_jailing_policy_address;
        let disable_jailing = args.disable_jailing;
        let enable_erc8004 = args.enable_erc8004;
        let private_key = args.private_key;
        let l2_jailing_policy_address = if disable_jailing {
            None
        } else {
            l2_jailing_policy_address
        };
        let l2_validation_registry_address = if enable_erc8004 {
            args.l2_validation_registry_address
        } else {
            None
        };
        let l1_bridge_value = args.l1_bridge_value_wei;
        let lookback_blocks = args.lookback_blocks;
        let tick_interval = Duration::from_secs(args.tick_interval_secs);
        let emissions_interval = Duration::from_secs(args.emissions_interval_secs);

        let l2_profile = ChainProfile {
            fee_strategy: FeeStrategy::resolve(
                args.l2_fee_strategy.as_deref(),
                args.max_fee_cap_gwei,
                args.fee_bump_percent,
                args.fee_bump_after_blocks,
            )?,
            lookback_blocks,
            ws: true,
        };
        let l1_profile = ChainProfile {
            fee_strategy: FeeStrategy::resolve(
                args.l1_fee_strategy.as_deref(),
                args.max_fee_cap_gwei,
                args.fee_bump_percent,
                args.fee_bump_after_blocks,
            )?,
            lookback_blocks,
            ws: true,
        };
        let l1_single_chain = args.l1_single_chain;

        let wallet: PrivateKeySigner = private_key.parse()?;
        let address = wallet.address();
        let otel = match (is_otel_disabled(), args.otel_endpoint) {
            (true, _) => {
                info!("OTEL export is disabled via environment variable");
                None
            }
            (false, Some(endpoint)) => Some(OtelConfig {
                endpoint,
                export_timeout: Duration::from_secs(args.otel_export_timeout_secs),
                export_interval: Duration::from_secs(args.otel_export_interval_secs),
            }),
            (false, None) => None,
        };

        info!(
            "Loaded KeeperConfig: l2_rpc_url={l2_rpc_url}, l1_rpc_url={l1_rpc_url}, heartbeat_manager={l2_heartbeat_manager_address}, emissions_controller={l1_emissions_controller_address}, wallet_address={address}"
        );

        Ok(KeeperConfig {
            l2_rpc_url,
            l1_rpc_url,
            l2_heartbeat_manager_address,
            l2_jailing_policy_address,
            l2_validation_registry_address,
            l1_emissions_controller_address,
            l2_staking_operators_address,
            private_key,
            l1_bridge_value,
            lookback_blocks,
            tick_interval,
            emissions_interval,
            disable_jailing,
            enable_erc8004,
            otel,
            l2_profile,
            l1_profile,
            l1_single_chain,
        })
    }
}

#[derive(Debug, Clone)]
pub struct OtelConfig {
    pub endpoint: String,
    pub export_timeout: Duration,
    pub export_interval: Duration,
}

fn is_otel_disabled() -> bool {
    env::var("OTEL_SDK_DISABLED").as_deref() == Ok("true")
}
