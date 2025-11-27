use anyhow::Result;
use clap::Parser;
use ethers::core::types::Address;

use crate::state::StateFile;

const STATE_FILE: &str = "nilav_node.env";

/// CLI arguments for the NilAV node
#[derive(Parser, Debug)]
#[command(name = "nilav_node")]
#[command(about = "NilAV Node - Real-time HTX verification using WebSocket streaming", long_about = None)]
pub struct CliArgs {
    /// Ethereum RPC endpoint (will be converted to WebSocket)
    #[arg(long, env = "RPC_URL")]
    pub rpc_url: Option<String>,

    /// NilAV contract address
    #[arg(long, env = "CONTRACT_ADDRESS")]
    pub contract_address: Option<String>,

    /// Private key for contract interactions
    #[arg(long, env = "PRIVATE_KEY")]
    pub private_key: Option<String>,
}

/// Node configuration with all required values resolved
#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub rpc_url: String,
    pub contract_address: Address,
    pub private_key: String,
}

impl NodeConfig {
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

        // Load private key with priority
        let private_key = cli_args
            .private_key
            .or_else(|| state_file.load_value("PRIVATE_KEY"))
            .unwrap_or_else(|| {
                "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d".to_string()
            });

        // Parse contract address
        let contract_address = contract_address_str.parse::<Address>()?;

        Ok(NodeConfig {
            rpc_url,
            contract_address,
            private_key,
        })
    }
}
