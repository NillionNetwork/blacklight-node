use alloy::primitives::{Address, B256, U256, keccak256};
use anyhow::Result;
use clap::Args;
use erc_8004_contract_clients::{ContractConfig, Erc8004Client};
use state_file::StateFile;
use std::sync::Arc;
use tracing::{info, warn};

use crate::common::{DEFAULT_SLOT_MS, Simulator, retry_submit};

const STATE_FILE_SIMULATOR: &str = "erc_8004_simulator.env";

#[derive(Args, Debug)]
#[command(about = "Register agents and submit ERC-8004 validation requests")]
pub struct Erc8004Args {
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

#[derive(Debug)]
pub struct Erc8004Config {
    pub rpc_url: String,
    pub identity_registry_contract_address: Address,
    pub validation_registry_contract_address: Address,
    pub private_key: String,
    pub agent_uri: String,
    pub heartbeat_manager_address: Address,
    pub slot_ms: u64,
}

impl Erc8004Config {
    pub fn load(args: Erc8004Args) -> Result<Self> {
        let state_file = StateFile::new(STATE_FILE_SIMULATOR);

        let rpc_url = args
            .rpc_url
            .or_else(|| state_file.load_value("RPC_URL"))
            .unwrap_or_else(|| "http://127.0.0.1:8545".to_string());

        let identity_registry_contract_address = args
            .identity_registry_contract_address
            .or_else(|| state_file.load_value("IDENTITY_REGISTRY_CONTRACT_ADDRESS"))
            .unwrap_or_else(|| "0x5FbDB2315678afecb367f032d93F642f64180aa3".to_string())
            .parse::<Address>()?;

        let validation_registry_contract_address = args
            .validation_registry_contract_address
            .or_else(|| state_file.load_value("VALIDATION_REGISTRY_CONTRACT_ADDRESS"))
            .unwrap_or_else(|| "0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512".to_string())
            .parse::<Address>()?;

        let private_key = args
            .private_key
            .or_else(|| state_file.load_value("PRIVATE_KEY"))
            .unwrap_or_else(|| {
                "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a".to_string()
            });

        let agent_uri = args
            .agent_uri
            .or_else(|| state_file.load_value("AGENT_URI"))
            .unwrap_or_else(|| "https://example.com/agent".to_string());

        let heartbeat_manager_address = args
            .heartbeat_manager_address
            .or_else(|| state_file.load_value("HEARTBEAT_MANAGER_ADDRESS"))
            .unwrap_or_else(|| "0x5FC8d32690cc91D4c39d9d3abcBD16989F875707".to_string())
            .parse::<Address>()?;

        info!(
            "Loaded Erc8004Config: rpc_url={rpc_url}, identity_registry={identity_registry_contract_address}, validation_registry={validation_registry_contract_address}"
        );

        Ok(Self {
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

pub struct Erc8004Simulator {
    client: Arc<Erc8004Client>,
    config: Arc<Erc8004Config>,
    agent_id: U256,
}

async fn setup_client(config: &Erc8004Config) -> Result<Erc8004Client> {
    let contract_config = ContractConfig::new(
        config.rpc_url.clone(),
        config.identity_registry_contract_address,
        config.validation_registry_contract_address,
    );

    let client = Erc8004Client::new(contract_config, config.private_key.clone()).await?;
    info!(
        identity_registry = %client.identity_registry.address(),
        validation_registry = %client.validation_registry.address(),
        signer = %client.signer_address(),
        "Connected to contracts"
    );
    Ok(client)
}

async fn register_agent(client: &Erc8004Client, config: &Erc8004Config) -> Result<U256> {
    info!(agent_uri = %config.agent_uri, "Registering agent");

    let (tx_hash, agent_id) = client
        .identity_registry
        .register_with_uri_and_get_id(config.agent_uri.clone())
        .await?;

    info!(tx_hash = ?tx_hash, agent_id = %agent_id, "Agent registration transaction submitted");

    match client.identity_registry.get_agent(agent_id).await {
        Ok((owner, uri, wallet)) => {
            info!(
                agent_id = %agent_id,
                owner = %owner,
                uri = %uri,
                wallet = %wallet,
                "Agent registered successfully"
            );
        }
        Err(e) => {
            warn!(agent_id = %agent_id, error = %e, "Could not verify agent registration");
        }
    }

    Ok(agent_id)
}

impl Erc8004Simulator {
    async fn submit_validation_request(&self, slot: u64) -> Result<()> {
        let client = Arc::clone(&self.client);
        let config = Arc::clone(&self.config);
        let agent_id = self.agent_id;

        retry_submit(
            move |attempt| {
                let client = Arc::clone(&client);
                let config = Arc::clone(&config);
                async move {
                    let block_number = client.get_block_number().await?;
                    let snapshot_id = block_number.saturating_sub(1);

                    let request_uri = config.agent_uri.clone();
                    let hash_input = format!("{}:{}", request_uri, snapshot_id);
                    let request_hash = B256::from(keccak256(hash_input.as_bytes()));

                    if attempt == 0 {
                        info!(
                            slot,
                            agent_id = %agent_id,
                            heartbeat_manager = %config.heartbeat_manager_address,
                            snapshot_id = snapshot_id,
                            request_uri = %request_uri,
                            "Submitting validation request"
                        );
                    } else {
                        info!(slot, attempt, "Retrying validation request submission");
                    }

                    let tx_hash = client
                        .validation_registry
                        .validation_request(
                            config.heartbeat_manager_address,
                            agent_id,
                            request_uri,
                            request_hash,
                            snapshot_id,
                        )
                        .await?;

                    info!(slot, tx_hash = ?tx_hash, "Validation request submitted");
                    Ok(())
                }
            },
            move |attempt, error| {
                warn!(slot, attempt, error = %error, "Submission reverted, will retry");
            },
        )
        .await
    }
}

#[async_trait::async_trait]
impl Simulator for Erc8004Simulator {
    type Args = Erc8004Args;

    async fn build(args: Self::Args) -> Result<Self> {
        let config = Erc8004Config::load(args)?;
        info!(slot_ms = config.slot_ms, "Configuration loaded");

        let client = setup_client(&config).await?;
        let agent_id = register_agent(&client, &config).await?;

        Ok(Self {
            client: Arc::new(client),
            config: Arc::new(config),
            agent_id,
        })
    }

    fn slot_ms(&self) -> u64 {
        self.config.slot_ms
    }

    fn submission_error_message(&self) -> &'static str {
        "ERC-8004 validation request submission failed"
    }

    async fn on_tick(&self, slot: u64) -> Result<()> {
        self.submit_validation_request(slot).await
    }
}
