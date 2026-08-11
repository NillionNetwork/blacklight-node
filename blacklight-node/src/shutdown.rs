use anyhow::{Context, Result};
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::args::NodeConfig;
use crate::supervisor::Supervisor;

/// Setup shutdown signal handler (Ctrl+C / SIGTERM)
pub async fn shutdown_signal(shutdown_token: CancellationToken) {
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

        shutdown_token.cancel();
    }

    #[cfg(not(unix))]
    {
        use tracing::error;

        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                info!("Shutdown signal received (Ctrl+C)");
                shutdown_token.cancel();
            }
            Err(err) => {
                error!(error = %err, "Failed to listen for shutdown signal");
            }
        }
    }
}

/// Deactivate node from contract on shutdown
///
/// Builds a fresh client rather than reusing the supervisor's. Shutdown is often
/// reached precisely because the WebSocket connection died, and on that path the
/// supervisor still holds the dead client — deactivation would then fail and
/// leave the operator active on-chain, so it keeps being selected into
/// committees it can no longer vote in.
pub async fn deactivate_node(config: &NodeConfig) -> Result<()> {
    info!("Initiating graceful shutdown");

    let client = Supervisor::create_client(config)
        .await
        .context("Failed to create client for deactivation")?;
    let node_address = client.signer_address();
    info!(node_address = %node_address, "Deactivating node from contract");

    let tx_hash = client.staking.deactivate_operator().await?;
    info!(node_address = %node_address, tx_hash = ?tx_hash, "Node deactivated successfully");

    Ok(())
}
