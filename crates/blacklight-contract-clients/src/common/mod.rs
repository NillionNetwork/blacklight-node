//! Common utilities for blacklight contract clients.
//!
//! This module re-exports shared utilities from `contract-clients-common`
//! and provides blacklight-specific extensions for error decoding.

// Re-export shared modules
pub use contract_clients_common::event_helper;
pub use contract_clients_common::tx_submitter;

// Provide blacklight-specific errors module with StakingOperators error support
pub mod errors;

use alloy::{
    contract::{CallBuilder, CallDecoder},
    providers::Provider,
};
use anyhow::anyhow;

use crate::common::errors::decode_any_error;

/// Estimate gas for a contract call with a 50% buffer.
///
/// Uses blacklight-specific error decoding for better error messages.
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
