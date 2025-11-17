use std::env;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use ethers::core::types::{Address, H256};
use nilav::{
    contract_client::{ContractConfig, NilAVClient},
    crypto::{load_or_generate_signing_key, verifying_key_from_signing},
    state::StateFile,
    types::Htx,
    verification::verify_htx,
};
use rand::random;
use tokio::time::interval;

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";
const STATE_FILE: &str = "nilav_node.env";

/// NilAV Node - Verifies HTXs assigned by the smart contract
#[derive(Parser)]
#[command(name = "nilav_node")]
#[command(about = "NilAV Node - Verifies HTXs assigned by the smart contract", long_about = None)]
struct Cli {
    /// Ethereum RPC endpoint
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

    /// Poll interval in milliseconds
    #[arg(long, env = "POLL_INTERVAL_MS", default_value = "5000")]
    poll_interval_ms: u64,
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

    // Setup smart contract client
    let contract_address = cli.contract_address.parse::<Address>()?;
    let contract_config = ContractConfig::new(cli.rpc_url.clone(), contract_address);
    let client = NilAVClient::new(contract_config, cli.node_private_key).await?;

    println!(
        "[nilAV:{}] Connected to contract at: {}",
        node_id,
        client.address()
    );
    println!(
        "[nilAV:{}] Node wallet address: {}",
        node_id,
        client.signer_address()
    );

    // Register this node with the contract
    let node_address = client.signer_address();
    println!("[nilAV:{}] Registering node with contract...", node_id);

    // Check if already registered
    let is_registered = client.is_node(node_address).await?;
    if is_registered {
        println!("[nilAV:{}] Node already registered", node_id);
    } else {
        match client.register_node(node_address).await {
            Ok(tx_hash) => {
                println!("[nilAV:{}] Node registered! tx: {:?}", node_id, tx_hash);
            }
            Err(e) => {
                eprintln!("[nilAV:{}] Failed to register node: {}", node_id, e);
                return Err(e);
            }
        }
    }

    let mut ticker = interval(Duration::from_millis(cli.poll_interval_ms));

    // Track the last processed block to avoid reprocessing old events
    // Load from state file if available, otherwise start from current block
    let mut last_processed_block =
        if let Some(block_str) = state_file.load_value("LAST_PROCESSED_BLOCK") {
            if let Ok(block) = block_str.parse::<u64>() {
                println!("[nilAV:{}] Resuming from saved block: {}", node_id, block);
                block
            } else {
                match client.get_block_number().await {
                    Ok(block) => {
                        println!(
                            "[nilAV:{}] Monitoring from current block: {}",
                            node_id, block
                        );
                        block
                    }
                    Err(e) => {
                        eprintln!(
                            "[nilAV:{}] Failed to get current block number: {}",
                            node_id, e
                        );
                        0
                    }
                }
            }
        } else {
            match client.get_block_number().await {
                Ok(block) => {
                    println!(
                        "[nilAV:{}] Monitoring from current block: {}",
                        node_id, block
                    );
                    block
                }
                Err(e) => {
                    eprintln!(
                        "[nilAV:{}] Failed to get current block number: {}",
                        node_id, e
                    );
                    0
                }
            }
        };

    println!(
        "[nilAV:{}] Listening for assignments (polling every {}ms)...",
        node_id, cli.poll_interval_ms
    );

    loop {
        ticker.tick().await;

        // Get HTX assigned events from the last processed block
        match client
            .get_htx_assigned_events_from(last_processed_block)
            .await
        {
            Ok(events) => {
                for event in events {
                    // Only process events assigned to this node
                    if event.node != node_address {
                        continue;
                    }

                    // Get the assignment details to check if already responded
                    let htx_id = H256::from(event.htx_id);
                    match client.get_assignment(htx_id).await {
                        Ok(assignment) => {
                            if assignment.responded {
                                // Already responded, skip
                                continue;
                            }

                            println!(
                                "[nilAV:{}] Processing assignment for HTX {:?}",
                                node_id, htx_id
                            );

                            // Retrieve the HTX data from the contract
                            let htx_bytes = match client.get_htx(htx_id).await {
                                Ok(bytes) => bytes,
                                Err(e) => {
                                    eprintln!(
                                        "[nilAV:{}] Failed to get HTX data for {:?}: {}",
                                        node_id, htx_id, e
                                    );
                                    continue;
                                }
                            };

                            // Parse the HTX data
                            let htx: Htx = match serde_json::from_slice(&htx_bytes) {
                                Ok(h) => h,
                                Err(e) => {
                                    eprintln!(
                                        "[nilAV:{}] Failed to parse HTX data for {:?}: {}",
                                        node_id, htx_id, e
                                    );
                                    // Respond with false if we can't parse the data
                                    match client.respond_htx(htx_id, false).await {
                                        Ok(tx_hash) => {
                                            println!(
                                                "[nilAV:{}] HTX {:?}: {}Not Verified{} (parse error) | tx: {:?}",
                                                node_id, htx_id, RED, RESET, tx_hash
                                            );
                                        }
                                        Err(e) => {
                                            eprintln!(
                                                "[nilAV:{}] Failed to respond to HTX {:?}: {}",
                                                node_id, htx_id, e
                                            );
                                        }
                                    }
                                    continue;
                                }
                            };

                            // Verify the HTX
                            let verification_result = verify_htx(&htx).await;
                            let result = verification_result.is_ok();

                            if let Err(ref e) = verification_result {
                                println!(
                                    "[nilAV:{}] HTX {:?} verification failed: {}",
                                    node_id, htx_id, e
                                );
                            }

                            match client.respond_htx(htx_id, result).await {
                                Ok(tx_hash) => {
                                    let verdict = if result {
                                        format!("{}Verified [VALID]{}", GREEN, RESET)
                                    } else {
                                        format!("{}Verified [INVALID]{}", RED, RESET)
                                    };
                                    println!(
                                        "[nilAV:{}] {} HTX {:?}: | tx: {:?}",
                                        node_id, verdict, htx_id, tx_hash
                                    );
                                }
                                Err(e) => {
                                    eprintln!(
                                        "[nilAV:{}] Failed to respond to HTX {:?}: {}",
                                        node_id, htx_id, e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!(
                                "[nilAV:{}] Failed to get assignment for HTX {:?}: {}",
                                node_id, htx_id, e
                            );
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[nilAV:{}] Failed to get assigned events: {}", node_id, e);
            }
        }

        // Update last processed block to current block to avoid reprocessing
        match client.get_block_number().await {
            Ok(current_block) => {
                last_processed_block = current_block;
                // Persist the last processed block to state file
                let _ = state_file.save_value("LAST_PROCESSED_BLOCK", &current_block.to_string());
            }
            Err(e) => {
                eprintln!("[nilAV:{}] Failed to update block number: {}", node_id, e);
            }
        }
    }
}
