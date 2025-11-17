use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use ethers::core::types::Address;
use nilav::{
    config::load_config_from_path,
    contract_client::{ContractConfig, NilAVClient},
    types::Htx,
};
use tokio::time::interval;

/// NilAV Server - Submits HTXs to the smart contract for verification
#[derive(Parser)]
#[command(name = "server")]
#[command(about = "NilAV Server - Submits HTXs to the smart contract", long_about = None)]
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

    /// Private key for signing transactions
    #[arg(
        long,
        env = "PRIVATE_KEY",
        default_value = "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a"
    )]
    private_key: String,

    /// Path to config file
    #[arg(long, env = "CONFIG_PATH", default_value = "config/config.toml")]
    config_path: String,

    /// Path to HTXs JSON file
    #[arg(long, env = "HTXS_PATH", default_value = "data/htxs.json")]
    htxs_path: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load config
    let config = load_config_from_path(&cli.config_path).unwrap_or_default();
    println!(
        "[server] config: validators_per_htx={}, approve_threshold={}",
        config.election.validators_per_htx, config.election.approve_threshold
    );

    // Setup smart contract client
    let contract_address = cli.contract_address.parse::<Address>()?;
    let contract_config = ContractConfig::new(cli.rpc_url.clone(), contract_address);
    let client = NilAVClient::new(contract_config, cli.private_key).await?;

    println!("[server] Connected to contract at: {}", client.address());
    println!("[server] Using signer address: {}", client.signer_address());

    // Load HTXs from file
    let htxs_str = std::fs::read_to_string(&cli.htxs_path).unwrap_or_else(|_| "[]".to_string());
    let htxs: Vec<Htx> = serde_json::from_str(&htxs_str).unwrap_or_else(|_| Vec::new());

    if htxs.is_empty() {
        println!("[server] Warning: No HTXs loaded from {}", cli.htxs_path);
    } else {
        println!("[server] Loaded {} HTXs from {}", htxs.len(), cli.htxs_path);
    }

    // Slot ticker - submits HTXs to the contract
    let mut ticker = interval(Duration::from_millis(config.slot_ms));
    let mut slot: u64 = 0;

    loop {
        ticker.tick().await;
        slot += 1;

        // Pick HTX round-robin from file
        if htxs.is_empty() {
            println!("[server] slot {}: no HTXs to submit", slot);
            continue;
        }

        let idx = ((slot - 1) as usize) % htxs.len();
        let htx = &htxs[idx];

        // Check how many nodes are registered
        let node_count = client.node_count().await?;
        if node_count.is_zero() {
            println!("[server] slot {}: no nodes registered, skipping", slot);
            continue;
        }

        println!(
            "[server] slot {}: submitting HTX to contract ({} nodes registered)",
            slot, node_count
        );

        // Submit HTX to contract - the contract will handle assignment
        match client.submit_htx(htx).await {
            Ok((tx_hash, htx_id)) => {
                println!("[server] slot {}: HTX submitted successfully", slot);
                println!("  Transaction hash: {:?}", tx_hash);
                println!("  HTX ID: {:?}", htx_id);
            }
            Err(e) => {
                eprintln!("[server] slot {}: failed to submit HTX: {}", slot, e);
            }
        }
    }
}
