use anyhow::Result;
use clap::Parser;
use ethers::core::types::Address;

use crate::config::consts::{
    DEFAULT_HTXS_PATH, DEFAULT_ROUTER_CONTRACT_ADDRESS, DEFAULT_RPC_URL, DEFAULT_SLOT_MS,
    DEFAULT_STAKING_CONTRACT_ADDRESS, DEFAULT_TOKEN_CONTRACT_ADDRESS, STATE_FILE_SIMULATOR,
};
use crate::state::StateFile;
use tracing::info;

/// CLI arguments for the NilCC simulator
#[derive(Parser, Debug)]
#[command(name = "nilcc_simulator")]
#[command(about = "NilAV Server - Submits HTXs to the smart contract", long_about = None)]
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

    /// Private key for signing transactions
    #[arg(long, env = "PRIVATE_KEY")]
    pub private_key: Option<String>,

    /// Path to config file
    #[arg(long, env = "CONFIG_PATH")]
    pub config_path: Option<String>,

    /// Path to HTXs JSON file
    #[arg(long, env = "HTXS_PATH")]
    pub htxs_path: Option<String>,
}

/// Simulator configuration with all required values resolved
#[derive(Debug, Clone)]
pub struct SimulatorConfig {
    pub rpc_url: String,
    pub router_contract_address: Address,
    pub staking_contract_address: Address,
    pub token_contract_address: Address,
    pub private_key: String,
    pub htxs_path: String,
    pub slot_ms: u64,
}

impl SimulatorConfig {
    /// Load configuration with priority: CLI/env -> state file -> defaults
    pub fn load(cli_args: CliArgs) -> Result<Self> {
        let state_file = StateFile::new(STATE_FILE_SIMULATOR);

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

        // Load private key with priority (different default than node)
        let private_key = cli_args
            .private_key
            .or_else(|| state_file.load_value("PRIVATE_KEY"))
            .unwrap_or_else(|| {
                "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a".to_string()
            });

        // Load HTXs path with priority
        let htxs_path = cli_args
            .htxs_path
            .or_else(|| state_file.load_value("HTXS_PATH"))
            .unwrap_or_else(|| DEFAULT_HTXS_PATH.to_string());

        // Parse contract addresses
        let router_contract_address = router_contract_address_str.parse::<Address>()?;
        let staking_contract_address = staking_contract_address_str.parse::<Address>()?;
        let token_contract_address = token_contract_address_str.parse::<Address>()?;

        info!(
            "Loaded SimulatorConfig: rpc_url={}, router_contract_address={}, htxs_path={}",
            rpc_url, router_contract_address, htxs_path
        );
        Ok(SimulatorConfig {
            rpc_url,
            router_contract_address,
            staking_contract_address,
            token_contract_address,
            private_key,
            htxs_path,
            slot_ms: DEFAULT_SLOT_MS,
        })
    }
}
