use anyhow::Result;
use args::{CliArgs, NodeConfig};
use clap::Parser;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::verification::HtxVerifier;

mod args;
mod shutdown;
mod supervisor;
mod verification;
mod wallet;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    let filter = EnvFilter::builder()
        .with_default_directive(tracing::Level::ERROR.into())
        .with_default_directive("attestation_verification=warn".parse()?)
        .with_default_directive("alloy_transport_ws=off".parse()?)
        .from_env_lossy()
        .add_directive("nilcc_artifacts=warn".parse()?)
        .add_directive("alloy=warn".parse()?)
        .add_directive("alloy_pubsub=error".parse()?)
        .add_directive("blacklight=info".parse()?)
        .add_directive("blacklight_node=info".parse()?);

    tracing_subscriber::registry()
        .with(fmt::layer().with_ansi(true))
        .with(filter)
        .init();

    // Load configuration
    let cli_args = CliArgs::parse();
    let verifier = HtxVerifier::new(cli_args.artifact_cache.clone(), cli_args.cert_cache.clone())?;
    let config = NodeConfig::load(cli_args).await?;

    // Setup shutdown handler
    let shutdown_token = CancellationToken::new();
    let shutdown_token_clone = shutdown_token.clone();
    tokio::spawn(async move {
        shutdown::shutdown_signal(shutdown_token_clone).await;
    });

    // Create and run supervisor (handles connection, validation, and event processing)
    let supervisor = supervisor::Supervisor::new(&config, &verifier, shutdown_token).await?;
    let client = supervisor.run().await?;

    // Graceful shutdown - deactivate node
    if let Err(e) = shutdown::deactivate_node(&client).await {
        error!(error = %e, "Failed to deactivate node gracefully");
    }

    info!("Shutdown complete");
    Ok(())
}
