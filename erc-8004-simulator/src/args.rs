use alloy::primitives::Address;
use anyhow::Result;
use clap::Parser;

use state_file::StateFile;
use tracing::info;

const STATE_FILE_SIMULATOR: &str = "erc_8004_simulator.env";

/// Default slot interval in milliseconds - how often simulator submits validation requests
#[cfg(debug_assertions)]
const DEFAULT_SLOT_MS: u64 = 3000; // 3 seconds for debug (faster testing)

#[cfg(not(debug_assertions))]
const DEFAULT_SLOT_MS: u64 = 5000; // 5 seconds for release

/// CLI arguments for the ERC-8004 simulator
#[derive(Parser, Debug)]
#[command(name = "erc_8004_simulator")]
#[command(about = "ERC-8004 Simulator - Registers agents and submits validation requests", long_about = None)]
pub struct CliArgs {
    /// RPC URL for the Ethereum node
    #[arg(long, env = "RPC_URL")]
    pub rpc_url: Option<String>,

    /// Address of the IdentityRegistry contract
    #[arg(long, env = "IDENTITY_REGISTRY_CONTRACT_ADDRESS")]
    pub identity_registry_contract_address: Option<String>,

    /// Address of the ValidationRegistry contract
    #[arg(long, env = "VALIDATION_REGISTRY_CONTRACT_ADDRESS")]
    pub validation_registry_contract_address: Option<String>,

    /// Private key for signing transactions
    #[arg(long, env = "PRIVATE_KEY")]
    pub private_key: Option<String>,

    /// Agent URI to register with
    #[arg(long, env = "AGENT_URI")]
    pub agent_uri: Option<String>,

    /// HeartbeatManager contract address to submit validation requests to
    #[arg(long, env = "HEARTBEAT_MANAGER_ADDRESS")]
    pub heartbeat_manager_address: Option<String>,
}

/// Simulator configuration with all required values resolved
#[derive(Debug, Clone)]
pub struct SimulatorConfig {
    pub rpc_url: String,
    pub identity_registry_contract_address: Address,
    pub validation_registry_contract_address: Address,
    pub private_key: String,
    pub agent_uri: String,
    pub heartbeat_manager_address: Address,
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
            .unwrap_or_else(|| "http://127.0.0.1:8545".to_string());

        // Load IdentityRegistry contract address
        let identity_registry_contract_address = cli_args
            .identity_registry_contract_address
            .or_else(|| state_file.load_value("IDENTITY_REGISTRY_CONTRACT_ADDRESS"))
            .unwrap_or_else(|| "0x5FbDB2315678afecb367f032d93F642f64180aa3".to_string())
            .parse::<Address>()?;

        // Load ValidationRegistry contract address
        let validation_registry_contract_address = cli_args
            .validation_registry_contract_address
            .or_else(|| state_file.load_value("VALIDATION_REGISTRY_CONTRACT_ADDRESS"))
            .unwrap_or_else(|| "0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512".to_string())
            .parse::<Address>()?;

        // Load private key with priority (Anvil account #3 as default)
        let private_key = cli_args
            .private_key
            .or_else(|| state_file.load_value("PRIVATE_KEY"))
            .unwrap_or_else(|| {
                "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a".to_string()
            });

        // Load agent URI
        let agent_uri = cli_args
            .agent_uri
            .or_else(|| state_file.load_value("AGENT_URI"))
            .unwrap_or_else(|| "https://example.com/agent".to_string());

        // Load HeartbeatManager contract address
        let heartbeat_manager_address = cli_args
            .heartbeat_manager_address
            .or_else(|| state_file.load_value("HEARTBEAT_MANAGER_ADDRESS"))
            .unwrap_or_else(|| "0x5FC8d32690cc91D4c39d9d3abcBD16989F875707".to_string())
            .parse::<Address>()?;

        info!(
            "Loaded SimulatorConfig: rpc_url={rpc_url}, identity_registry={identity_registry_contract_address}, validation_registry={validation_registry_contract_address}"
        );

        Ok(SimulatorConfig {
            rpc_url,
            identity_registry_contract_address,
            validation_registry_contract_address,
            private_key,
            agent_uri,
            heartbeat_manager_address,
            slot_ms: DEFAULT_SLOT_MS,
        })
    }
}
