use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use nilav::{
    config::{SimulatorCliArgs, SimulatorConfig},
    contract_client::{ContractConfig, NilAVClient},
    types::Htx,
};
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

async fn setup_client(config: &SimulatorConfig) -> Result<NilAVClient> {
    let contract_config = ContractConfig::new(
        config.rpc_url.clone(),
        config.router_contract_address,
        config.staking_contract_address,
        config.token_contract_address,
    );
    
    let client = NilAVClient::new(contract_config, config.private_key.clone()).await?;
    
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

async fn run_submission_loop(client: NilAVClient, htxs: Vec<Htx>, slot_ms: u64) -> Result<()> {
    let mut ticker = interval(Duration::from_millis(slot_ms));
    let mut slot = 0u64;
    
    loop {
        ticker.tick().await;
        slot += 1;
        
        if let Err(e) = submit_next_htx(&client, &htxs, slot).await {
            error!(slot, error = %e, "Submission failed");
        }
    }
}

async fn submit_next_htx(client: &NilAVClient, htxs: &[Htx], slot: u64) -> Result<()> {
    if htxs.is_empty() {
        warn!(slot, "No HTXs available");
        return Ok(());
    }
    
    let node_count = client.router.node_count().await?;
    if node_count.is_zero() {
        warn!(slot, "No nodes registered");
        return Ok(());
    }
    
    let htx = &htxs[((slot - 1) as usize) % htxs.len()];
    
    info!(slot, node_count = %node_count, "Submitting HTX");
    
    let (tx_hash, htx_id) = client.router.submit_htx(htx).await?;
    
    info!(
        slot,
        tx_hash = ?tx_hash,
        htx_id = ?htx_id,
        "HTX submitted"
    );
    
    Ok(())
}
