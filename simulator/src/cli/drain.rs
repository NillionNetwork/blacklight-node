use alloy::{
    primitives::{utils::format_ether, Address, U256},
    providers::Provider,
};
use anyhow::{Context, Result};
use clap::Args;
use contract_clients_common::ProviderContext;
use std::path::PathBuf;

#[derive(Args, Debug)]
pub struct DrainArgs {
    /// Path to a file containing private keys (one per line)
    #[arg(long)]
    pub keys_file: PathBuf,

    /// RPC URL of the chain to drain from
    #[arg(long, env = "RPC_URL")]
    pub rpc_url: String,

    /// Destination address to send all ETH to
    #[arg(long)]
    pub destination: String,
}

pub async fn run(args: DrainArgs) -> Result<()> {
    let destination: Address = args
        .destination
        .parse::<Address>()
        .context("invalid destination address")?;

    let keys_content =
        std::fs::read_to_string(&args.keys_file).context("failed to read keys file")?;

    let keys: Vec<&str> = keys_content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    if keys.is_empty() {
        println!("No private keys found in file");
        return Ok(());
    }

    println!("Destination: {destination}");
    println!("Keys loaded: {}", keys.len());
    println!();

    let mut total_drained = U256::ZERO;
    let mut success_count = 0u64;
    let mut skip_count = 0u64;
    let mut error_count = 0u64;

    for (i, key) in keys.iter().enumerate() {
        let label = format!("[{}/{}]", i + 1, keys.len());

        let ctx = match ProviderContext::with_ws_retries(&args.rpc_url, key, Some(3)).await {
            Ok(ctx) => ctx,
            Err(e) => {
                println!("{label} ERROR creating provider: {e}");
                error_count += 1;
                continue;
            }
        };

        let source = ctx.signer_address();
        let balance = match ctx.get_balance().await {
            Ok(b) => b,
            Err(e) => {
                println!("{label} {source} ERROR fetching balance: {e}");
                error_count += 1;
                continue;
            }
        };

        if balance.is_zero() {
            println!("{label} {source} balance=0, skipping");
            skip_count += 1;
            continue;
        }

        // Estimate gas cost for a simple ETH transfer
        let gas_price = match ctx.provider().get_gas_price().await {
            Ok(p) => p,
            Err(e) => {
                println!("{label} {source} ERROR fetching gas price: {e}");
                error_count += 1;
                continue;
            }
        };

        // Simple ETH transfer = 21000 gas. Use at least 1 for max_fee.
        let gas_limit = U256::from(21_000u64);
        let max_fee = std::cmp::max(gas_price, 1);
        let gas_cost = gas_limit * U256::from(max_fee);
        let buffer = gas_cost / U256::from(5); // 20% safety buffer
        let total_cost = gas_cost + buffer;

        if balance <= total_cost {
            println!(
                "{label} {source} balance={} ETH, insufficient to cover gas, skipping",
                format_ether(balance)
            );
            skip_count += 1;
            continue;
        }

        let send_amount = balance - total_cost;

        println!(
            "{label} {source} balance={} ETH, sending {} ETH ...",
            format_ether(balance),
            format_ether(send_amount)
        );

        match ctx.send_eth(destination, send_amount).await {
            Ok(tx_hash) => {
                println!("{label} {source} tx: {tx_hash}");
                total_drained += send_amount;
                success_count += 1;
            }
            Err(e) => {
                println!("{label} {source} ERROR sending tx: {e}");
                error_count += 1;
            }
        }
    }

    println!();
    println!("=== Summary ===");
    println!("Total drained:  {} ETH", format_ether(total_drained));
    println!("Successful:     {success_count}");
    println!("Skipped (zero): {skip_count}");
    println!("Errors:         {error_count}");

    Ok(())
}
