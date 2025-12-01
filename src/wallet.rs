use anyhow::{Context, Result};
use ethers::prelude::*;
use ethers::signers::LocalWallet;
use std::str::FromStr;
use term_table::row::Row;
use term_table::table_cell::{Alignment as CellAlignment, TableCell};
use term_table::{Table, TableStyle};
use tracing::{info, warn};

/// Generate a new random Ethereum wallet
pub fn generate_wallet() -> Result<LocalWallet> {
    let wallet = LocalWallet::new(&mut rand::thread_rng());
    Ok(wallet)
}

/// Load wallet from private key string (with or without 0x prefix)
pub fn load_wallet(private_key: &str) -> Result<LocalWallet> {
    let key = private_key.trim_start_matches("0x");
    let wallet = LocalWallet::from_str(key).context("Failed to parse private key")?;
    Ok(wallet)
}

/// Check if an address has funds on the given RPC endpoint
pub async fn check_balance(rpc_url: &str, address: Address) -> Result<U256> {
    let provider =
        Provider::<Http>::try_from(rpc_url).context("Failed to connect to RPC endpoint")?;

    let balance = provider
        .get_balance(address, None)
        .await
        .context("Failed to fetch balance")?;

    Ok(balance)
}

/// Display a nice banner warning about insufficient funds
pub fn display_insufficient_funds_banner(address: Address, rpc_url: &str) {
    let address_str = format!("{:?}", address);

    let mut table = Table::new();
    table.style = TableStyle::extended();
    table.add_row(Row::new(vec![TableCell::builder(
        "❌  INSUFFICIENT FUNDS  ❌",
    )
    .col_span(2)
    .alignment(CellAlignment::Center)
    .build()]));
    table.add_row(Row::new(vec![
        TableCell::builder("Address")
            .alignment(CellAlignment::Right)
            .build(),
        TableCell::builder(address_str)
            .alignment(CellAlignment::Left)
            .build(),
    ]));
    table.add_row(Row::new(vec![
        TableCell::builder("RPC URL")
            .alignment(CellAlignment::Right)
            .build(),
        TableCell::builder(rpc_url.to_owned())
            .alignment(CellAlignment::Left)
            .build(),
    ]));
    table.add_row(Row::new(vec![TableCell::builder("❌ No ETH Balance")
        .col_span(2)
        .alignment(CellAlignment::Center)
        .build()]));
    table.add_row(Row::new(vec![TableCell::builder(
        "Please fund this address with ETH",
    )
    .col_span(2)
    .alignment(CellAlignment::Center)
    .build()]));

    warn!("\n{}", table.render());
}

/// Display a nice banner warning about insufficient funds
pub fn display_account_created_banner(address: Address, rpc_url: &str) {
    let address_str = format!("{:?}", address);

    let mut table = Table::new();
    table.style = TableStyle::extended();
    table.add_row(Row::new(vec![TableCell::builder(
        "✅  Account Created Successfully ✅",
    )
    .col_span(2)
    .alignment(CellAlignment::Center)
    .build()]));
    table.add_row(Row::new(vec![
        TableCell::builder("Address")
            .alignment(CellAlignment::Right)
            .build(),
        TableCell::builder(address_str)
            .alignment(CellAlignment::Left)
            .build(),
    ]));
    table.add_row(Row::new(vec![
        TableCell::builder("RPC URL")
            .alignment(CellAlignment::Right)
            .build(),
        TableCell::builder(rpc_url.to_owned())
            .alignment(CellAlignment::Left)
            .build(),
    ]));
    table.add_row(Row::new(vec![TableCell::builder(
        "❗ Please fund this address with ETH ❗",
    )
    .col_span(2)
    .alignment(CellAlignment::Center)
    .build()]));
    table.add_row(Row::new(vec![TableCell::builder(
        "Please fund this address with ETH to continue.",
    )
    .col_span(2)
    .alignment(CellAlignment::Center)
    .build()]));

    warn!("\n{}", table.render());
}

/// Display a success banner showing wallet loaded with funds
pub fn display_wallet_loaded_banner(address: Address, balance: U256, rpc_url: &str) {
    let eth_balance = ethers::utils::format_ether(balance);
    let address_str = format!("{:?}", address);
    let balance_str = format!("{} ETH", eth_balance);

    let mut table = Table::new();
    table.style = TableStyle::extended();
    table.add_row(Row::new(vec![TableCell::builder(
        "🎉 WALLET LOADED SUCCESSFULLY 🎉",
    )
    .col_span(2)
    .alignment(CellAlignment::Center)
    .build()]));
    table.add_row(Row::new(vec![
        TableCell::builder("Address")
            .alignment(CellAlignment::Right)
            .build(),
        TableCell::builder(address_str)
            .alignment(CellAlignment::Left)
            .build(),
    ]));
    table.add_row(Row::new(vec![
        TableCell::builder("Balance")
            .alignment(CellAlignment::Right)
            .build(),
        TableCell::builder(balance_str)
            .alignment(CellAlignment::Left)
            .build(),
    ]));
    table.add_row(Row::new(vec![
        TableCell::builder("RPC URL")
            .alignment(CellAlignment::Right)
            .build(),
        TableCell::builder(rpc_url.to_owned())
            .alignment(CellAlignment::Left)
            .build(),
    ]));
    table.add_row(Row::new(vec![TableCell::builder("✅ Ready to operate")
        .col_span(2)
        .alignment(CellAlignment::Center)
        .build()]));

    info!("\n{}", table.render());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_wallet() {
        let wallet = generate_wallet().unwrap();
        assert_eq!(wallet.address().as_bytes().len(), 20);
    }

    #[test]
    fn test_load_wallet() {
        let private_key = "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
        let wallet = load_wallet(private_key).unwrap();

        // This is the known address for this private key
        let expected_address = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
            .parse::<Address>()
            .unwrap();
        assert_eq!(wallet.address(), expected_address);
    }

    #[test]
    fn test_load_wallet_without_prefix() {
        let private_key = "59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d";
        let wallet = load_wallet(private_key).unwrap();

        let expected_address = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
            .parse::<Address>()
            .unwrap();
        assert_eq!(wallet.address(), expected_address);
    }
}
