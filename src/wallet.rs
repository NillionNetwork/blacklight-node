use alloy::primitives::{Address, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::signers::local::PrivateKeySigner;
use anyhow::{Context, Result};
use term_table::row::Row;
use term_table::table_cell::{Alignment as CellAlignment, TableCell};
use term_table::{Table, TableStyle};
use tracing::{info, warn};

fn format_eth_clean(amount: U256) -> String {
    let formatted = alloy::primitives::utils::format_ether(amount);
    // Trim trailing zeros after decimal point
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_string()
}
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
pub fn generate_wallet() -> Result<PrivateKeySigner> {
    let wallet = PrivateKeySigner::random();
    Ok(wallet)
}

/// Load wallet from private key string (with or without 0x prefix)
pub fn load_wallet(private_key: &str) -> Result<PrivateKeySigner> {
    let wallet = private_key
        .parse::<PrivateKeySigner>()
        .context("Failed to parse private key")?;
    Ok(wallet)
}

/// Check if an address has funds on the given RPC endpoint
pub async fn check_balance(rpc_url: &str, address: Address) -> Result<U256> {
    let provider =
        ProviderBuilder::new().connect_http(rpc_url.parse().context("Failed to parse RPC URL")?);

    let balance = provider
        .get_balance(address)
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
    min_eth_balance: U256,
) {
    let address_str = format!("{:?}", address);
    let eth_formatted = alloy::primitives::utils::format_ether(eth_balance);
    let staked_formatted = alloy::primitives::utils::format_ether(staked_balance);
    let min_eth_formatted = format_eth_clean(min_eth_balance);
    let has_sufficient_eth = eth_balance >= min_eth_balance;
    let has_stake = staked_balance > U256::ZERO;

    let eth_balance_str = if has_sufficient_eth {
        format!("{} ETH ✅", eth_formatted)
    } else {
        format!("{} ETH ❌ (min: {} ETH)", eth_formatted, min_eth_formatted)
    };
    let staked_balance_str = if has_stake {
        format!("{} TEST ✅", staked_formatted)
    } else {
        format!("{} TEST ❌", staked_formatted)
    };

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
        TableCell::builder(eth_balance_str)
            .alignment(CellAlignment::Left)
            .build(),
    ]));

    // TEST Staked row (always shown)
    table.add_row(Row::new(vec![
        TableCell::builder("TEST Staked")
            .alignment(CellAlignment::Right)
            .build(),
        TableCell::builder(staked_balance_str)
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
            format!("❗ Please fund this address with at least {} ETH and stake TEST tokens to continue ❗", min_eth_formatted)
        }
        WalletStatus::InsufficientFunds => {
            if !has_stake && !has_sufficient_eth {
                format!("❗ Please fund this address with at least {} ETH and stake TEST tokens to continue ❗", min_eth_formatted)
            } else if !has_stake {
                "⚠️  Please stake TEST tokens to continue".to_string()
            } else {
                format!(
                    "⚠️  Please fund this address with at least {} ETH for gas transactions",
                    min_eth_formatted
                )
            }
        }
        WalletStatus::Ready => "✅ Ready to operate".to_string(),
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
        assert_eq!(wallet.address().len(), 20);
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
