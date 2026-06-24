use alloy::{
    primitives::{
        utils::{format_ether, format_units, parse_ether, parse_units},
        Address, U256,
    },
    sol,
};
use anyhow::{Context, Result};
use clap::Args;
use contract_clients_common::ProviderContext;
use std::path::PathBuf;

sol!(
    #[sol(rpc)]
    contract IERC20 {
        function transfer(address to, uint256 value) external returns (bool);
        function balanceOf(address account) external view returns (uint256);
    }
);

#[derive(Args, Debug)]
pub struct WalletArgs {
    #[command(subcommand)]
    pub command: WalletCommand,
}

#[derive(clap::Subcommand, Debug)]
pub enum WalletCommand {
    /// Send ETH to an address (e.g. 0.1 for 0.1 ETH)
    SendEth { to: String, amount: String },
    /// Send NIL (staking token) to an address (e.g. 100 for 100 NIL)
    SendNil { to: String, amount: String },
    /// Check ETH balance of an address
    BalanceEth { address: String },
    /// Check NIL (staking token) balance of an address
    BalanceNil { address: String },
    /// Show current wallet address and its ETH + NIL balances
    Status,
    /// Fund ETH to multiple addresses from a file (one address per line)
    FundEth {
        /// Path to a file containing destination addresses (one per line)
        #[arg(long)]
        addresses_file: PathBuf,
        /// Amount of ETH to send to each address (e.g. "0.1")
        #[arg(long)]
        amount: String,
    },
    /// Fund NIL to multiple addresses from a file (one address per line)
    FundNil {
        /// Path to a file containing destination addresses (one per line)
        #[arg(long)]
        addresses_file: PathBuf,
        /// Amount of NIL to send to each address (e.g. "100")
        #[arg(long)]
        amount: String,
    },
}

fn parse_address(s: &str) -> Result<Address> {
    s.parse::<Address>()
        .with_context(|| format!("invalid address: {s}"))
}

async fn load_ctx() -> Result<ProviderContext> {
    let rpc_url = std::env::var("RPC_URL").context("RPC_URL not set (env or .env)")?;
    let private_key = std::env::var("PRIVATE_KEY").context("PRIVATE_KEY not set (env or .env)")?;
    ProviderContext::new_http(&rpc_url, &private_key).context("failed to create provider context")
}

fn load_addresses(path: &PathBuf) -> Result<Vec<Address>> {
    let content = std::fs::read_to_string(path).context("failed to read addresses file")?;
    content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(parse_address)
        .collect()
}

fn load_nil_token_address() -> Result<Address> {
    std::env::var("STAKE_TOKEN_ADDRESS")
        .context("STAKE_TOKEN_ADDRESS not set (env or .env)")?
        .parse::<Address>()
        .context("invalid STAKE_TOKEN_ADDRESS")
}

pub async fn run(args: WalletArgs) -> Result<()> {
    let ctx = load_ctx().await?;
    let provider = ctx.provider();
    let my_address = ctx.signer_address();

    match args.command {
        WalletCommand::SendEth { to, amount } => {
            let to = parse_address(&to)?;
            let amount =
                parse_ether(&amount).with_context(|| format!("invalid ETH amount: {amount}"))?;
            let tx_hash = ctx.send_eth(to, amount).await?;
            println!("tx: {tx_hash}");
        }
        WalletCommand::SendNil { to, amount } => {
            let to = parse_address(&to)?;
            let amount: U256 = parse_units(&amount, 6)
                .with_context(|| format!("invalid NIL amount: {amount}"))?
                .into();
            let token = load_nil_token_address()?;
            let erc20 = IERC20::new(token, provider);
            let tx_hash = erc20.transfer(to, amount).send().await?.watch().await?;
            println!("tx: {tx_hash}");
        }
        WalletCommand::BalanceEth { address } => {
            let addr = parse_address(&address)?;
            let balance = ctx.get_balance_of(addr).await?;
            println!("{} ETH", format_ether(balance));
        }
        WalletCommand::BalanceNil { address } => {
            let addr = parse_address(&address)?;
            let token = load_nil_token_address()?;
            let erc20 = IERC20::new(token, provider);
            let balance = erc20.balanceOf(addr).call().await?;
            println!("{} NIL", format_units(balance, 6)?);
        }
        WalletCommand::FundEth {
            addresses_file,
            amount,
        } => {
            let amount =
                parse_ether(&amount).with_context(|| format!("invalid ETH amount: {amount}"))?;

            let addresses = load_addresses(&addresses_file)?;
            if addresses.is_empty() {
                println!("No addresses found in file");
                return Ok(());
            }

            let sender_balance = ctx.get_balance().await?;
            let total_needed = amount * U256::from(addresses.len());
            println!("Sender:      {my_address}");
            println!("Balance:     {} ETH", format_ether(sender_balance));
            println!("Amount each: {} ETH", format_ether(amount));
            println!("Recipients:  {}", addresses.len());
            println!(
                "Total needed: {} ETH (excluding gas)",
                format_ether(total_needed)
            );
            println!();

            let mut success_count = 0u64;
            let mut error_count = 0u64;

            for (i, to) in addresses.iter().enumerate() {
                let label = format!("[{}/{}]", i + 1, addresses.len());
                match ctx.send_eth(*to, amount).await {
                    Ok(tx_hash) => {
                        println!("{label} {to} tx: {tx_hash}");
                        success_count += 1;
                    }
                    Err(e) => {
                        println!("{label} {to} ERROR: {e}");
                        error_count += 1;
                    }
                }
            }

            println!();
            println!("=== Summary ===");
            println!("Successful: {success_count}");
            println!("Errors:     {error_count}");
        }
        WalletCommand::FundNil {
            addresses_file,
            amount,
        } => {
            let amount: U256 = parse_units(&amount, 6)
                .with_context(|| format!("invalid NIL amount: {amount}"))?
                .into();

            let addresses = load_addresses(&addresses_file)?;
            if addresses.is_empty() {
                println!("No addresses found in file");
                return Ok(());
            }

            let token = load_nil_token_address()?;
            let erc20 = IERC20::new(token, provider);

            println!("Sender:      {my_address}");
            println!("Token:       {token}");
            println!("Amount each: {} NIL", format_units(amount, 6)?);
            println!("Recipients:  {}", addresses.len());
            println!();

            let mut success_count = 0u64;
            let mut error_count = 0u64;

            for (i, to) in addresses.iter().enumerate() {
                let label = format!("[{}/{}]", i + 1, addresses.len());
                match erc20.transfer(*to, amount).send().await {
                    Ok(pending) => match pending.watch().await {
                        Ok(tx_hash) => {
                            println!("{label} {to} tx: {tx_hash}");
                            success_count += 1;
                        }
                        Err(e) => {
                            println!("{label} {to} ERROR watching tx: {e}");
                            error_count += 1;
                        }
                    },
                    Err(e) => {
                        println!("{label} {to} ERROR sending tx: {e}");
                        error_count += 1;
                    }
                }
            }

            println!();
            println!("=== Summary ===");
            println!("Successful: {success_count}");
            println!("Errors:     {error_count}");
        }
        WalletCommand::Status => {
            let eth_balance = ctx.get_balance().await?;
            let nil_balance = match load_nil_token_address() {
                Ok(token) => {
                    let erc20 = IERC20::new(token, provider);
                    match erc20.balanceOf(my_address).call().await {
                        Ok(b) => match format_units(b, 6) {
                            Ok(f) => format!("{f} NIL"),
                            Err(e) => format!("(error: {e})"),
                        },
                        Err(e) => format!("(error: {e})"),
                    }
                }
                Err(_) => "(STAKE_TOKEN_ADDRESS not set)".to_string(),
            };

            println!("Address:     {my_address}");
            println!("ETH balance: {} ETH", format_ether(eth_balance));
            println!("NIL balance: {nil_balance}");
        }
    }

    Ok(())
}
