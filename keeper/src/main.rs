use alloy::primitives::U256;
use alloy::primitives::utils::format_ether;
use anyhow::{Context, Result, bail};
use args::{CliArgs, KeeperConfig};
use clap::Parser;
use clients::{L1EmissionsClient, L2KeeperClient};
use opentelemetry::KeyValue;
use opentelemetry_otlp::{MetricExporterBuilder, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use std::env;
use std::sync::Arc;
use tokio::signal;
use tokio::signal::unix::SignalKind;
use tokio::sync::Mutex;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::args::OtelConfig;
use crate::l1::EmissionsSupervisor;
use crate::l2::L2Supervisor;

mod args;
mod clients;
mod contracts;
mod l1;
mod l2;
mod metrics;

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

fn setup_otel(config: &OtelConfig) -> anyhow::Result<SdkMeterProvider> {
    let service_name =
        env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| env!("CARGO_PKG_NAME").to_string());
    let attributes = vec![KeyValue::new("service.version", env!("CARGO_PKG_VERSION"))];
    let resource = Resource::builder()
        .with_service_name(service_name)
        .with_attributes(attributes)
        .build();
    let exporter = MetricExporterBuilder::new()
        .with_tonic()
        .with_endpoint(config.endpoint.clone())
        .with_timeout(config.export_timeout)
        .build()
        .context("Failed to build metrics exporter")?;

    let reader = PeriodicReader::builder(exporter)
        .with_interval(config.export_interval)
        .build();
    let provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_reader(reader)
        .build();
    opentelemetry::global::set_meter_provider(provider.clone());
    Ok(provider)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    let cli_args = CliArgs::parse();
    let config = KeeperConfig::load(cli_args).await?;

    let metrics = match &config.otel {
        Some(config) => {
            info!("Exporting metrics to {}", config.endpoint);
            let handle = setup_otel(config).context("Failed to configure metrics")?;
            Some(handle)
        }
        None => {
            info!("Metric exports disabled");
            None
        }
    };

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
            config.l2_staking_operators_address,
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
    let l1 = EmissionsSupervisor::new(config.clone(), l2_client.clone()).await?;
    let l2 = L2Supervisor::new(l2_client, state.clone()).await?;
    l2.spawn(config).await?;
    l1.spawn();

    info!("Press ctrl+c to gracefully shutdown");
    shutdown_signal().await;

    if let Some(metrics) = metrics {
        info!("Shutting down metrics exporter");
        let _ = metrics.shutdown();
    }

    Ok(())
}
