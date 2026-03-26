use alloy::primitives::Address;
use anyhow::{Context, Result};
use blacklight_contract_clients::NodeOperatorFactoryClient;
use clap::Parser;
use contract_clients_common::ProviderContext;
use std::time::Duration;
use tokio::signal;
use tokio::signal::unix::SignalKind;
use tokio::time::interval;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(Parser, Debug)]
#[command(
    name = "managed-node-keeper",
    about = "Periodically harvests rewards for all managed nodes in the factory"
)]
struct Args {
    /// L2 RPC endpoint
    #[arg(long, env = "L2_RPC_URL")]
    l2_rpc_url: String,

    /// NodeOperatorFactory contract address
    #[arg(long, env = "L2_NODE_OPERATOR_FACTORY_ADDRESS")]
    l2_node_operator_factory_address: Address,

    /// Private key for contract interactions
    #[arg(long, env = "PRIVATE_KEY")]
    private_key: String,

    /// Harvest interval in seconds (default: 1200 = 20 mins)
    #[arg(long, env = "HARVEST_INTERVAL_SECS", default_value_t = 1200)]
    harvest_interval_secs: u64,
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
    dotenv::from_filename("mn_keeper.env").ok();

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    let args = Args::parse();
    let harvest_interval = Duration::from_secs(args.harvest_interval_secs);

    let ctx = ProviderContext::new_http(&args.l2_rpc_url, &args.private_key)
        .context("Failed to create provider")?;

    let factory = NodeOperatorFactoryClient::new(
        ctx.provider().clone(),
        args.l2_node_operator_factory_address,
        ctx.tx_lock(),
    );

    info!(
        factory = ?args.l2_node_operator_factory_address,
        signer = ?ctx.signer_address(),
        interval_secs = args.harvest_interval_secs,
        "Managed node keeper started"
    );

    let harvest_loop = async {
        let mut ticker = interval(harvest_interval);
        loop {
            ticker.tick().await;
            match factory.harvest_all_rewards().await {
                Ok(tx_hash) => {
                    info!("Harvested all rewards: {tx_hash}");
                }
                Err(e) => {
                    error!("Failed to harvest rewards: {e}");
                }
            }
        }
    };

    tokio::select! {
        _ = harvest_loop => {},
        _ = shutdown_signal() => {
            info!("Shutting down");
        },
    }

    Ok(())
}
