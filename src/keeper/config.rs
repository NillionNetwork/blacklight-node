use alloy::primitives::{Address, U256};
use anyhow::{Context, Result};
use clap::Parser;

use crate::config::consts::{DEFAULT_LOOKBACK_BLOCKS, MIN_ETH_BALANCE, STATE_FILE_KEEPER};
use crate::state::StateFile;
use crate::wallet::{check_balance, generate_wallet, load_wallet};
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
    pub l2_rpc_url: Option<String>,

    /// L1 RPC endpoint (will be converted to WebSocket)
    #[arg(long, env = "L1_RPC_URL")]
    pub l1_rpc_url: Option<String>,

    /// L2 HeartbeatManager contract address
    #[arg(long, env = "L2_HEARTBEAT_MANAGER_ADDRESS")]
    pub l2_heartbeat_manager_address: Option<String>,

    /// L2 JailingPolicy contract address (optional)
    #[arg(long, env = "L2_JAILING_POLICY_ADDRESS")]
    pub l2_jailing_policy_address: Option<String>,

    /// Disable all jailing actions even if a JailingPolicy address is configured
    #[arg(long, env = "DISABLE_JAILING")]
    pub disable_jailing: Option<bool>,

    /// L1 EmissionsController contract address
    #[arg(long, env = "L1_EMISSIONS_CONTROLLER_ADDRESS")]
    pub l1_emissions_controller_address: Option<String>,

    /// Private key for contract interactions
    #[arg(long, env = "PRIVATE_KEY")]
    pub private_key: Option<String>,

    /// ETH value (wei) to forward for L1 -> L2 bridge messages
    #[arg(long, env = "L1_BRIDGE_VALUE_WEI")]
    pub l1_bridge_value_wei: Option<String>,

    /// Lookback blocks for historical event queries
    #[arg(long, env = "LOOKBACK_BLOCKS")]
    pub lookback_blocks: Option<u64>,

    /// Keeper tick interval in seconds (L2 rounds/rewards/jailing)
    #[arg(long, env = "TICK_INTERVAL_SECS")]
    pub tick_interval_secs: Option<u64>,

    /// Emissions check interval in seconds (L1)
    #[arg(long, env = "EMISSIONS_INTERVAL_SECS")]
    pub emissions_interval_secs: Option<u64>,
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
        let state_file = StateFile::new(STATE_FILE_KEEPER);

        let l2_rpc_url = cli_args
            .l2_rpc_url
            .or_else(|| state_file.load_value("L2_RPC_URL"))
            .unwrap_or_else(|| "TODO".to_string());

        let l1_rpc_url = cli_args
            .l1_rpc_url
            .or_else(|| state_file.load_value("L1_RPC_URL"))
            .unwrap_or_else(|| "TODO".to_string());

        let l2_heartbeat_manager_address_str = cli_args
            .l2_heartbeat_manager_address
            .or_else(|| state_file.load_value("L2_HEARTBEAT_MANAGER_ADDRESS"))
            .context("Missing L2_HEARTBEAT_MANAGER_ADDRESS")?;

        let l1_emissions_controller_address_str = cli_args
            .l1_emissions_controller_address
            .or_else(|| state_file.load_value("L1_EMISSIONS_CONTROLLER_ADDRESS"))
            .context("Missing L1_EMISSIONS_CONTROLLER_ADDRESS")?;

        let l2_jailing_policy_address_str = cli_args
            .l2_jailing_policy_address
            .or_else(|| state_file.load_value("L2_JAILING_POLICY_ADDRESS"))
            .and_then(|addr| {
                let trimmed = addr.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            });

        let disable_jailing = cli_args
            .disable_jailing
            .or_else(|| {
                state_file
                    .load_value("DISABLE_JAILING")
                    .and_then(|v| v.parse::<bool>().ok())
            })
            .unwrap_or(false);

        let mut wallet_was_created = false;
        let private_key = match cli_args
            .private_key
            .or_else(|| state_file.load_value("PRIVATE_KEY"))
        {
            Some(pk) => pk,
            None => {
                info!("No private key found. Generating new wallet...");
                let wallet = generate_wallet()?;
                let private_key = format!("0x{}", hex::encode(wallet.to_bytes()));
                let public_key = format!("{:?}", wallet.address());

                let mut state = std::collections::HashMap::new();
                state.insert("PRIVATE_KEY".to_string(), private_key.clone());
                state.insert("PUBLIC_KEY".to_string(), public_key.clone());
                state.insert("L2_RPC_URL".to_string(), l2_rpc_url.clone());
                state.insert("L1_RPC_URL".to_string(), l1_rpc_url.clone());
                state.insert(
                    "L2_HEARTBEAT_MANAGER_ADDRESS".to_string(),
                    l2_heartbeat_manager_address_str.clone(),
                );
                state.insert(
                    "L1_EMISSIONS_CONTROLLER_ADDRESS".to_string(),
                    l1_emissions_controller_address_str.clone(),
                );
                if let Some(ref jailing_addr) = l2_jailing_policy_address_str {
                    state.insert(
                        "L2_JAILING_POLICY_ADDRESS".to_string(),
                        jailing_addr.clone(),
                    );
                }
                state_file.save_all(&state)?;

                info!("New wallet generated and saved to {}", STATE_FILE_KEEPER);
                info!("Address: {}", public_key);
                wallet_was_created = true;

                private_key
            }
        };

        let l2_heartbeat_manager_address = l2_heartbeat_manager_address_str.parse::<Address>()?;
        let l1_emissions_controller_address =
            l1_emissions_controller_address_str.parse::<Address>()?;
        let l2_jailing_policy_address = if disable_jailing {
            None
        } else {
            match l2_jailing_policy_address_str {
                Some(addr) => Some(addr.parse::<Address>()?),
                None => None,
            }
        };

        let l1_bridge_value = cli_args
            .l1_bridge_value_wei
            .or_else(|| state_file.load_value("L1_BRIDGE_VALUE_WEI"))
            .unwrap_or_else(|| "0".to_string())
            .parse::<U256>()
            .context("Invalid L1_BRIDGE_VALUE_WEI")?;

        let lookback_blocks = cli_args
            .lookback_blocks
            .or_else(|| {
                state_file
                    .load_value("LOOKBACK_BLOCKS")
                    .and_then(|v| v.parse::<u64>().ok())
            })
            .unwrap_or(DEFAULT_LOOKBACK_BLOCKS);

        let tick_interval_secs = cli_args
            .tick_interval_secs
            .or_else(|| {
                state_file
                    .load_value("TICK_INTERVAL_SECS")
                    .and_then(|v| v.parse::<u64>().ok())
            })
            .unwrap_or(5);

        let emissions_interval_secs = cli_args
            .emissions_interval_secs
            .or_else(|| {
                state_file
                    .load_value("EMISSIONS_INTERVAL_SECS")
                    .and_then(|v| v.parse::<u64>().ok())
            })
            .unwrap_or(30);

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

        if wallet_was_created {
            anyhow::bail!(
                "Account created successfully. Please fund the address with at least {} ETH on both L1 and L2 to continue.",
                alloy::primitives::utils::format_ether(MIN_ETH_BALANCE)
            );
        }

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
