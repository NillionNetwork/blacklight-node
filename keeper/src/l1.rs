use crate::{args::KeeperConfig, clients::L1EmissionsClient};
use alloy::{eips::BlockNumberOrTag, primitives::U256, providers::Provider};
use anyhow::Result;
use blacklight_contract_clients::common::errors::decode_any_error;
use std::{sync::Arc, time::Duration};
use tokio::{sync::Notify, time::interval};
use tracing::{debug, info, warn};

const INITIAL_RECONNECT_DELAY: Duration = Duration::from_secs(1);
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);

pub async fn run_l1_supervisor(config: KeeperConfig, shutdown_notify: Arc<Notify>) -> Result<()> {
    let mut reconnect_delay = INITIAL_RECONNECT_DELAY;
    let max_delay = MAX_RECONNECT_DELAY;

    loop {
        let l1_client = match create_l1_client_with_retry(&config, shutdown_notify.clone()).await {
            Ok(client) => client,
            Err(_) => break,
        };
        let l1_client = Arc::new(l1_client);

        match run_l1_emissions_loop(
            l1_client.clone(),
            config.l1_bridge_value,
            config.emissions_interval_secs,
            shutdown_notify.clone(),
        )
        .await
        {
            Ok(()) => break,
            Err(e) => {
                warn!(error = %e, reconnect_delay = ?reconnect_delay, "Emissions loop error, reconnecting");
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(reconnect_delay) => {
                reconnect_delay = std::cmp::min(reconnect_delay * 2, max_delay);
            }
            _ = shutdown_notify.notified() => {
                break;
            }
        }
    }

    Ok(())
}

async fn create_l1_client_with_retry(
    config: &KeeperConfig,
    shutdown_notify: Arc<Notify>,
) -> Result<L1EmissionsClient> {
    let mut delay = INITIAL_RECONNECT_DELAY;
    let max_delay = MAX_RECONNECT_DELAY;

    loop {
        match L1EmissionsClient::new(
            config.l1_rpc_url.clone(),
            config.l1_emissions_controller_address,
            config.private_key.clone(),
        )
        .await
        {
            Ok(client) => return Ok(client),
            Err(e) => {
                warn!(error = %e, delay = ?delay, "Failed to connect L1, retrying");
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {
                        delay = std::cmp::min(delay * 2, max_delay);
                    }
                    _ = shutdown_notify.notified() => {
                        return Err(anyhow::anyhow!("Shutdown requested"));
                    }
                }
            }
        }
    }
}

async fn run_l1_emissions_loop(
    l1_client: Arc<L1EmissionsClient>,
    bridge_value: U256,
    emissions_interval_secs: u64,
    shutdown_notify: Arc<Notify>,
) -> Result<()> {
    let mut ticker = interval(Duration::from_secs(emissions_interval_secs));
    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            _ = shutdown_notify.notified() => {
                return Ok(());
            }
        }
        process_emissions(l1_client.clone(), bridge_value).await?;
    }
}

async fn process_emissions(l1_client: Arc<L1EmissionsClient>, bridge_value: U256) -> Result<()> {
    let emissions = l1_client.emissions();
    let minted_epochs = emissions.mintedEpochs().call().await?;
    let total_epochs = emissions.epochs().call().await?;
    if minted_epochs >= total_epochs {
        return Ok(());
    }

    let ready_at = emissions.nextEpochReadyAt().call().await?;
    let latest = l1_client
        .provider()
        .get_block_by_number(BlockNumberOrTag::Latest)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Missing latest block"))?;
    let now = U256::from(latest.header.timestamp);

    if now < ready_at {
        debug!(
            ready_at = ?ready_at,
            now = ?now,
            "Next emission not ready"
        );
        return Ok(());
    }

    info!(
        minted_epochs = ?minted_epochs,
        total_epochs = ?total_epochs,
        "Minting and bridging next emission epoch"
    );

    let call = emissions.mintAndBridgeNextEpoch().value(bridge_value);
    match call.send().await {
        Ok(pending) => {
            let receipt = pending.get_receipt().await?;
            info!(tx_hash = ?receipt.transaction_hash, "Emission minted and bridged");
        }
        Err(e) => {
            return Err(anyhow::anyhow!(format_contract_error(&e)));
        }
    }

    Ok(())
}

fn format_contract_error<E: std::fmt::Display + std::fmt::Debug>(err: &E) -> String {
    decode_any_error(err).to_string()
}
