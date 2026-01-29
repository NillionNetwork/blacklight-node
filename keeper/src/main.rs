use alloy::primitives::U256;
use alloy::primitives::utils::format_ether;
use anyhow::{Context, Result, bail};
use args::{CliArgs, KeeperConfig};
use clap::Parser;
use clients::{L1EmissionsClient, L2KeeperClient};
use std::sync::Arc;
use tokio::signal;
use tokio::signal::unix::SignalKind;
use tokio::sync::Mutex;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::l1::L1Supervisor;
use crate::l2::L2Supervisor;

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

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install ctrl-c handler");
    };

    let terminate = async {
        signal::unix::signal(SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    tokio::select! {
        _ = ctrl_c => {
            info!("Received ctrl-c");
        },
        _ = terminate => {
            info!("Received SIGTERM");
        },
    }
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
    info!("Checking balances for address: {address}");

    let l1_balance = l1_client
        .get_balance()
        .await
        .context("Failed to get L1 balance")?;
    let l2_balance = l2_client
        .get_balance()
        .await
        .context("Failed to get L2 balance")?;
    if l2_balance < MIN_ETH_BALANCE || l1_balance < MIN_ETH_BALANCE {
        bail!(
            "Insufficient funds. Keeper requires at least {} ETH on both L1 and L2.",
            alloy::primitives::utils::format_ether(MIN_ETH_BALANCE)
        );
    }

    let l1_balance = format!("{} ETH", format_ether(l1_balance));
    let l2_balance = format!("{} ETH", format_ether(l2_balance));
    info!(
        l2_balance = l2_balance,
        l1_balance = l1_balance,
        "Keeper wallet {address} ready"
    );

    let state = Arc::new(Mutex::new(Default::default()));
    let l1 = L1Supervisor::new(config.clone()).await?;
    let l2 = L2Supervisor::new(&config, state.clone()).await?;
    l2.spawn(config).await?;
    l1.spawn();

    info!("Press ctrl+c to gracefully shutdown");
    shutdown_signal().await;

    Ok(())
}
