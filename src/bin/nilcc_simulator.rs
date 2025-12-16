use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use niluv::{
    config::{SimulatorCliArgs, SimulatorConfig},
    contract_client::{ContractConfig, NilUVClient},
    types::Htx,
};
use rand::Rng;
use tokio::time::interval;
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let config = load_config()?;
    let client = setup_client(&config).await?;
    let htxs = load_htxs(&config.htxs_path);

    run_submission_loop(client, htxs, config.slot_ms).await
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(fmt::layer().with_ansi(true))
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();
}

fn load_config() -> Result<SimulatorConfig> {
    let cli_args = SimulatorCliArgs::parse();
    let config = SimulatorConfig::load(cli_args)?;
    info!(slot_ms = config.slot_ms, "Configuration loaded");
    Ok(config)
}

async fn setup_client(config: &SimulatorConfig) -> Result<NilUVClient> {
    let contract_config = ContractConfig::new(
        config.rpc_url.clone(),
        config.router_contract_address,
        config.staking_contract_address,
        config.token_contract_address,
    );

    let client = NilUVClient::new(contract_config, config.private_key.clone()).await?;

    info!(
        contract = %client.router.address(),
        signer = %client.signer_address(),
        "Connected to contract"
    );

    Ok(client)
}

fn load_htxs(path: &str) -> Vec<Htx> {
    let htxs_json = std::fs::read_to_string(path).unwrap_or_else(|_| "[]".to_string());
    let htxs: Vec<Htx> = serde_json::from_str(&htxs_json).unwrap_or_default();

    if htxs.is_empty() {
        warn!(path = %path, "No HTXs loaded");
    } else {
        info!(count = htxs.len(), path = %path, "HTXs loaded");
    }

    htxs
}

async fn run_submission_loop(client: NilUVClient, htxs: Vec<Htx>, slot_ms: u64) -> Result<()> {
    let mut ticker = interval(Duration::from_millis(slot_ms));
    let mut slot = 0u64;
    let client = Arc::new(client);
    let htxs = Arc::new(htxs);

    loop {
        ticker.tick().await;
        slot += 1;

        // Spawn submission as a background task so it doesn't block the next slot
        let client = Arc::clone(&client);
        let htxs = Arc::clone(&htxs);
        tokio::spawn(async move {
            if let Err(e) = submit_next_htx(&client, &htxs, slot).await {
                error!(slot, error = %e, "Submission failed");
            }
        });
    }
}

const MAX_RETRIES: u32 = 3;
const RETRY_DELAY_MS: u64 = 500;

async fn submit_next_htx(client: &Arc<NilUVClient>, htxs: &Arc<Vec<Htx>>, slot: u64) -> Result<()> {
    if htxs.is_empty() {
        warn!(slot, "No HTXs available");
        return Ok(());
    }

    let node_count = client.router.node_count().await?;
    if node_count.is_zero() {
        warn!(slot, "No nodes registered");
        return Ok(());
    }

    let mut last_error = None;

    for attempt in 0..MAX_RETRIES {
        // Randomly select an HTX and make it unique by appending a random nonce to workload_id
        // This prevents "HTX already exists" errors when multiple submissions land in the same block
        // Scope rng to drop it before await (ThreadRng is not Send)
        let htx = {
            let mut rng = rand::rng();
            let idx = rng.random_range(0..htxs.len());
            let nonce: u128 = rng.random_range(0..u128::MAX); // 128-bit random number
            let mut htx = htxs[idx].clone();
            htx.workload_id.current = format!("{}-{:x}", htx.workload_id.current, nonce);
            htx
        };

        if attempt == 0 {
            info!(slot, node_count = %node_count, "Submitting HTX");
        } else {
            info!(slot, attempt, "Retrying HTX submission");
        }

        match client.router.submit_htx(&htx.into()).await {
            Ok(tx_hash) => {
                info!(slot, tx_hash = ?tx_hash, "HTX submitted");
                return Ok(());
            }
            Err(e) => {
                let error_str = e.to_string();
                // Only retry on on-chain reverts (state race conditions)
                if error_str.contains("reverted on-chain") {
                    warn!(slot, attempt, error = %e, "Submission reverted, will retry");
                    last_error = Some(e);
                    tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS)).await;
                    continue;
                }
                // For other errors (simulation failures, etc.), fail immediately
                return Err(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Max retries exceeded")))
}
