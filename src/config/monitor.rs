use anyhow::Result;
use clap::Parser;
use ethers::core::types::Address;

use crate::config::consts::{
    DEFAULT_ROUTER_CONTRACT_ADDRESS, DEFAULT_RPC_URL, DEFAULT_STAKING_CONTRACT_ADDRESS,
    DEFAULT_TOKEN_CONTRACT_ADDRESS, STATE_FILE_MONITOR,
};
use crate::state::StateFile;
use tracing::info;

/// CLI arguments for the NilAV monitor
#[derive(Parser, Debug)]
#[command(name = "monitor")]
#[command(about = "NilAV Contract Monitor - Interactive TUI", long_about = None)]
pub struct CliArgs {
    /// Ethereum RPC endpoint
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

    /// Load all historical HTX events
    #[arg(long, env = "ALL_HTXS")]
    pub all_htxs: Option<bool>,
}

/// Monitor configuration with all required values resolved
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    pub rpc_url: String,
    pub router_contract_address: Address,
    pub staking_contract_address: Address,
    pub token_contract_address: Address,
    pub private_key: String,
    pub all_htxs: bool,
}

impl MonitorConfig {
    /// Load configuration with priority: CLI/env -> state file -> defaults
    pub fn load(cli_args: CliArgs) -> Result<Self> {
        let state_file = StateFile::new(STATE_FILE_MONITOR);

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

        // Load private key with priority (monitor uses first Hardhat account)
        let private_key = cli_args
            .private_key
            .or_else(|| state_file.load_value("PRIVATE_KEY"))
            .unwrap_or_else(|| {
                "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string()
            });

        // Load all_htxs flag with priority
        let all_htxs = cli_args
            .all_htxs
            .or_else(|| {
                state_file
                    .load_value("ALL_HTXS")
                    .and_then(|s| s.parse::<bool>().ok())
            })
            .unwrap_or(false);

        // Parse contract addresses
        let router_contract_address = router_contract_address_str.parse::<Address>()?;
        let staking_contract_address = staking_contract_address_str.parse::<Address>()?;
        let token_contract_address = token_contract_address_str.parse::<Address>()?;

        info!(
            "Loaded MonitorConfig: rpc_url={}, router_contract_address={}, all_htxs={}",
            rpc_url, router_contract_address, all_htxs
        );
        Ok(MonitorConfig {
            rpc_url,
            router_contract_address,
            staking_contract_address,
            token_contract_address,
            private_key,
            all_htxs,
        })
    }
}
