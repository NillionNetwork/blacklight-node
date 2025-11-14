use std::env;

use anyhow::Result;
use blake3::Hasher as Blake3;
use clap::Parser;
use ed25519_dalek::{SigningKey, VerifyingKey};
use ethers::core::types::{Address, H256};
use nilav::{
    smart_contract::{ContractConfig, NilAVClient},
    types::Htx,
};
use rand::random;
use reqwest::Client;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
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

/// Load a value from the state file
fn load_state_value(key: &str) -> Option<String> {
    let path = PathBuf::from(STATE_FILE);
    if !path.exists() {
        return None;
    }
    if let Ok(contents) = fs::read_to_string(&path) {
        for line in contents.lines() {
            if let Some(val) = line.strip_prefix(&format!("{}=", key)) {
                return Some(val.trim().to_string());
            }
        }
    }
    None
}

/// Save a value to the state file, preserving other values
fn save_state_value(key: &str, value: &str) -> Result<()> {
    let path = PathBuf::from(STATE_FILE);
    let mut state = HashMap::new();
    
    // Load existing state
    if path.exists() {
        if let Ok(contents) = fs::read_to_string(&path) {
            for line in contents.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    state.insert(k.to_string(), v.trim().to_string());
                }
            }
        }
    }
    
    // Update the value
    state.insert(key.to_string(), value.to_string());
    
    // Write back to file (sorted by key for consistency)
    let mut keys: Vec<_> = state.keys().collect();
    keys.sort();
    let mut content = String::new();
    for k in keys {
        content.push_str(&format!("{}={}\n", k, state[k]));
    }
    fs::write(&path, content)?;
    Ok(())
}

fn signing_key_from_secret_or_file(secret: Option<String>) -> SigningKey {
    if let Some(secret_hex) = secret {
        if let Ok(decoded) = hex::decode(secret_hex.trim_start_matches("0x")) {
            if decoded.len() == 32 {
                let mut seed = [0u8; 32];
                seed.copy_from_slice(&decoded);
                return SigningKey::from_bytes(&seed);
            }
            // fallback: hash arbitrary input to 32 bytes
            let mut hasher = Blake3::new();
            hasher.update(&decoded);
            let digest = hasher.finalize();
            let seed: [u8; 32] = digest.as_bytes().clone();
            return SigningKey::from_bytes(&seed);
        }
    }
    // Try loading from state file
    if let Some(secret_hex) = load_state_value("NODE_SECRET") {
        if let Ok(decoded) = hex::decode(secret_hex.trim_start_matches("0x")) {
            if decoded.len() == 32 {
                let mut seed = [0u8; 32];
                seed.copy_from_slice(&decoded);
                return SigningKey::from_bytes(&seed);
            }
        }
    }
    // Create new seed and persist
    let seed: [u8; 32] = random();
    let secret_hex = format!("0x{}", hex::encode(seed));
    let _ = save_state_value("NODE_SECRET", &secret_hex);
    SigningKey::from_bytes(&seed)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load or generate node_id, preserving it in state file
    let node_id = if let Some(id) = cli.node_id {
        let _ = save_state_value("NODE_ID", &id);
        id
    } else if let Some(id) = load_state_value("NODE_ID") {
        id
    } else {
        let id = env::var("HOSTNAME")
            .ok()
            .unwrap_or_else(|| format!("node-{}", hex::encode(rand::random::<[u8; 4]>())));
        let _ = save_state_value("NODE_ID", &id);
        id
    };

    let sk = signing_key_from_secret_or_file(cli.node_secret);
    let vk = VerifyingKey::from(&sk);
    println!("[nilAV:{}] pubkey {}", node_id, hex::encode(vk.to_bytes()));

    // Setup smart contract client
    let contract_address = cli.contract_address.parse::<Address>()?;
    let contract_config = ContractConfig::new(cli.rpc_url.clone(), contract_address);
    let client = NilAVClient::new(contract_config, cli.node_private_key).await?;

    println!("[nilAV:{}] Connected to contract at: {}", node_id, client.address());
    println!("[nilAV:{}] Node wallet address: {}", node_id, client.signer_address());

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
    let mut last_processed_block = if let Some(block_str) = load_state_value("LAST_PROCESSED_BLOCK") {
        if let Ok(block) = block_str.parse::<u64>() {
            println!("[nilAV:{}] Resuming from saved block: {}", node_id, block);
            block
        } else {
            match client.get_block_number().await {
                Ok(block) => {
                    println!("[nilAV:{}] Monitoring from current block: {}", node_id, block);
                    block
                }
                Err(e) => {
                    eprintln!("[nilAV:{}] Failed to get current block number: {}", node_id, e);
                    0
                }
            }
        }
    } else {
        match client.get_block_number().await {
            Ok(block) => {
                println!("[nilAV:{}] Monitoring from current block: {}", node_id, block);
                block
            }
            Err(e) => {
                eprintln!("[nilAV:{}] Failed to get current block number: {}", node_id, e);
                0
            }
        }
    };

    println!("[nilAV:{}] Listening for assignments (polling every {}ms)...", node_id, cli.poll_interval_ms);

    loop {
        ticker.tick().await;

        // Get HTX assigned events from the last processed block
        match client.get_htx_assigned_events_from(last_processed_block).await {
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
                                    node_id, htx_id, e.message()
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
                let _ = save_state_value("LAST_PROCESSED_BLOCK", &current_block.to_string());
            }
            Err(e) => {
                eprintln!("[nilAV:{}] Failed to update block number: {}", node_id, e);
            }
        }
    }
}

// Keep verification logic for when we can access HTX data
#[allow(dead_code)]
async fn verify_htx(htx: &Htx) -> Result<(), VerificationError> {
    let client = Client::new();
    // Fetch nil_cc measurement
    let meas_url = &htx.nil_cc_measurement.url;
    let meas_resp = client.get(meas_url).send().await;
    let meas_json: serde_json::Value = match meas_resp.and_then(|r| r.error_for_status()) {
        Ok(resp) => match resp.json().await {
            Ok(v) => v,
            Err(e) => return Err(VerificationError::NilccJson(e.to_string())),
        },
        Err(e) => return Err(VerificationError::NilccUrl(e.to_string())),
    };
    let measurement = meas_json
        .get("measurement")
        .and_then(|v| v.as_str())
        .or_else(|| {
            meas_json
                .get("report")
                .and_then(|r| r.get("measurement"))
                .and_then(|v| v.as_str())
        });
    let measurement = match measurement {
        Some(s) => s.to_string(),
        None => return Err(VerificationError::MissingMeasurement),
    };

    // Fetch builder measurement index
    let builder_resp = client.get(&htx.builder_measurement.url).send().await;
    let builder_json: serde_json::Value = match builder_resp.and_then(|r| r.error_for_status()) {
        Ok(resp) => match resp.json().await {
            Ok(v) => v,
            Err(e) => return Err(VerificationError::BuilderJson(e.to_string())),
        },
        Err(e) => return Err(VerificationError::BuilderUrl(e.to_string())),
    };

    let mut matches_any = false;
    match builder_json {
        serde_json::Value::Object(map) => {
            for (_k, v) in map {
                if let Some(val) = v.as_str() {
                    if val == measurement {
                        matches_any = true;
                        break;
                    }
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                if let Some(val) = v.as_str() {
                    if val == measurement {
                        matches_any = true;
                        break;
                    }
                }
            }
        }
        _ => {}
    }

    if matches_any {
        Ok(())
    } else {
        Err(VerificationError::NotInBuilderIndex)
    }
}

#[derive(Debug)]
enum VerificationError {
    NilccUrl(String),
    NilccJson(String),
    MissingMeasurement,
    BuilderUrl(String),
    BuilderJson(String),
    NotInBuilderIndex,
}

impl VerificationError {
    fn message(&self) -> String {
        match self {
            VerificationError::NilccUrl(e) => format!("invalid nil_cc_measurement URL: {}", e),
            VerificationError::NilccJson(e) => format!("invalid nil_cc_measurement JSON: {}", e),
            VerificationError::MissingMeasurement => {
                "missing `measurement` field (looked at root and report.measurement)".to_string()
            }
            VerificationError::BuilderUrl(e) => format!("invalid builder_measurement URL: {}", e),
            VerificationError::BuilderJson(e) => format!("invalid builder_measurement JSON: {}", e),
            VerificationError::NotInBuilderIndex => {
                "measurement not found in builder index".to_string()
            }
        }
    }
}
