use anyhow::{Context, Result};
use ethers::prelude::*;
use ethers::signers::LocalWallet;
use std::str::FromStr;
use term_table::row::Row;
use term_table::table_cell::{Alignment as CellAlignment, TableCell};
use term_table::{Table, TableStyle};
use tracing::{info, warn};

/// Wallet validation status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalletStatus {
    /// Wallet was just created, needs funding
    Created,
    /// Wallet has no ETH balance
    InsufficientFunds,
    /// Wallet is ready to operate
    Ready,
}

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

/// Display wallet status banner with ETH balance and TEST stake information
/// This consolidated function handles all wallet states with a single implementation
pub fn display_wallet_status(
    status: WalletStatus,
    address: Address,
    rpc_url: &str,
    eth_balance: U256,
    staked_balance: U256,
) {
    let address_str = format!("{:?}", address);
    let eth_formatted = ethers::utils::format_ether(eth_balance);
    let staked_formatted = ethers::utils::format_ether(staked_balance);
    let has_stake = staked_balance > U256::zero();

    let mut table = Table::new();
    table.style = TableStyle::extended();

    // Header row based on status
    let (header, use_warn) = match status {
        WalletStatus::Created => ("✅  Account Created Successfully ✅", true),
        WalletStatus::InsufficientFunds => ("❌  INSUFFICIENT FUNDS  ❌", true),
        WalletStatus::Ready => ("🎉 WALLET LOADED SUCCESSFULLY 🎉", false),
    };
    table.add_row(Row::new(vec![TableCell::builder(header)
        .col_span(2)
        .alignment(CellAlignment::Center)
        .build()]));

    // Address row
    table.add_row(Row::new(vec![
        TableCell::builder("Address")
            .alignment(CellAlignment::Right)
            .build(),
        TableCell::builder(address_str)
            .alignment(CellAlignment::Left)
            .build(),
    ]));

    // ETH Balance row (always shown)
    table.add_row(Row::new(vec![
        TableCell::builder("ETH Balance")
            .alignment(CellAlignment::Right)
            .build(),
        TableCell::builder(format!("{} ETH", eth_formatted))
            .alignment(CellAlignment::Left)
            .build(),
    ]));

    // TEST Staked row (always shown)
    table.add_row(Row::new(vec![
        TableCell::builder("TEST Staked")
            .alignment(CellAlignment::Right)
            .build(),
        TableCell::builder(format!("{} TEST", staked_formatted))
            .alignment(CellAlignment::Left)
            .build(),
    ]));

    // RPC URL row
    table.add_row(Row::new(vec![
        TableCell::builder("RPC URL")
            .alignment(CellAlignment::Right)
            .build(),
        TableCell::builder(rpc_url.to_owned())
            .alignment(CellAlignment::Left)
            .build(),
    ]));

    // Status message based on wallet status and stake
    let status_message = match status {
        WalletStatus::Created => {
            "❗ Please fund this address with ETH and stake TEST tokens to continue ❗"
        }
        WalletStatus::InsufficientFunds => {
            "❗ Please fund this address with ETH and stake TEST tokens to continue ❗"
        }
        WalletStatus::Ready => {
            if has_stake {
                "✅ Ready to operate"
            } else {
                "⚠️  Please stake TEST tokens to continue"
            }
        }
    };
    table.add_row(Row::new(vec![TableCell::builder(status_message)
        .col_span(2)
        .alignment(CellAlignment::Center)
        .build()]));

    // Log based on status
    if use_warn {
        warn!("\n{}", table.render());
    } else {
        info!("\n{}", table.render());
    }
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
