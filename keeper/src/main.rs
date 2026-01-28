use alloy::primitives::U256;
use alloy::providers::Provider;
use anyhow::{Context, Result};
use args::{CliArgs, KeeperConfig};
use clap::Parser;
use clients::{L1EmissionsClient, L2KeeperClient};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::l1::run_l1_supervisor;
use crate::l2::run_l2_supervisor;

mod args;
mod clients;
mod contracts;
mod l1;
mod l2;

const MIN_ETH_BALANCE: U256 = eth_to_wei(0.00001);

const fn eth_to_wei(eth: f64) -> U256 {
    let wei = (eth * 1_000_000_000_000_000_000.0) as u64;
    U256::from_limbs([wei, 0, 0, 0])
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    let cli_args = CliArgs::parse();
    let config = KeeperConfig::load(cli_args).await?;

    if config.disable_jailing || config.l2_jailing_policy_address.is_none() {
        info!("Jailing disabled");
    } else {
        info!(
            jailing_policy = ?config.l2_jailing_policy_address,
            "Jailing enabled"
        );
    }

    info!("Keeper initialized");

    let l2_client = Arc::new(
        L2KeeperClient::new(
            config.l2_rpc_url.clone(),
            config.l2_heartbeat_manager_address,
            config.l2_jailing_policy_address,
            config.private_key.clone(),
        )
        .await?,
    );
    let l1_client = Arc::new(
        L1EmissionsClient::new(
            config.l1_rpc_url.clone(),
            config.l1_emissions_controller_address,
            config.private_key.clone(),
        )
        .await?,
    );

    let address = l1_client.signer_address();
    info!("Checking L1 balance for address: {address}");
    let l1_balance = l1_client
        .provider()
        .get_balance(address)
        .await
        .context("Failed to get L1 balance")?;

    info!("Checking L2 balance for address: {address}");
    let l2_balance = l2_client
        .provider()
        .get_balance(address)
        .await
        .context("Failed to get L2 balance")?;

    if l2_balance < MIN_ETH_BALANCE || l1_balance < MIN_ETH_BALANCE {
        anyhow::bail!(
            "Insufficient funds. Keeper requires at least {} ETH on both L1 and L2.",
            alloy::primitives::utils::format_ether(MIN_ETH_BALANCE)
        );
    }

    let l2_balance = l2_client.get_balance().await?;
    let l1_balance = l1_client.get_balance().await?;
    info!(l2_balance = ?l2_balance, l1_balance = ?l1_balance, "Keeper wallet {address} ready");

    info!("Press Ctrl+C to gracefully shutdown");

    let shutdown_notify = Arc::new(Notify::new());
    let shutdown_clone = shutdown_notify.clone();
    tokio::spawn(async move {
        setup_shutdown_handler(shutdown_clone).await;
    });

    let state = Arc::new(Mutex::new(Default::default()));

    let l2_handle = tokio::spawn(run_l2_supervisor(
        config.clone(),
        state.clone(),
        shutdown_notify.clone(),
    ));
    let l1_handle = tokio::spawn(run_l1_supervisor(config, shutdown_notify.clone()));

    shutdown_notify.notified().await;
    info!("Shutdown requested, stopping keeper");

    let _ = l2_handle.await;
    let _ = l1_handle.await;

    Ok(())
}

async fn setup_shutdown_handler(shutdown_notify: Arc<Notify>) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm =
            signal(SignalKind::terminate()).expect("Failed to register SIGTERM handler");
        let mut sigint =
            signal(SignalKind::interrupt()).expect("Failed to register SIGINT handler");

        tokio::select! {
            _ = sigterm.recv() => {
                info!("Shutdown signal received (SIGTERM)");
            }
            _ = sigint.recv() => {
                info!("Shutdown signal received (SIGINT/Ctrl+C)");
            }
        }

        shutdown_notify.notify_waiters();
    }

    #[cfg(not(unix))]
    {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                info!("Shutdown signal received (Ctrl+C)");
                shutdown_notify.notify_waiters();
            }
            Err(err) => {
                error!(error = %err, "Failed to listen for shutdown signal");
            }
        }
    }
}
