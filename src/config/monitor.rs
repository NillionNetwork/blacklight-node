use anyhow::Result;
use clap::Parser;
use ethers::core::types::Address;

use crate::state::StateFile;
use tracing::info;

const STATE_FILE: &str = "nilav_monitor.env";

/// CLI arguments for the NilAV monitor
#[derive(Parser, Debug)]
#[command(name = "monitor")]
#[command(about = "NilAV Contract Monitor - Interactive TUI", long_about = None)]
pub struct CliArgs {
    /// Ethereum RPC endpoint
    #[arg(long, env = "RPC_URL")]
    pub rpc_url: Option<String>,

    /// NilAV contract address
    #[arg(long, env = "CONTRACT_ADDRESS")]
    pub contract_address: Option<String>,

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
    pub contract_address: Address,
    pub private_key: String,
    pub all_htxs: bool,
}

impl MonitorConfig {
    /// Load configuration with priority: CLI/env -> state file -> defaults
    pub fn load(cli_args: CliArgs) -> Result<Self> {
        let state_file = StateFile::new(STATE_FILE);

        // Load RPC URL with priority
        let rpc_url = cli_args
            .rpc_url
            .or_else(|| state_file.load_value("RPC_URL"))
            .unwrap_or_else(|| "http://localhost:8545".to_string());

        // Load contract address with priority
        let contract_address_str = cli_args
            .contract_address
            .or_else(|| state_file.load_value("CONTRACT_ADDRESS"))
            .unwrap_or_else(|| "0x5FbDB2315678afecb367f032d93F642f64180aa3".to_string());

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

        // Parse contract address
        let contract_address = contract_address_str.parse::<Address>()?;

        info!(
            "Loaded MonitorConfig: rpc_url={}, contract_address={}, all_htxs={}",
            rpc_url, contract_address, all_htxs
        );
        Ok(MonitorConfig {
            rpc_url,
            contract_address,
            private_key,
            all_htxs,
        })
    }
}
