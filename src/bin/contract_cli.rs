use anyhow::Result;
use clap::{Parser, Subcommand};
use ethers::core::types::{Address, H256};
use nilav::{
    contract_client::{ContractConfig, NilAVClient},
    types::Htx,
};
use std::fs;

/// NilAV Contract CLI - Interface for interacting with the NilAV smart contract
#[derive(Parser)]
#[command(name = "contract_cli")]
#[command(about = "NilAV Contract CLI", long_about = None)]
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
        default_value = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
    )]
    private_key: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Register a new nilAV node
    RegisterNode {
        /// Address of the node to register
        node_address: String,
    },

    /// Deregister a nilAV node
    DeregisterNode {
        /// Address of the node to deregister
        node_address: String,
    },

    /// List all registered nodes
    ListNodes,

    /// Get total number of registered nodes
    NodeCount,

    /// Check if address is a registered node
    IsNode {
        /// Address to check
        address: String,
    },

    /// Submit an HTX for verification
    SubmitHtx {
        /// HTX data as JSON string
        htx_json: String,
    },

    /// Submit an HTX from a file
    SubmitHtxFile {
        /// HTX file path
        htx_file: String,
    },

    /// Get assignment details for HTX
    GetAssignment {
        /// HTX ID (hex string with 0x prefix)
        htx_id: String,
    },

    /// List all HTX submitted events
    EventsSubmitted,

    /// List all HTX assigned events
    EventsAssigned,

    /// List all HTX responded events
    EventsResponded,

    /// List all node registration events
    EventsNodes,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let contract_address = cli.contract_address.parse::<Address>()?;
    let config = ContractConfig::new(cli.rpc_url, contract_address);
    let client = NilAVClient::new(config, cli.private_key).await?;

    println!("Connected to contract at: {}", client.address());
    println!("Using signer address: {}", client.signer_address());
    println!();

    match cli.command {
        Commands::RegisterNode { node_address } => {
            let node_addr: Address = node_address.parse()?;
            let tx_hash = client.register_node(node_addr).await?;
            println!("Node registered!");
            println!("Transaction hash: {:?}", tx_hash);
        }

        Commands::DeregisterNode { node_address } => {
            let node_addr: Address = node_address.parse()?;
            let tx_hash = client.deregister_node(node_addr).await?;
            println!("Node deregistered!");
            println!("Transaction hash: {:?}", tx_hash);
        }

        Commands::ListNodes => {
            let nodes = client.get_nodes().await?;
            println!("Registered nodes ({}):", nodes.len());
            for (i, node) in nodes.iter().enumerate() {
                println!("  [{}] {}", i, node);
            }
        }

        Commands::NodeCount => {
            let count = client.node_count().await?;
            println!("Total registered nodes: {}", count);
        }

        Commands::IsNode { address } => {
            let addr: Address = address.parse()?;
            let is_node = client.is_node(addr).await?;
            println!(
                "Address {} is {}a registered node",
                addr,
                if is_node { "" } else { "NOT " }
            );
        }

        Commands::SubmitHtx { htx_json } => {
            let htx_data: Htx = serde_json::from_str(&htx_json)?;
            let (tx_hash, htx_id) = client.submit_htx(&htx_data).await?;
            println!("HTX submitted!");
            println!("Transaction hash: {:?}", tx_hash);
            println!("HTX ID: {:?}", htx_id);
        }

        Commands::SubmitHtxFile { htx_file } => {
            let htx_data = fs::read_to_string(htx_file)?;
            let htx_data: Htx = serde_json::from_str(&htx_data)?;
            let (tx_hash, htx_id) = client.submit_htx(&htx_data).await?;
            println!("HTX submitted!");
            println!("Transaction hash: {:?}", tx_hash);
            println!("HTX ID: {:?}", htx_id);
        }

        Commands::GetAssignment { htx_id } => {
            let htx_id: H256 = htx_id.parse()?;
            let assignment = client.get_assignment(htx_id).await?;
            println!("Assignment details:");
            println!("  Node: {}", assignment.node);
            println!("  Responded: {}", assignment.responded);
            println!("  Result: {}", assignment.result);
        }

        Commands::EventsSubmitted => {
            let events = client.get_htx_submitted_events().await?;
            println!("HTX Submitted Events ({}):", events.len());
            for event in events {
                println!("  HTX ID: {:?}", event.htx_id);
                println!("  Raw HTX Hash: {:?}", event.raw_htx_hash);
                println!("  Sender: {:?}", event.sender);
                println!();
            }
        }

        Commands::EventsAssigned => {
            let events = client.get_htx_assigned_events().await?;
            println!("HTX Assigned Events ({}):", events.len());
            for event in events {
                println!("  HTX ID: {:?}", event.htx_id);
                println!("  Node: {:?}", event.node);
                println!();
            }
        }

        Commands::EventsResponded => {
            let events = client.get_htx_responded_events().await?;
            println!("HTX Responded Events ({}):", events.len());
            for event in events {
                println!("  HTX ID: {:?}", event.htx_id);
                println!("  Node: {:?}", event.node);
                println!("  Result: {}", event.result);
                println!();
            }
        }

        Commands::EventsNodes => {
            let registered = client.get_node_registered_events().await?;
            let deregistered = client.get_node_deregistered_events().await?;
            println!("Node Registered Events ({}):", registered.len());
            for event in registered {
                println!("  Node: {:?}", event.node);
            }
            println!();
            println!("Node Deregistered Events ({}):", deregistered.len());
            for event in deregistered {
                println!("  Node: {:?}", event.node);
            }
        }
    }

    Ok(())
}
