//! # Contract Clients Common
//!
//! Shared utilities for Ethereum contract clients using Alloy.
//!
//! This crate provides:
//! - **Error decoding**: Human-readable Solidity revert errors
//! - **Event helpers**: Utilities for event listening and querying
//! - **Transaction submission**: Reliable transaction submission with gas estimation
//!
//! ## Usage
//!
//! ```ignore
//! use contract_clients_common::{
//!     errors::{decode_any_error, DecodedRevert},
//!     event_helper::BlockRange,
//!     tx_submitter::TransactionSubmitter,
//! };
//! ```

use alloy::{
    contract::{CallBuilder, CallDecoder},
    providers::Provider,
};
use anyhow::anyhow;

use crate::errors::decode_any_error;

pub mod errors;
pub mod event_helper;
pub mod tx_submitter;

/// Estimate gas for a contract call with a 50% buffer.
///
/// This is useful for ensuring transactions have enough gas headroom,
/// especially for complex operations that may use more gas than estimated.
///
/// # Arguments
///
/// * `call` - The contract call to estimate gas for
///
/// # Returns
///
/// The estimated gas with a 50% buffer added.
pub async fn overestimate_gas<P: Provider, D: CallDecoder>(
    call: &CallBuilder<P, D>,
) -> anyhow::Result<u64> {
    // Estimate gas and add a 50% buffer
    let estimated_gas = call.estimate_gas().await.map_err(|e| {
        let decoded = decode_any_error(&e);
        anyhow!("failed to estimate gas: {decoded}")
    })?;
    let gas_with_buffer = estimated_gas.saturating_add(estimated_gas / 2);
    Ok(gas_with_buffer)
}
