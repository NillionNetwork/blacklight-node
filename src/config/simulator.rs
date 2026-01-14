use alloy::primitives::Address;
use anyhow::Result;
use clap::Parser;

use crate::config::consts::{DEFAULT_HTXS_PATH, DEFAULT_SLOT_MS, STATE_FILE_SIMULATOR};
use crate::config::ChainArgs;
use crate::config::ChainConfig;
use crate::state::StateFile;
use tracing::info;

/// CLI arguments for the NilCC simulator
#[derive(Parser, Debug)]
#[command(name = "nilcc_simulator")]
#[command(about = "NilUV Server - Submits HTXs to the smart contract", long_about = None)]
pub struct CliArgs {
    #[clap(flatten)]
    pub chain_args: ChainArgs,

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
    pub manager_contract_address: Address,
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
        let ChainConfig {
            rpc_url,
            manager_contract_address,
            staking_contract_address,
            token_contract_address,
        } = ChainConfig::new(cli_args.chain_args, &state_file)?;

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

        info!(
            "Loaded SimulatorConfig: rpc_url={rpc_url}, manager_contract_address={manager_contract_address}, htxs_path={htxs_path}"
        );
        Ok(SimulatorConfig {
            rpc_url,
            manager_contract_address,
            staking_contract_address,
            token_contract_address,
            private_key,
            htxs_path,
            slot_ms: DEFAULT_SLOT_MS,
        })
    }
}
