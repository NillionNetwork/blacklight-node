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

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const RESET: &str = "\x1b[0m";
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
        eprintln!(
            "[nilAV:{}] Failed to get HTX data for {:?}: {}",
            node_id, htx_id, e
        );
        e
    })?;

    // Parse the HTX data
    let htx: Htx = match serde_json::from_slice(&htx_bytes) {
        Ok(h) => h,
        Err(e) => {
            eprintln!(
                "[nilAV:{}] Failed to parse HTX data for {}{:?}{}: {}",
                node_id, CYAN, htx_id, RESET, e
            );
            // Respond with false if we can't parse the data
            ws_client.respond_htx(htx_id, false).await?;
            println!(
                "[nilAV:{}] HTX {}{:?}{}: {}Not Verified{} (parse error) | tx: submitted",
                node_id, CYAN, htx_id, RESET, RED, RESET
            );
            return Ok(());
        }
    };

    // Verify the HTX
    let verification_result = verify_htx(&htx).await;
    let result = verification_result.is_ok();

    if let Err(ref e) = verification_result {
        println!(
            "[nilAV:{}] HTX {}{:?}{} verification failed: {}",
            node_id, CYAN, htx_id, RESET, e
        );
    }

    // Submit the verification result
    match ws_client.respond_htx(htx_id, result).await {
        Ok(tx_hash) => {
            let verdict = if result {
                format!("{}Verified [VALID]{}", GREEN, RESET)
            } else {
                format!("{}Verified [INVALID]{}", RED, RESET)
            };
            println!(
                "[nilAV:{}] {} HTX {}{:?}{}: | tx: {:?}",
                node_id, verdict, CYAN, htx_id, RESET, tx_hash
            );
            Ok(())
        }
        Err(e) => {
            eprintln!(
                "[nilAV:{}] Failed to respond to HTX {}{:?}{}: {}",
                node_id, YELLOW, htx_id, RESET, e
            );
            Err(e)
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
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
    println!("[nilAV:{}] pubkey {}", node_id, hex::encode(vk.to_bytes()));

    // Setup contract connection details
    let contract_address = cli.contract_address.parse::<Address>()?;
    let private_key = cli.node_private_key.clone();

    // Start WebSocket event listener with auto-reconnection
    println!(
        "[nilAV:{}] Starting real-time WebSocket event listener with auto-reconnection...",
        node_id
    );

    // Reconnection loop - will restart the listener if it fails
    let mut reconnect_delay = std::time::Duration::from_secs(1);
    let max_reconnect_delay = std::time::Duration::from_secs(60);
    let mut registered = false;

    loop {
        println!("[nilAV:{}] Connecting WebSocket listener...", node_id);

        // Create a fresh WebSocket client for this connection attempt
        let contract_config = ContractConfig::new(cli.rpc_url.clone(), contract_address);
        let ws_client = match NilAVWsClient::new(contract_config, private_key.clone()).await {
            Ok(client) => {
                println!("[nilAV:{}] WebSocket connection established", node_id);
                println!(
                    "[nilAV:{}] Connected to contract at: {}",
                    node_id,
                    client.address()
                );
                reconnect_delay = std::time::Duration::from_secs(1); // Reset delay on success
                client
            }
            Err(e) => {
                eprintln!(
                    "[nilAV:{}] Failed to connect WebSocket: {}. Retrying in {:?}...",
                    node_id, e, reconnect_delay
                );
                tokio::time::sleep(reconnect_delay).await;
                reconnect_delay = std::cmp::min(reconnect_delay * 2, max_reconnect_delay);
                continue;
            }
        };

        // Register node if not already registered
        let node_address = ws_client.signer_address();
        if !registered {
            println!("[nilAV:{}] Node wallet address: {}", node_id, node_address);
            println!("[nilAV:{}] Registering node with contract...", node_id);

            // Check if already registered
            match ws_client.is_node(node_address).await {
                Ok(is_registered) => {
                    if is_registered {
                        println!("[nilAV:{}] Node already registered", node_id);
                        registered = true;
                    } else {
                        match ws_client.register_node(node_address).await {
                            Ok(tx_hash) => {
                                println!("[nilAV:{}] Node registered! tx: {:?}", node_id, tx_hash);
                                registered = true;
                            }
                            Err(e) => {
                                eprintln!(
                                    "[nilAV:{}] Failed to register node: {}. Retrying in {:?}...",
                                    node_id, e, reconnect_delay
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
                    eprintln!(
                        "[nilAV:{}] Failed to check registration: {}. Retrying in {:?}...",
                        node_id, e, reconnect_delay
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
        println!(
            "[nilAV:{}] Checking for pending assignments from before connection...",
            node_id
        );
        match ws_client_arc.get_htx_assigned_events().await {
            Ok(assigned_events) => {
                let pending: Vec<_> = assigned_events
                    .iter()
                    .filter(|e| e.node == node_address)
                    .collect();

                if !pending.is_empty() {
                    println!("[nilAV:{}] Found {} historical assignment(s) for this node, processing backlog...",
                             node_id, pending.len());

                    for event in pending {
                        let htx_id = H256::from(event.htx_id);

                        // Check if already responded
                        match ws_client_arc.get_assignment(htx_id).await {
                            Ok(assignment) if assignment.responded => {
                                // Already responded, skip
                                continue;
                            }
                            Ok(_) => {
                                println!(
                                    "[nilAV:{}] Processing pending assignment for HTX {}{:?}{}",
                                    node_id, CYAN, htx_id, RESET
                                );
                                // Spawn a task to process this assignment concurrently
                                let ws_client = ws_client_arc.clone();
                                let node_id_clone = node_id.clone();
                                tokio::spawn(async move {
                                    if let Err(e) =
                                        process_htx_assignment(ws_client, &node_id_clone, htx_id)
                                            .await
                                    {
                                        eprintln!(
                                            "[nilAV:{}] Failed to process pending HTX {}{:?}{}: {}",
                                            node_id_clone, CYAN, htx_id, RESET, e
                                        );
                                    }
                                });
                            }
                            Err(e) => {
                                eprintln!("[nilAV:{}] Failed to check assignment status for HTX {}{:?}{}: {}", node_id, CYAN, htx_id, RESET, e);
                            }
                        }
                    }
                    println!("[nilAV:{}] Backlog processing complete", node_id);
                } else {
                    println!("[nilAV:{}] No pending assignments found", node_id);
                }
            }
            Err(e) => {
                eprintln!(
                    "[nilAV:{}] Failed to query historical assignments: {}",
                    node_id, e
                );
            }
        }

        // Start listening for HTX assigned events for this specific node
        let ws_client_for_callback = ws_client_arc.clone();
        let listen_result = ws_client_arc.listen_htx_assigned_for_node(
            node_address,
            move |event| {
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

                            println!(
                                "[nilAV:{}] Processing real-time assignment for HTX {}{:?}{}",
                                node_id, CYAN, htx_id, RESET
                            );

                            // Retrieve the HTX data from the contract
                            let htx_bytes = match ws_client.get_htx(htx_id).await {
                                Ok(bytes) => bytes,
                                Err(e) => {
                                    eprintln!(
                                        "[nilAV:{}] Failed to get HTX data for {}{:?}{}: {}",
                                        node_id, CYAN, htx_id, RESET, e
                                    );
                                    return;
                                }
                            };

                            // Parse the HTX data
                            let htx: Htx = match serde_json::from_slice(&htx_bytes) {
                                Ok(h) => h,
                                Err(e) => {
                                    eprintln!(
                                        "[nilAV:{}] Failed to parse HTX data for {}{:?}{}: {}",
                                        node_id, CYAN, htx_id, RESET, e
                                    );
                                    // Respond with false if we can't parse the data
                                    match ws_client.respond_htx(htx_id, false).await {
                                        Ok(tx_hash) => {
                                            println!(
                                                "[nilAV:{}] HTX {}{:?}{}: {}Not Verified{} (parse error) | tx: {:?}",
                                                node_id, CYAN, htx_id, RESET, RED, RESET, tx_hash
                                            );
                                        }
                                        Err(e) => {
                                            eprintln!(
                                                "[nilAV:{}] Failed to respond to HTX {}{:?}{}: {}",
                                                node_id, CYAN, htx_id, RESET, e
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
                                println!(
                                    "[nilAV:{}] HTX {}{:?}{} verification failed: {}",
                                    node_id, CYAN, htx_id, RESET, e
                                );
                            }

                            // Submit the verification result
                            match ws_client.respond_htx(htx_id, result).await {
                                Ok(tx_hash) => {
                                    let verdict = if result {
                                        format!("{}Verified [VALID]{}", GREEN, RESET)
                                    } else {
                                        format!("{}Verified [INVALID]{}", RED, RESET)
                                    };
                                    println!(
                                        "[nilAV:{}] {} HTX {}{:?}{}: | tx: {:?}",
                                        node_id, verdict, CYAN, htx_id, RESET, tx_hash
                                    );

                                    // Note: WebSocket mode doesn't need to save block state
                                }
                                Err(e) => {
                                    eprintln!(
                                        "[nilAV:{}] Failed to respond to HTX {}{:?}{}: {}",
                                        node_id, CYAN, htx_id, RESET, e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "[nilAV:{}] Failed to get assignment for HTX {}{:?}{}: {}",
                                node_id, CYAN, htx_id, RESET, e
                            );
                        }
                    }
                    });

                    // Return immediately to allow processing next event
                    Ok(())
                }
            }
        ).await;

        // If we reach here, the listener has exited (connection dropped or error)
        match listen_result {
            Ok(_) => {
                eprintln!(
                    "[nilAV:{}] WebSocket listener exited normally. Reconnecting in {:?}...",
                    node_id, reconnect_delay
                );
            }
            Err(e) => {
                eprintln!(
                    "[nilAV:{}] WebSocket listener error: {}. Reconnecting in {:?}...",
                    node_id, e, reconnect_delay
                );
            }
        }

        tokio::time::sleep(reconnect_delay).await;
        reconnect_delay = std::cmp::min(reconnect_delay * 2, max_reconnect_delay);
    }
}
