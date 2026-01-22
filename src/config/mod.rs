pub mod consts;
pub mod monitor;
pub mod node;
pub mod simulator;

// Re-export for convenience
pub use consts::*;
pub use monitor::{CliArgs as MonitorCliArgs, MonitorConfig};
pub use node::{validate_node_requirements, CliArgs as NodeCliArgs, NodeConfig};
pub use simulator::{CliArgs as SimulatorCliArgs, SimulatorConfig};

use alloy::primitives::Address;
use anyhow::anyhow;
use clap::Args;

use crate::state::StateFile;

#[derive(Args, Debug)]
pub struct ChainArgs {
    /// Ethereum RPC endpoint
    #[arg(long, env = "RPC_URL")]
    pub rpc_url: Option<String>,

    /// Heartbeat manager contract address
    #[arg(long, env = "MANAGER_CONTRACT_ADDRESS")]
    pub manager_contract_address: Option<String>,

    /// blacklight staking contract address
    #[arg(long, env = "STAKING_CONTRACT_ADDRESS")]
    pub staking_contract_address: Option<String>,

    /// NIL token contract address
    #[arg(long, env = "TOKEN_CONTRACT_ADDRESS")]
    pub token_contract_address: Option<String>,
}

pub(crate) struct ChainConfig {
    pub(crate) rpc_url: String,
    pub(crate) manager_contract_address: Address,
    pub(crate) staking_contract_address: Address,
    pub(crate) token_contract_address: Address,
}

impl ChainConfig {
    pub(crate) fn new(args: ChainArgs, state_file: &StateFile) -> anyhow::Result<Self> {
        // Load RPC URL with priority
        let rpc_url = args
            .rpc_url
            .or_else(|| state_file.load_value("RPC_URL"))
            .ok_or_else(|| anyhow!("no RPC url provided"))?;

        // Load contract addresses with priority
        let manager_contract_address = args
            .manager_contract_address
            .or_else(|| state_file.load_value("MANAGER_CONTRACT_ADDRESS"))
            .ok_or_else(|| anyhow!("no manager contract address provided"))?
            .parse()?;

        let staking_contract_address = args
            .staking_contract_address
            .or_else(|| state_file.load_value("STAKING_CONTRACT_ADDRESS"))
            .ok_or_else(|| anyhow!("no staking contract address provided"))?
            .parse()?;

        let token_contract_address = args
            .token_contract_address
            .or_else(|| state_file.load_value("TOKEN_CONTRACT_ADDRESS"))
            .ok_or_else(|| anyhow!("no token contract address provided"))?
            .parse()?;
        Ok(Self {
            rpc_url,
            manager_contract_address,
            staking_contract_address,
            token_contract_address,
        })
    }
}
