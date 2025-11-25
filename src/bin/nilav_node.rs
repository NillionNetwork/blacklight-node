use std::env;

use anyhow::Result;
use clap::Parser;
use ethers::core::types::{Address, H256};
use nilav::{
    contract_client::{ContractConfig, NilAVWsClient},
    crypto::{load_or_generate_signing_key, verifying_key_from_signing},
    state::StateFile,
    types::Htx,
    verification::verify_htx,
};
use rand::random;
use tracing::{debug, error, info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

const STATE_FILE: &str = "nilav_node.env";

/// NilAV Node - Verifies HTXs assigned by the smart contract using WebSocket streaming
#[derive(Parser)]
#[command(name = "nilav_node")]
#[command(about = "NilAV Node - Real-time HTX verification using WebSocket streaming", long_about = None)]
struct Cli {
    /// Ethereum RPC endpoint (will be converted to WebSocket)
    #[arg(long, env = "RPC_URL", default_value = "http://localhost:8545")]
    rpc_url: String,

    /// NilAV contract address
    #[arg(
        long,
        env = "CONTRACT_ADDRESS",
        default_value = "0x5FbDB2315678afecb367f032d93F642f64180aa3"
    )]
    contract_address: String,

    /// Private key for contract interactions
    #[arg(
        long,
        env = "NODE_PRIVATE_KEY",
        default_value = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d"
    )]
    node_private_key: String,

    /// Node ID for logging
    #[arg(long, env = "NODE_ID")]
    node_id: Option<String>,

    /// Ed25519 signing secret (hex)
    #[arg(long, env = "NODE_SECRET")]
    node_secret: Option<String>,
}

/// Process a single HTX assignment - verifies and submits result
async fn process_htx_assignment(
    ws_client: std::sync::Arc<NilAVWsClient>,
    node_id: &str,
    htx_id: H256,
) -> Result<()> {
    // Retrieve the HTX data from the contract
    let htx_bytes = ws_client.get_htx(htx_id).await.map_err(|e| {
        error!(
            node_id = %node_id,
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
                node_id = %node_id,
                htx_id = ?htx_id,
                error = %e,
                "Failed to parse HTX data"
            );
            // Respond with false if we can't parse the data
            ws_client.respond_htx(htx_id, false).await?;
            warn!(
                node_id = %node_id,
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
            node_id = %node_id,
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
                node_id = %node_id,
                htx_id = ?htx_id,
                tx_hash = ?tx_hash,
                verdict = %verdict,
                "HTX verified"
            );
            Ok(())
        }
        Err(e) => {
            error!(
                node_id = %node_id,
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

    let cli = Cli::parse();
    let state_file = StateFile::new(STATE_FILE);

    // Load or generate node_id, preserving it in state file
    let node_id = if let Some(id) = cli.node_id {
        let _ = state_file.save_value("NODE_ID", &id);
        id
    } else if let Some(id) = state_file.load_value("NODE_ID") {
        id
    } else {
        let id = env::var("HOSTNAME")
            .ok()
            .unwrap_or_else(|| format!("node-{}", hex::encode(random::<[u8; 4]>())));
        let _ = state_file.save_value("NODE_ID", &id);
        id
    };

    // Load or generate signing key
    let secret = cli
        .node_secret
        .or_else(|| state_file.load_value("NODE_SECRET"));
    let (sk, secret_hex) = load_or_generate_signing_key(secret);
    let _ = state_file.save_value("NODE_SECRET", &secret_hex);
    let vk = verifying_key_from_signing(&sk);
    info!(node_id = %node_id, pubkey = %hex::encode(vk.to_bytes()), "Node initialized");

    // Setup contract connection details
    let contract_address = cli.contract_address.parse::<Address>()?;
    let private_key = cli.node_private_key.clone();

    // Start WebSocket event listener with auto-reconnection
    info!(node_id = %node_id, "Starting real-time WebSocket event listener with auto-reconnection");

    // Reconnection loop - will restart the listener if it fails
    let mut reconnect_delay = std::time::Duration::from_secs(1);
    let max_reconnect_delay = std::time::Duration::from_secs(60);
    let mut registered = false;

    loop {
        info!(node_id = %node_id, "Connecting WebSocket listener");

        // Create a fresh WebSocket client for this connection attempt
        let contract_config = ContractConfig::new(cli.rpc_url.clone(), contract_address);
        let ws_client = match NilAVWsClient::new(contract_config, private_key.clone()).await {
            Ok(client) => {
                info!(
                    node_id = %node_id,
                    contract_address = %client.address(),
                    "WebSocket connection established"
                );
                reconnect_delay = std::time::Duration::from_secs(1); // Reset delay on success
                client
            }
            Err(e) => {
                error!(
                    node_id = %node_id,
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
            info!(node_id = %node_id, node_address = %node_address, "Registering node with contract");

            // Check if already registered
            match ws_client.is_node(node_address).await {
                Ok(is_registered) => {
                    if is_registered {
                        info!(node_id = %node_id, "Node already registered");
                        registered = true;
                    } else {
                        match ws_client.register_node(node_address).await {
                            Ok(tx_hash) => {
                                info!(node_id = %node_id, tx_hash = ?tx_hash, "Node registered successfully");
                                registered = true;
                            }
                            Err(e) => {
                                error!(
                                    node_id = %node_id,
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
                        node_id = %node_id,
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
        let node_id_clone = node_id.clone();

        // IMPORTANT: Process any backlog of assignments that happened before we connected
        // Query historical HTX assigned events for this node
        info!(node_id = %node_id, "Checking for pending assignments from before connection");
        match ws_client_arc.get_htx_assigned_events().await {
            Ok(assigned_events) => {
                let pending: Vec<_> = assigned_events
                    .iter()
                    .filter(|e| e.node == node_address)
                    .collect();

                if !pending.is_empty() {
                    info!(
                        node_id = %node_id,
                        count = pending.len(),
                        "Found historical assignments, processing backlog"
                    );

                    for event in pending {
                        let htx_id = H256::from(event.htx_id);

                        // Check if already responded
                        match ws_client_arc.get_assignment(htx_id).await {
                            Ok(assignment) if assignment.responded => {
                                // Already responded, skip
                                continue;
                            }
                            Ok(_) => {
                                debug!(
                                    node_id = %node_id,
                                    htx_id = ?htx_id,
                                    "Processing pending assignment"
                                );
                                // Spawn a task to process this assignment concurrently
                                let ws_client = ws_client_arc.clone();
                                let node_id_clone = node_id.clone();
                                tokio::spawn(async move {
                                    if let Err(e) =
                                        process_htx_assignment(ws_client, &node_id_clone, htx_id)
                                            .await
                                    {
                                        error!(
                                            node_id = %node_id_clone,
                                            htx_id = ?htx_id,
                                            error = %e,
                                            "Failed to process pending HTX"
                                        );
                                    }
                                });
                            }
                            Err(e) => {
                                error!(
                                    node_id = %node_id,
                                    htx_id = ?htx_id,
                                    error = %e,
                                    "Failed to check assignment status"
                                );
                            }
                        }
                    }
                    info!(node_id = %node_id, "Backlog processing complete");
                } else {
                    info!(node_id = %node_id, "No pending assignments found");
                }
            }
            Err(e) => {
                error!(
                    node_id = %node_id,
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
                let node_id = node_id_clone.clone();

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

                                debug!(
                                    node_id = %node_id,
                                    htx_id = ?htx_id,
                                    "Processing real-time assignment"
                                );

                                // Retrieve the HTX data from the contract
                                let htx_bytes = match ws_client.get_htx(htx_id).await {
                                    Ok(bytes) => bytes,
                                    Err(e) => {
                                        error!(
                                            node_id = %node_id,
                                            htx_id = ?htx_id,
                                            error = %e,
                                            "Failed to get HTX data"
                                        );
                                        return;
                                    }
                                };

                                // Parse the HTX data
                                let htx: Htx = match serde_json::from_slice(&htx_bytes) {
                                    Ok(h) => h,
                                    Err(e) => {
                                        error!(
                                            node_id = %node_id,
                                            htx_id = ?htx_id,
                                            error = %e,
                                            "Failed to parse HTX data"
                                        );
                                        // Respond with false if we can't parse the data
                                        match ws_client.respond_htx(htx_id, false).await {
                                            Ok(tx_hash) => {
                                                warn!(
                                                    node_id = %node_id,
                                                    htx_id = ?htx_id,
                                                    tx_hash = ?tx_hash,
                                                    "HTX not verified (parse error) | tx: submitted"
                                                );
                                            }
                                            Err(e) => {
                                                error!(
                                                    node_id = %node_id,
                                                    htx_id = ?htx_id,
                                                    error = %e,
                                                    "Failed to respond to HTX"
                                                );
                                            }
                                        }
                                        return;
                                    }
                                };

                                // Verify the HTX
                                let verification_result = verify_htx(&htx).await;
                                let result = verification_result.is_ok();

                                if let Err(ref e) = verification_result {
                                    warn!(
                                        node_id = %node_id,
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
                                            node_id = %node_id,
                                            htx_id = ?htx_id,
                                            tx_hash = ?tx_hash,
                                            verdict = %verdict,
                                            "HTX verified"
                                        );

                                        // Note: WebSocket mode doesn't need to save block state
                                    }
                                    Err(e) => {
                                        error!(
                                            node_id = %node_id,
                                            htx_id = ?htx_id,
                                            error = %e,
                                            "Failed to respond to HTX"
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                error!(
                                    node_id = %node_id,
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
                    node_id = %node_id,
                    reconnect_delay = ?reconnect_delay,
                    "WebSocket listener exited normally. Reconnecting..."
                );
            }
            Err(e) => {
                error!(
                    node_id = %node_id,
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
