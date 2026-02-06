use alloy::primitives::Address;
use anyhow::Result;
use blacklight_contract_clients::{
    BlacklightClient, ContractConfig,
    htx::{Htx, JsonHtx, NillionHtx, PhalaHtx},
};
use chain_args::{ChainArgs, ChainConfig};
use clap::Args;
use rand::Rng;
use state_file::StateFile;
use std::sync::Arc;
use tracing::{info, warn};

use crate::common::{DEFAULT_SLOT_MS, Simulator, retry_submit};

const STATE_FILE_SIMULATOR: &str = "nilcc_simulator.env";

/// Default path to HTXs JSON file
const DEFAULT_HTXS_PATH: &str = "data/htxs.json";

#[derive(Args, Debug)]
#[command(about = "Submit HTXs to the HeartbeatManager contract")]
pub struct NilccArgs {
    #[command(flatten)]
    pub chain_args: ChainArgs,

    /// Private key for signing transactions
    #[arg(long, env = "PRIVATE_KEY")]
    pub private_key: Option<String>,

    /// Path to HTXs JSON file
    #[arg(long, env = "HTXS_PATH")]
    pub htxs_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NilccConfig {
    pub rpc_url: String,
    pub manager_contract_address: Address,
    pub staking_contract_address: Address,
    pub token_contract_address: Address,
    pub private_key: String,
    pub htxs_path: String,
    pub slot_ms: u64,
}

pub struct NilccSimulator {
    client: Arc<BlacklightClient>,
    htxs: Arc<Vec<Htx>>,
    slot_ms: u64,
}

impl NilccConfig {
    pub fn load(args: NilccArgs) -> Result<Self> {
        let state_file = StateFile::new(STATE_FILE_SIMULATOR);
        let ChainConfig {
            rpc_url,
            manager_contract_address,
            staking_contract_address,
            token_contract_address,
        } = ChainConfig::new(args.chain_args, &state_file)?;

        let private_key = args
            .private_key
            .or_else(|| state_file.load_value("PRIVATE_KEY"))
            .unwrap_or_else(|| {
                "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a".to_string()
            });

        let htxs_path = args
            .htxs_path
            .or_else(|| state_file.load_value("HTXS_PATH"))
            .unwrap_or_else(|| DEFAULT_HTXS_PATH.to_string());

        info!(
            "Loaded NilccConfig: rpc_url={rpc_url}, manager_contract_address={manager_contract_address}, htxs_path={htxs_path}"
        );
        Ok(Self {
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

fn load_htxs(path: &str) -> Vec<Htx> {
    let htxs_json = std::fs::read_to_string(path).unwrap_or_else(|_| "[]".to_string());
    let json_htxs: Vec<JsonHtx> = serde_json::from_str(&htxs_json).unwrap_or_default();
    let htxs: Vec<Htx> = json_htxs.into_iter().map(Htx::from).collect();

    if htxs.is_empty() {
        warn!(path = %path, "No HTXs loaded");
    } else {
        info!(count = htxs.len(), path = %path, "HTXs loaded");
    }

    htxs
}

impl NilccSimulator {
    async fn submit_next_htx(&self, slot: u64) -> Result<()> {
        let client = Arc::clone(&self.client);
        let htxs = Arc::clone(&self.htxs);

        if htxs.is_empty() {
            warn!(slot, "No HTXs available");
            return Ok(());
        }

        let node_count = client.manager.node_count().await?;
        if node_count.is_zero() {
            warn!(slot, "No nodes registered");
            return Ok(());
        }

        retry_submit(
            move |attempt| {
                let client = Arc::clone(&client);
                let htxs = Arc::clone(&htxs);
                async move {
                    let htx = {
                        let mut rng = rand::rng();
                        let idx = rng.random_range(0..htxs.len());
                        let nonce: u128 = rng.random_range(0..u128::MAX);
                        let mut htx = htxs[idx].clone();
                        match &mut htx {
                            Htx::Nillion(NillionHtx::V1(htx)) => {
                                htx.workload_id.current =
                                    format!("{}-{:x}", htx.workload_id.current, nonce);
                            }
                            Htx::Phala(PhalaHtx::V1(htx)) => {
                                htx.app_compose = format!("{}-{:x}", htx.app_compose, nonce);
                            }
                            Htx::Erc8004(_) => {
                                unreachable!("ERC-8004 HTXs should not be loaded from JSON files")
                            }
                        }
                        htx
                    };

                    if attempt == 0 {
                        info!(slot, node_count = %node_count, "Submitting HTX");
                    } else {
                        info!(slot, attempt, "Retrying HTX submission");
                    }

                    let tx_hash = client.manager.submit_htx(&htx).await?;
                    info!(slot, tx_hash = ?tx_hash, "HTX submitted");
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
impl Simulator for NilccSimulator {
    type Args = NilccArgs;

    async fn build(args: Self::Args) -> Result<Self> {
        let config = NilccConfig::load(args)?;
        info!(slot_ms = config.slot_ms, "Configuration loaded");

        let contract_config = ContractConfig::new(
            config.rpc_url.clone(),
            config.manager_contract_address,
            config.staking_contract_address,
            config.token_contract_address,
        );

        let client = BlacklightClient::new(contract_config, config.private_key.clone()).await?;
        info!(
            contract = %client.manager.address(),
            signer = %client.signer_address(),
            "Connected to contract"
        );

        let htxs = load_htxs(&config.htxs_path);

        Ok(Self {
            client: Arc::new(client),
            htxs: Arc::new(htxs),
            slot_ms: config.slot_ms,
        })
    }

    fn slot_ms(&self) -> u64 {
        self.slot_ms
    }

    fn submission_error_message(&self) -> &'static str {
        "NilCC submission failed"
    }

    async fn on_tick(&self, slot: u64) -> Result<()> {
        self.submit_next_htx(slot).await
    }
}
