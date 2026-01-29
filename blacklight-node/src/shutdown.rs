use anyhow::Result;
use blacklight_contract_clients::BlacklightClient;
use tokio_util::sync::CancellationToken;
use tracing::info;

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
pub async fn deactivate_node(client: &BlacklightClient) -> Result<()> {
    let node_address = client.signer_address();
    info!("Initiating graceful shutdown");
    info!(node_address = %node_address, "Deactivating node from contract");

    let tx_hash = client.staking.deactivate_operator().await?;
    info!(node_address = %node_address, tx_hash = ?tx_hash, "Node deactivated successfully");

    Ok(())
}
