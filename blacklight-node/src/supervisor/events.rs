use alloy::primitives::Address;
use anyhow::Result;
use blacklight_contract_clients::BlacklightClient;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::verification::HtxVerifier;

use super::htx::process_htx_assignment;

/// Listen for HTX assignment events and process them
pub async fn run_event_listener(
    client: BlacklightClient,
    node_address: Address,
    shutdown_token: CancellationToken,
    verifier: &HtxVerifier,
    verified_counter: Arc<AtomicU64>,
) -> Result<()> {
    let client_for_callback = client.clone();
    let counter_for_callback = verified_counter.clone();
    let shutdown_for_callback = shutdown_token.clone();

    let manager = Arc::new(client.manager.clone());
    let listen_future = manager.listen_htx_assigned_for_node(node_address, move |event| {
        let client = client_for_callback.clone();
        let counter = counter_for_callback.clone();
        let shutdown_clone = shutdown_for_callback.clone();

        async move {
            let htx_id = event.heartbeatKey;
            let node_addr = client.signer_address();
            let verifier = verifier.clone();
            tokio::spawn(async move {
                // Check if already responded
                match client.manager.get_node_vote(htx_id, node_addr).await {
                    Ok(Some(_)) => (),
                    Ok(None) => {
                        info!(htx_id = ?htx_id, "📥 HTX received");
                        if let Err(e) = process_htx_assignment(
                            client,
                            event,
                            &verifier,
                            counter,
                            shutdown_clone,
                            node_address,
                        )
                        .await
                        {
                            error!(htx_id = ?htx_id, error = %e, "Failed to process real-time HTX");
                        }
                    }
                    Err(e) => {
                        error!(htx_id = ?htx_id, error = %e, "Failed to get assignment for HTX");
                    }
                }
            });

            Ok(())
        }
    });

    // Listen for either events or shutdown signal
    tokio::select! {
        result = listen_future => {
            result?;
            Ok(())
        },
        _ = shutdown_token.cancelled() => {
            info!("Shutdown signal received during event listening");
            Err(anyhow::anyhow!("Shutdown requested"))
        }
    }
}
