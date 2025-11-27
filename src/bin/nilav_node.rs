use anyhow::Result;
use clap::Parser;
use ethers::core::types::H256;
use nilav::{
    config::{NodeCliArgs, NodeConfig},
    contract_client::{ContractConfig, NilAVWsClient},
    types::Htx,
    verification::verify_htx,
};
use tracing::{debug, error, info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Process a single HTX assignment - verifies and submits result
async fn process_htx_assignment(
    ws_client: std::sync::Arc<NilAVWsClient>,
    htx_id: H256,
) -> Result<()> {
    // Retrieve the HTX data from the contract
    let htx_bytes = ws_client.get_htx(htx_id).await.map_err(|e| {
        error!(
            htx_id = ?htx_id,
            error = %e,
            "Failed to get HTX data"
        );
        e
    })?;

    // Parse the HTX data
    let htx: Htx = match serde_json::from_slice(&htx_bytes) {
        Ok(h) => h,
        Err(e) => {
            error!(
                htx_id = ?htx_id,
                error = %e,
                "Failed to parse HTX data"
            );
            // Respond with false if we can't parse the data
            ws_client.respond_htx(htx_id, false).await?;
            warn!(
                htx_id = ?htx_id,
                "HTX not verified (parse error) | tx: submitted"
            );
            return Ok(());
        }
    };

    // Verify the HTX
    let verification_result = verify_htx(&htx).await;
    let result = verification_result.is_ok();

    if let Err(ref e) = verification_result {
        warn!(
            htx_id = ?htx_id,
            error = %e,
            "HTX verification failed"
        );
    }

    // Submit the verification result
    match ws_client.respond_htx(htx_id, result).await {
        Ok(tx_hash) => {
            let verdict = if result { "VALID" } else { "INVALID" };
            info!(
                htx_id = ?htx_id,
                tx_hash = ?tx_hash,
                verdict = %verdict,
                "HTX verified"
            );
            Ok(())
        }
        Err(e) => {
            error!(
                htx_id = ?htx_id,
                error = %e,
                "Failed to respond to HTX"
            );
            Err(e)
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(fmt::layer().with_ansi(true))
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    // Load configuration
    let cli_args = NodeCliArgs::parse();
    let config = NodeConfig::load(cli_args)?;

    info!("Node initialized");

    // Start WebSocket event listener with auto-reconnection
    info!("Starting real-time WebSocket event listener with auto-reconnection");

    // Reconnection loop - will restart the listener if it fails
    let mut reconnect_delay = std::time::Duration::from_secs(1);
    let max_reconnect_delay = std::time::Duration::from_secs(60);
    let mut registered = false;

    loop {
        info!("Connecting WebSocket listener");

        // Create a fresh WebSocket client for this connection attempt
        let contract_config = ContractConfig::new(config.rpc_url.clone(), config.contract_address);
        let ws_client = match NilAVWsClient::new(contract_config, config.private_key.clone()).await
        {
            Ok(client) => {
                let balance = client.get_balance().await?;
                info!(balance = ?balance, "WebSocket connection established");
                reconnect_delay = std::time::Duration::from_secs(1); // Reset delay on success
                client
            }
            Err(e) => {
                error!(
                    error = %e,
                    reconnect_delay = ?reconnect_delay,
                    "Failed to connect WebSocket. Retrying..."
                );
                tokio::time::sleep(reconnect_delay).await;
                reconnect_delay = std::cmp::min(reconnect_delay * 2, max_reconnect_delay);
                continue;
            }
        };

        // Register node if not already registered
        let node_address = ws_client.signer_address();
        if !registered {
            info!(node_address = %node_address, "Registering node with contract");

            // Check if already registered
            match ws_client.is_node(node_address).await {
                Ok(is_registered) => {
                    if is_registered {
                        info!("Node already registered");
                        registered = true;
                    } else {
                        match ws_client.register_node(node_address).await {
                            Ok(tx_hash) => {
                                info!(tx_hash = ?tx_hash, "Node registered successfully");
                                registered = true;
                            }
                            Err(e) => {
                                error!(
                                    error = %e,
                                    reconnect_delay = ?reconnect_delay,
                                    "Failed to register node. Retrying..."
                                );
                                tokio::time::sleep(reconnect_delay).await;
                                reconnect_delay =
                                    std::cmp::min(reconnect_delay * 2, max_reconnect_delay);
                                continue;
                            }
                        }
                    }
                }
                Err(e) => {
                    error!(
                        error = %e,
                        reconnect_delay = ?reconnect_delay,
                        "Failed to check registration. Retrying..."
                    );
                    tokio::time::sleep(reconnect_delay).await;
                    reconnect_delay = std::cmp::min(reconnect_delay * 2, max_reconnect_delay);
                    continue;
                }
            }
        }

        let ws_client_arc = std::sync::Arc::new(ws_client);

        // Start a background keepalive task to prevent connection timeouts
        // This task periodically queries the blockchain to keep the WebSocket connection alive
        let ws_client_keepalive = ws_client_arc.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
            loop {
                interval.tick().await;
                match ws_client_keepalive.get_block_number().await {
                    Ok(block) => {
                        debug!(block_number = %block, "Keepalive ping successful");
                    }
                    Err(e) => {
                        warn!(error = %e, "Keepalive ping failed - connection may be dead");
                        break; // Exit keepalive task if connection is dead
                    }
                }
            }
        });

        // IMPORTANT: Process any backlog of assignments that happened before we connected
        // Query historical HTX assigned events for this node
        info!("Checking for pending assignments from before connection");
        match ws_client_arc.get_htx_assigned_events().await {
            Ok(assigned_events) => {
                let pending: Vec<_> = assigned_events
                    .iter()
                    .filter(|e| e.node == node_address)
                    .collect();

                if !pending.is_empty() {
                    info!(
                        count = pending.len(),
                        "Found historical assignments, processing backlog"
                    );

                    for event in pending {
                        let htx_id = H256::from(event.htx_id);

                        // Check if already responded
                        match ws_client_arc.get_assignment(htx_id).await {
                            Ok(assignment) if assignment.responded => {
                                // Already responded, skip
                                debug!(htx_id = ?htx_id, "Already responded HTX, skipping");
                                continue;
                            }
                            Ok(_) => {
                                info!(
                                    htx_id = ?htx_id,
                                    "Processing pending HTX"
                                );
                                // Spawn a task to process this assignment concurrently
                                let ws_client = ws_client_arc.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = process_htx_assignment(ws_client, htx_id).await
                                    {
                                        error!(
                                            htx_id = ?htx_id,
                                            error = %e,
                                            "Failed to process pending HTX"
                                        );
                                    }
                                });
                            }
                            Err(e) => {
                                error!(
                                    htx_id = ?htx_id,
                                    error = %e,
                                    "Failed to check assignment status"
                                );
                            }
                        }
                    }
                    info!("Backlog processing complete");
                } else {
                    info!("No pending assignments found");
                }
            }
            Err(e) => {
                error!(
                    error = %e,
                    "Failed to query historical assignments"
                );
            }
        }

        // Start listening for HTX assigned events for this specific node
        let ws_client_for_callback = ws_client_arc.clone();
        let listen_result = ws_client_arc
            .listen_htx_assigned_for_node(node_address, move |event| {
                let ws_client = ws_client_for_callback.clone();

                async move {
                    let htx_id = H256::from(event.htx_id);

                    // Spawn a task to process this HTX concurrently (non-blocking)
                    tokio::spawn(async move {
                        // Check if already responded
                        match ws_client.get_assignment(htx_id).await {
                            Ok(assignment) => {
                                if assignment.responded {
                                    // Already responded, skip
                                    return;
                                }

                                info!(
                                    htx_id = ?htx_id,
                                    "Processing HTX"
                                );

                                // Use the same function as backlog processing
                                if let Err(e) = process_htx_assignment(ws_client, htx_id).await {
                                    error!(
                                        htx_id = ?htx_id,
                                        error = %e,
                                        "Failed to process real-time HTX"
                                    );
                                }
                            }
                            Err(e) => {
                                error!(
                                    htx_id = ?htx_id,
                                    error = %e,
                                    "Failed to get assignment for HTX"
                                );
                            }
                        }
                    });

                    // Return immediately to allow processing next event
                    Ok(())
                }
            })
            .await;

        // If we reach here, the listener has exited (connection dropped or error)
        match listen_result {
            Ok(_) => {
                warn!(
                    reconnect_delay = ?reconnect_delay,
                    "WebSocket listener exited normally. Reconnecting..."
                );
            }
            Err(e) => {
                error!(
                    error = %e,
                    reconnect_delay = ?reconnect_delay,
                    "WebSocket listener error. Reconnecting..."
                );
            }
        }

        tokio::time::sleep(reconnect_delay).await;
        reconnect_delay = std::cmp::min(reconnect_delay * 2, max_reconnect_delay);
    }
}
