use anyhow::{Context, Result};
use clap::Parser;
use ethers::core::types::Address;
use ethers::prelude::U256;
use ethers::signers::Signer;

use crate::config::consts::{
    DEFAULT_ROUTER_CONTRACT_ADDRESS, DEFAULT_RPC_URL, DEFAULT_STAKING_CONTRACT_ADDRESS,
    DEFAULT_TOKEN_CONTRACT_ADDRESS, STATE_FILE_NODE,
};
use crate::state::StateFile;
use crate::wallet::{
    check_balance, display_account_created_banner, display_insufficient_funds_banner,
    display_wallet_loaded_banner, generate_wallet, load_wallet,
};
use tracing::info;

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
}

/// Node configuration with all required values resolved
#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub rpc_url: String,
    pub router_contract_address: Address,
    pub staking_contract_address: Address,
    pub token_contract_address: Address,
    pub private_key: String,
}

enum WalletState {
    Created(Address),
    NeedsFunding(Address),
    Ready { address: Address, balance: U256 },
}

impl WalletState {
    fn ensure_ready(self, rpc_url: &str) -> Result<()> {
        match self {
            WalletState::Created(address) => {
                display_account_created_banner(address, rpc_url);
                anyhow::bail!(
                    "Account created successfully. Please fund the address with ETH to continue."
                );
            }
            WalletState::NeedsFunding(address) => {
                display_insufficient_funds_banner(address, rpc_url);
                anyhow::bail!("Insufficient funds. Please load ETH to the address and try again.");
            }
            WalletState::Ready { address, balance } => {
                display_wallet_loaded_banner(address, balance, rpc_url);
                Ok(())
            }
        }
    }
}

impl NodeConfig {
    /// Load configuration with priority: CLI/env -> state file -> defaults
    /// Generates a new wallet if none exists and checks balance before proceeding
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

        let mut wallet_was_created = false;

        // Load or generate private key
        let private_key = match cli_args
            .private_key
            .or_else(|| state_file.load_value("PRIVATE_KEY"))
        {
            Some(pk) => pk,
            None => {
                // Generate a new wallet
                info!("No private key found. Generating new wallet...");
                let wallet = generate_wallet()?;
                let private_key = format!("0x{}", hex::encode(wallet.signer().to_bytes()));
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
                state_file.save_all(&state)?;

                info!("New wallet generated and saved to {}", STATE_FILE_NODE);
                info!("Address: {}", public_key);
                wallet_was_created = true;

                private_key
            }
        };

        // Parse contract addresses
        let router_contract_address = router_contract_address_str.parse::<Address>()?;
        let staking_contract_address = staking_contract_address_str.parse::<Address>()?;
        let token_contract_address = token_contract_address_str.parse::<Address>()?;

        // Load wallet and check balance
        let wallet = load_wallet(&private_key)?;
        let address = wallet.address();

        info!("Checking balance for address: {:?}", address);
        let balance = check_balance(&rpc_url, address)
            .await
            .context("Failed to check balance")?;

        let wallet_state = if wallet_was_created {
            WalletState::Created(address)
        } else if balance == U256::zero() {
            WalletState::NeedsFunding(address)
        } else {
            WalletState::Ready { address, balance }
        };

        wallet_state.ensure_ready(&rpc_url)?;

        info!(
            "Loaded NodeConfig: rpc_url={}, router_contract_address={}",
            rpc_url, router_contract_address
        );
        Ok(NodeConfig {
            rpc_url,
            router_contract_address,
            staking_contract_address,
            token_contract_address,
            private_key,
        })
    }
}
