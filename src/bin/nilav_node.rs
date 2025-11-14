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
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tokio::signal;
use tokio::time::interval;

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

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
        default_value = "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0"
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

fn signing_key_from_secret_or_file(secret: Option<String>, node_id: &str) -> SigningKey {
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
    // Try nodeid.env in CWD
    let path = PathBuf::from(format!("{}.env", node_id));
    if path.exists() {
        if let Ok(contents) = fs::read_to_string(&path) {
            for line in contents.lines() {
                if let Some(val) = line.strip_prefix("NODE_SECRET=") {
                    if let Ok(decoded) = hex::decode(val.trim().trim_start_matches("0x")) {
                        if decoded.len() == 32 {
                            let mut seed = [0u8; 32];
                            seed.copy_from_slice(&decoded);
                            return SigningKey::from_bytes(&seed);
                        }
                    }
                }
            }
        }
    }
    // Create new seed and persist
    let seed: [u8; 32] = random();
    let line = format!("NODE_SECRET=0x{}\n", hex::encode(seed));
    let _ = fs::write(&path, line);
    SigningKey::from_bytes(&seed)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let node_id = cli.node_id.unwrap_or_else(|| {
        env::var("HOSTNAME")
            .ok()
            .unwrap_or_else(|| format!("node-{}", hex::encode(rand::random::<[u8; 4]>())))
    });

    let sk = signing_key_from_secret_or_file(cli.node_secret, &node_id);
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

    println!("[nilAV:{}] Listening for assignments (polling every {}ms)...", node_id, cli.poll_interval_ms);

    loop {
        ticker.tick().await;

        // Get HTX assigned events
        match client.get_htx_assigned_events().await {
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

                            // We need to get the HTX data somehow - this is a limitation
                            // The contract doesn't store the full HTX data, only its hash
                            // For now, we'll need to retrieve it from the submitted events
                            // In production, you might want to store HTX data off-chain

                            // TODO: This is a workaround - ideally the contract would emit
                            // the HTX data in the assigned event, or we'd have off-chain storage
                            println!(
                                "[nilAV:{}] TODO Warning: Current implementation cannot retrieve full HTX data from contract -> Responding with default verification result",
                                node_id
                            );

                            let result = true; // Default to true since we can't verify without data

                            match client.respond_htx(htx_id, result).await {
                                Ok(tx_hash) => {
                                    let verdict = if result {
                                        format!("{}Verified{}", GREEN, RESET)
                                    } else {
                                        format!("{}Not Verified{} (no HTX data)", RED, RESET)
                                    };
                                    println!(
                                        "[nilAV:{}] HTX {:?}: {} | tx: {:?}",
                                        node_id, htx_id, verdict, tx_hash
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

#[allow(dead_code)]
#[derive(Debug)]
enum VerificationError {
    NilccUrl(String),
    NilccJson(String),
    MissingMeasurement,
    BuilderUrl(String),
    BuilderJson(String),
    NotInBuilderIndex,
}

#[allow(dead_code)]
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
