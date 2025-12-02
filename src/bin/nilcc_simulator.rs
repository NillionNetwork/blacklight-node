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
    // Initialize tracing
    tracing_subscriber::registry()
        .with(fmt::layer().with_ansi(true))
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    // Load configuration
    let cli_args = SimulatorCliArgs::parse();
    let config = SimulatorConfig::load(cli_args)?;

    info!(slot_ms = config.slot_ms, "Loaded configuration");

    // Setup smart contract client
    let contract_config = ContractConfig::new(
        config.rpc_url.clone(),
        config.router_contract_address,
        config.staking_contract_address,
        config.token_contract_address,
    );
    let client = NilAVClient::new(contract_config, config.private_key.clone()).await?;

    info!(
        contract_address = %client.router.address(),
        signer_address = %client.signer_address(),
        "Connected to smart contract"
    );

    // Load HTXs from file
    let htxs_str = std::fs::read_to_string(&config.htxs_path).unwrap_or_else(|_| "[]".to_string());
    let htxs: Vec<Htx> = serde_json::from_str(&htxs_str).unwrap_or_else(|_| Vec::new());

    if htxs.is_empty() {
        warn!(htxs_path = %config.htxs_path, "No HTXs loaded from file");
    } else {
        info!(count = htxs.len(), htxs_path = %config.htxs_path, "Loaded HTXs from file");
    }

    // Slot ticker - submits HTXs to the contract
    let mut ticker = interval(Duration::from_millis(config.slot_ms));
    let mut slot: u64 = 0;

    loop {
        ticker.tick().await;
        slot += 1;

        // Pick HTX round-robin from file
        if htxs.is_empty() {
            warn!(slot = slot, "No HTXs to submit");
            continue;
        }

        let idx = ((slot - 1) as usize) % htxs.len();
        let htx = &htxs[idx];

        // Check how many nodes are registered
        let node_count = client.router.node_count().await?;
        if node_count.is_zero() {
            warn!(slot = slot, "No nodes registered, skipping HTX submission");
            continue;
        }

        info!(
            slot = slot,
            node_count = %node_count,
            "Submitting HTX to contract"
        );

        // Submit HTX to contract - the contract will handle assignment
        match client.router.submit_htx(htx).await {
            Ok((tx_hash, htx_id)) => {
                info!(
                    slot = slot,
                    tx_hash = ?tx_hash,
                    htx_id = ?htx_id,
                    "HTX submitted successfully"
                );
            }
            Err(e) => {
                error!(
                    slot = slot,
                    error = %e,
                    "Failed to submit HTX"
                );
            }
        }
    }
}
