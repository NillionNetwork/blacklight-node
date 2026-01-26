use alloy::{
    contract::{CallBuilder, CallDecoder},
    providers::Provider,
};
use anyhow::anyhow;

use crate::contract_client::common::errors::decode_any_error;

pub mod errors;
pub mod event_helper;
pub mod tx_submitter;

pub async fn overestimate_gas<P: Provider, D: CallDecoder>(
    call: &CallBuilder<&P, D>,
) -> anyhow::Result<u64> {
    // Estimate gas and add a 50% buffer
    let estimated_gas = call.estimate_gas().await.map_err(|e| {
        let decoded = decode_any_error(&e);
        anyhow!("failed to estimate gas: {decoded}")
    })?;
    let gas_with_buffer = estimated_gas.saturating_add(estimated_gas / 2);
    Ok(gas_with_buffer)
}
