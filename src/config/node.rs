use std::env::temp_dir;
use std::path::PathBuf;

use alloy::primitives::utils::format_ether;
use alloy::primitives::{Address, U256};
use anyhow::{Context, Result};
use clap::Parser;

use crate::config::consts::{
    DEFAULT_ROUTER_CONTRACT_ADDRESS, DEFAULT_RPC_URL, DEFAULT_STAKING_CONTRACT_ADDRESS,
    DEFAULT_TOKEN_CONTRACT_ADDRESS, MIN_ETH_BALANCE, STATE_FILE_NODE,
};
use crate::contract_client::NilAVClient;
use crate::state::StateFile;
use crate::wallet::{display_wallet_status, generate_wallet, WalletStatus};
use tracing::{error, info};

/// CLI arguments for the NilAV node
#[derive(Parser, Debug)]
#[command(name = "nilav_node")]
#[command(about = "NilAV Node - Real-time HTX verification using WebSocket streaming", long_about = None)]
pub struct CliArgs {
    /// Ethereum RPC endpoint (will be converted to WebSocket)
    #[arg(long, env = "RPC_URL")]
    pub rpc_url: Option<String>,

    /// NilAV router contract address
    #[arg(long, env = "ROUTER_CONTRACT_ADDRESS")]
    pub router_contract_address: Option<String>,

    /// NilAV staking contract address
    #[arg(long, env = "STAKING_CONTRACT_ADDRESS")]
    pub staking_contract_address: Option<String>,

    /// TEST token contract address
    #[arg(long, env = "TOKEN_CONTRACT_ADDRESS")]
    pub token_contract_address: Option<String>,

    /// Private key for contract interactions
    #[arg(long, env = "PRIVATE_KEY")]
    pub private_key: Option<String>,

    /// The path where nilcc artifacts will be cached.
    #[clap(short, long, default_value = default_artifact_cache_path().into_os_string(), env = "ARTIFACT_CACHE")]
    pub artifact_cache: PathBuf,

    /// The path where AMD certificates will be cached.
    #[clap(short, long, default_value = default_cert_cache_path().into_os_string(), env = "CERT_CACHE")]
    pub cert_cache: PathBuf,
}

/// Node configuration with all required values resolved
#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub rpc_url: String,
    pub router_contract_address: Address,
    pub staking_contract_address: Address,
    pub token_contract_address: Address,
    pub private_key: String,
    pub was_wallet_created: bool,
}

impl NodeConfig {
    /// Load configuration with priority: CLI/env -> state file -> defaults
    /// Generates a new wallet if none exists
    /// Returns (NodeConfig, was_wallet_created)
    pub async fn load(cli_args: CliArgs) -> Result<Self> {
        let state_file = StateFile::new(STATE_FILE_NODE);

        // Load RPC URL with priority
        let rpc_url = cli_args
            .rpc_url
            .or_else(|| state_file.load_value("RPC_URL"))
            .unwrap_or_else(|| DEFAULT_RPC_URL.to_string());

        // Load contract addresses with priority
        let router_contract_address_str = cli_args
            .router_contract_address
            .or_else(|| state_file.load_value("ROUTER_CONTRACT_ADDRESS"))
            .unwrap_or_else(|| DEFAULT_ROUTER_CONTRACT_ADDRESS.to_string());

        let staking_contract_address_str = cli_args
            .staking_contract_address
            .or_else(|| state_file.load_value("STAKING_CONTRACT_ADDRESS"))
            .unwrap_or_else(|| DEFAULT_STAKING_CONTRACT_ADDRESS.to_string());

        let token_contract_address_str = cli_args
            .token_contract_address
            .or_else(|| state_file.load_value("TOKEN_CONTRACT_ADDRESS"))
            .unwrap_or_else(|| DEFAULT_TOKEN_CONTRACT_ADDRESS.to_string());

        // Load or generate private key
        let mut was_wallet_created = false;
        let private_key = match cli_args
            .private_key
            .or_else(|| state_file.load_value("PRIVATE_KEY"))
        {
            Some(pk) => pk,
            None => {
                // Generate a new wallet
                info!("No private key found. Generating new wallet...");
                let wallet = generate_wallet()?;
                let private_key = format!("0x{}", hex::encode(wallet.to_bytes()));
                let public_key = format!("{:?}", wallet.address());

                // Save all values to state file using save_all
                let mut state = std::collections::HashMap::new();
                state.insert("PRIVATE_KEY".to_string(), private_key.clone());
                state.insert("PUBLIC_KEY".to_string(), public_key.clone());
                state.insert("RPC_URL".to_string(), rpc_url.clone());
                state.insert(
                    "ROUTER_CONTRACT_ADDRESS".to_string(),
                    router_contract_address_str.clone(),
                );
                state.insert(
                    "STAKING_CONTRACT_ADDRESS".to_string(),
                    staking_contract_address_str.clone(),
                );
                state.insert(
                    "TOKEN_CONTRACT_ADDRESS".to_string(),
                    token_contract_address_str.clone(),
                );
                state_file.save_all(&state).map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to save state file on path {}: {}",
                        STATE_FILE_NODE,
                        e
                    )
                })?;

                info!("New wallet generated and saved to {}", STATE_FILE_NODE);
                info!("Address: {}", public_key);
                was_wallet_created = true;

                private_key
            }
        };

        // Parse contract addresses
        let router_contract_address = router_contract_address_str.parse::<Address>()?;
        let staking_contract_address = staking_contract_address_str.parse::<Address>()?;
        let token_contract_address = token_contract_address_str.parse::<Address>()?;

        info!(
            "Loaded NodeConfig: rpc_url={}, router_contract_address={} staking_contract_address={} token_contract_address={}",
            rpc_url, router_contract_address, staking_contract_address, token_contract_address
        );
        Ok(NodeConfig {
            rpc_url,
            router_contract_address,
            staking_contract_address,
            token_contract_address,
            private_key,
            was_wallet_created,
        })
    }
}

/// Validates that the node has sufficient ETH balance and staked TEST tokens
/// Returns Ok(()) if ready, or Err if validation fails with user-friendly display
pub async fn validate_node_requirements(
    client: &NilAVClient,
    rpc_url: &str,
    was_wallet_created: bool,
) -> Result<()> {
    let address = client.signer_address();

    info!("Checking ETH balance for address: {:?}", address);
    let eth_balance = client
        .get_balance()
        .await
        .context("Failed to check ETH balance")?;

    info!(
        "Checking staked TEST token balance for address: {:?}",
        address
    );
    let staked_balance = client.staking.stake_of(address).await.unwrap_or_else(|e| {
        error!("Could not fetch staked balance: {}", e);
        U256::ZERO
    });

    // Determine wallet status and display unified banner
    let status = if was_wallet_created {
        WalletStatus::Created
    } else if eth_balance < MIN_ETH_BALANCE || staked_balance == U256::ZERO {
        WalletStatus::InsufficientFunds
    } else {
        WalletStatus::Ready
    };

    // Display wallet status with all information
    display_wallet_status(
        status,
        address,
        rpc_url,
        eth_balance,
        staked_balance,
        MIN_ETH_BALANCE,
    );

    // Return error if not ready
    match status {
        WalletStatus::Created => {
            anyhow::bail!(
                "Account created successfully. Please fund the address with at least {} ETH to continue.",
                format_ether(MIN_ETH_BALANCE)
            )
        }
        WalletStatus::InsufficientFunds => {
            anyhow::bail!(
                "Insufficient funds. Please ensure the address has at least {} ETH and try again.",
                format_ether(MIN_ETH_BALANCE)
            )
        }
        WalletStatus::Ready => Ok(()),
    }
}

fn default_cache_path() -> PathBuf {
    temp_dir().join("nilav-cache")
}

fn default_cert_cache_path() -> PathBuf {
    default_cache_path().join("certs")
}

fn default_artifact_cache_path() -> PathBuf {
    default_cache_path().join("artifacts")
}
