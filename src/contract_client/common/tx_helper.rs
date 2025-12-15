//! # Transaction Helper
//!
//! This module provides common transaction submission patterns to reduce boilerplate
//! across contract clients. It handles:
//! - Pre-simulation to catch reverts with proper error messages
//! - Transaction locking to prevent nonce conflicts
//! - Receipt validation and error extraction
//!
//! ## Usage
//!
//! ```ignore
//! use crate::contract_client::tx_helper::send_and_confirm;
//!
//! let call = self.contract.someMethod(args);
//! let tx_hash = send_and_confirm(call, &self.tx_lock, "someMethod").await?;
//! ```

use alloy::{primitives::B256, providers::Provider};
use anyhow::{anyhow, Error, Result};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Send a transaction and wait for confirmation.
///
/// This is the standard pattern for most contract methods:
/// 1. Pre-simulate to catch reverts with proper error messages
/// 2. Acquire transaction lock to prevent nonce conflicts
/// 3. Send the transaction
/// 4. Wait for receipt and validate success
///
/// # Arguments
///
/// * `call` - The contract call builder
/// * `tx_lock` - Mutex to prevent concurrent transaction sending
/// * `method_name` - Name of the method for error messages
///
/// # Returns
///
/// The transaction hash on success, or an error with decoded revert reason.
pub async fn send_and_confirm<P, D>(
    call: alloy::contract::CallBuilder<P, D>,
    tx_lock: &Arc<Mutex<()>>,
    method_name: &str,
) -> Result<B256>
where
    P: Provider + Clone,
    D: alloy::contract::CallDecoder + Clone,
{
    // Pre-simulate to catch reverts with proper error messages
    if let Err(e) = call.call().await {
        let decoded = super::errors::decode_any_error(&e);
        return Err(anyhow!("{} reverted: {}", method_name, decoded));
    }

    // Acquire lock and send
    let _guard = tx_lock.lock().await;
    let pending = call.send().await.map_err(|e| {
        let decoded = super::errors::decode_any_error(&e);
        anyhow!("{} failed to send: {}", method_name, decoded)
    })?;

    // Wait for receipt
    let receipt = pending.get_receipt().await?;

    // Validate success
    if !receipt.status() {
        return Err(anyhow!(
            "{} reverted on-chain. Tx hash: {:?}",
            method_name,
            receipt.transaction_hash
        ));
    }

    Ok(receipt.transaction_hash)
}

/// Send a transaction with custom gas limit and wait for confirmation.
///
/// Similar to `send_and_confirm`, but allows specifying a custom gas limit.
/// Useful when gas estimation might be inaccurate (e.g., variable node selection).
///
/// # Arguments
///
/// * `call` - The contract call builder
/// * `tx_lock` - Mutex to prevent concurrent transaction sending
/// * `method_name` - Name of the method for error messages
/// * `gas_limit` - Custom gas limit to use
///
/// # Returns
///
/// The transaction hash on success, or an error with decoded revert reason.
pub async fn send_with_gas_and_confirm<P, D>(
    call: alloy::contract::CallBuilder<P, D>,
    tx_lock: &Arc<Mutex<()>>,
    method_name: &str,
    gas_limit: u64,
) -> Result<B256>
where
    P: Provider + Clone,
    D: alloy::contract::CallDecoder + Clone,
{
    // Pre-simulate to catch reverts with proper error messages
    if let Err(e) = call.call().await {
        let decoded = super::errors::decode_any_error(&e);
        return Err(anyhow!("{} reverted: {}", method_name, decoded));
    }

    // Apply gas limit
    let call_with_gas = call.gas(gas_limit);

    // Acquire lock and send
    let _guard = tx_lock.lock().await;
    let pending = call_with_gas.send().await.map_err(|e| {
        let decoded = super::errors::decode_any_error(&e);
        anyhow!("{} failed to send: {}", method_name, decoded)
    })?;

    // Wait for receipt
    let receipt = pending.get_receipt().await?;

    // Validate success
    if !receipt.status() {
        let gas_used = receipt.gas_used;
        if gas_used >= gas_limit {
            return Err(anyhow!(
                "{} ran out of gas (used {} of {} limit). Tx: {:?}",
                method_name,
                gas_used,
                gas_limit,
                receipt.transaction_hash
            ));
        }
        return Err(anyhow!(
            "{} reverted on-chain. Tx hash: {:?}",
            method_name,
            receipt.transaction_hash
        ));
    }

    Ok(receipt.transaction_hash)
}

/// Decode contract errors into human-readable messages.
///
/// This is the shared error decoder previously duplicated in each client.
/// It handles common error patterns from RPC providers and decodes
/// Solidity revert data when available.
///
/// # Arguments
///
/// * `e` - Any error that implements Display and Debug
///
/// # Returns
///
/// An anyhow::Error with a human-readable message.
pub fn decode_error<E: std::fmt::Display + std::fmt::Debug>(e: E) -> Error {
    let error_str = e.to_string();
    let decoded = super::errors::decode_any_error(&e);

    // If we successfully decoded a revert, use that
    if !matches!(decoded, super::errors::DecodedRevert::NoRevertData(_)) {
        return anyhow!("Contract reverted: {}", decoded);
    }

    // Common error patterns from RPC providers
    if error_str.contains("insufficient funds") {
        anyhow!("Insufficient ETH for gas. Please fund the account.")
    } else if error_str.contains("replacement transaction underpriced") {
        anyhow!("Transaction underpriced. A pending transaction may be blocking.")
    } else if error_str.contains("nonce too low") {
        anyhow!("Nonce too low. A transaction may have been confirmed already.")
    } else {
        anyhow!("Transaction failed: {}", e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_error_insufficient_funds() {
        let err = std::io::Error::other("insufficient funds for gas");
        let result = decode_error(err);
        assert!(result.to_string().contains("Insufficient ETH"));
    }

    #[test]
    fn test_decode_error_nonce_too_low() {
        let err = std::io::Error::other("nonce too low");
        let result = decode_error(err);
        assert!(result.to_string().contains("Nonce too low"));
    }

    #[test]
    fn test_decode_error_underpriced() {
        let err = std::io::Error::other("replacement transaction underpriced");
        let result = decode_error(err);
        assert!(result.to_string().contains("underpriced"));
    }
}
