use alloy::primitives::Address;
use anyhow::Result;
use clap::Parser;

use crate::config::consts::STATE_FILE_MONITOR;
use crate::config::ChainArgs;
use crate::config::ChainConfig;
use crate::state::StateFile;
use tracing::info;

/// CLI arguments for the blacklight monitor
#[derive(Parser, Debug)]
#[command(name = "monitor")]
#[command(about = "Blacklight Contract Monitor - Interactive TUI", long_about = None)]
pub struct CliArgs {
    #[clap(flatten)]
    pub chain_args: ChainArgs,

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
        let ChainConfig {
            rpc_url,
            manager_contract_address,
            staking_contract_address,
            token_contract_address,
        } = ChainConfig::new(cli_args.chain_args, &state_file)?;

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
