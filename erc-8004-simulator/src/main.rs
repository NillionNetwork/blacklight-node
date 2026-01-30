use alloy::primitives::{B256, U256, keccak256};
use anyhow::Result;
use args::{CliArgs, SimulatorConfig};
use clap::Parser;
use erc_8004_contract_clients::{ContractConfig, Erc8004Client};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::{error, info, warn};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

mod args;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let config = load_config()?;
    let client = setup_client(&config).await?;

    // Register the agent first
    let agent_id = register_agent(&client, &config).await?;

    // Run the validation request submission loop
    run_submission_loop(client, config, agent_id).await
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(fmt::layer().with_ansi(true))
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();
}

fn load_config() -> Result<SimulatorConfig> {
    let cli_args = CliArgs::parse();
    let config = SimulatorConfig::load(cli_args)?;
    info!(slot_ms = config.slot_ms, "Configuration loaded");
    Ok(config)
}

async fn setup_client(config: &SimulatorConfig) -> Result<Erc8004Client> {
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

async fn register_agent(client: &Erc8004Client, config: &SimulatorConfig) -> Result<U256> {
    info!(agent_uri = %config.agent_uri, "Registering agent");

    let (tx_hash, agent_id) = client
        .identity_registry
        .register_with_uri_and_get_id(config.agent_uri.clone())
        .await?;

    info!(tx_hash = ?tx_hash, agent_id = %agent_id, "Agent registration transaction submitted");

    // Verify registration by querying the agent
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

async fn run_submission_loop(
    client: Erc8004Client,
    config: SimulatorConfig,
    agent_id: U256,
) -> Result<()> {
    let mut ticker = interval(Duration::from_millis(config.slot_ms));
    let mut slot = 0u64;
    let client = Arc::new(client);
    let config = Arc::new(config);

    loop {
        ticker.tick().await;
        slot += 1;

        // Spawn submission as a background task so it doesn't block the next slot
        let client = Arc::clone(&client);
        let config = Arc::clone(&config);
        tokio::spawn(async move {
            if let Err(e) = submit_validation_request(&client, &config, agent_id, slot).await {
                error!(slot, error = %e, "Validation request submission failed");
            }
        });
    }
}

const MAX_RETRIES: u32 = 3;
const RETRY_DELAY_MS: u64 = 500;

async fn submit_validation_request(
    client: &Arc<Erc8004Client>,
    config: &Arc<SimulatorConfig>,
    agent_id: U256,
    slot: u64,
) -> Result<()> {
    let mut last_error = None;

    for attempt in 0..MAX_RETRIES {
        // Get current block number for snapshot ID (use block - 1 for committee selection)
        let block_number = client.get_block_number().await?;
        let snapshot_id = block_number.saturating_sub(1);

        // Use same URI but include snapshot_id in hash to make each request unique
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

        match client
            .validation_registry
            .validation_request(
                config.heartbeat_manager_address,
                agent_id,
                request_uri.clone(),
                request_hash,
                snapshot_id,
            )
            .await
        {
            Ok(tx_hash) => {
                info!(slot, tx_hash = ?tx_hash, "Validation request submitted");
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
