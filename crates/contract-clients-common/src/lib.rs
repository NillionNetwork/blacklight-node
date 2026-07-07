//! # Contract Clients Common
//!
//! Shared utilities for Ethereum contract clients using Alloy.
//!
//! This crate provides:
//! - **Error decoding**: Human-readable Solidity revert errors
//! - **Event helpers**: Utilities for event listening and querying
//! - **Transaction submission**: Reliable transaction submission with gas estimation

use alloy::{
    contract::{CallBuilder, CallDecoder},
    providers::Provider,
};
use anyhow::anyhow;

use crate::errors::extract_revert_from_contract_error_with_custom;
use crate::tx_submitter::ErrorDecoder;

pub mod chain_profile;
pub mod errors;
pub mod event_helper;
pub mod provider_context;
pub mod tx_submitter;

pub use chain_profile::{ChainProfile, FeeStrategy};
pub use provider_context::ProviderContext;

/// Estimate gas for a contract call with a 50% buffer.
pub async fn overestimate_gas<P: Provider, D: CallDecoder>(
    call: &CallBuilder<P, D>,
    decoder: ErrorDecoder,
) -> anyhow::Result<u64> {
    let estimated_gas = call.estimate_gas().await.map_err(|e| {
        let decoded = extract_revert_from_contract_error_with_custom(&e, decoder);
        anyhow!("failed to estimate gas: {decoded}")
    })?;
    let gas_with_buffer = estimated_gas.saturating_add(estimated_gas / 2);
    Ok(gas_with_buffer)
}
