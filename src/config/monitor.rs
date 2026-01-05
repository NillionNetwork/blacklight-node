use alloy::primitives::Address;
use anyhow::anyhow;
use anyhow::Result;
use clap::Parser;

use crate::config::consts::STATE_FILE_MONITOR;
use crate::state::StateFile;
use tracing::info;

/// CLI arguments for the NilUV monitor
#[derive(Parser, Debug)]
#[command(name = "monitor")]
#[command(about = "NilUV Contract Monitor - Interactive TUI", long_about = None)]
pub struct CliArgs {
    /// Ethereum RPC endpoint
    #[arg(long, env = "RPC_URL")]
    pub rpc_url: Option<String>,

    /// Heartbeat manager contract address
    #[arg(long, env = "MANAGER_CONTRACT_ADDRESS")]
    pub manager_contract_address: Option<String>,

    /// NilUV staking contract address
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
    pub manager_contract_address: Address,
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
            .ok_or_else(|| anyhow!("no RPC url provided"))?;

        // Load contract addresses with priority
        let manager_contract_address = cli_args
            .manager_contract_address
            .or_else(|| state_file.load_value("MANAGER_CONTRACT_ADDRESS"))
            .ok_or_else(|| anyhow!("no manager contract address provided"))?;

        let staking_contract_address = cli_args
            .staking_contract_address
            .or_else(|| state_file.load_value("STAKING_CONTRACT_ADDRESS"))
            .ok_or_else(|| anyhow!("no staking contract address provided"))?;

        let token_contract_address = cli_args
            .token_contract_address
            .or_else(|| state_file.load_value("TOKEN_CONTRACT_ADDRESS"))
            .ok_or_else(|| anyhow!("no token contract address provided"))?;

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
        let manager_contract_address = manager_contract_address.parse::<Address>()?;
        let staking_contract_address = staking_contract_address.parse::<Address>()?;
        let token_contract_address = token_contract_address.parse::<Address>()?;

        info!(
            "Loaded MonitorConfig: rpc_url={rpc_url}, manager_contract_address={manager_contract_address}, all_htxs={all_htxs}"
        );
        Ok(MonitorConfig {
            rpc_url,
            manager_contract_address,
            staking_contract_address,
            token_contract_address,
            private_key,
            all_htxs,
        })
    }
}
