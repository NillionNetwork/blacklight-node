use crate::errors::{DecodedRevert, extract_revert_from_contract_error_with_custom};
use crate::overestimate_gas;
use alloy::{
    consensus::Transaction,
    contract::CallBuilder,
    primitives::{B256, Bytes},
    providers::Provider,
    rpc::types::TransactionReceipt,
};
use anyhow::{Result, bail};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

/// A reusable error decoder function pointer.
/// Takes raw ABI-encoded revert data and returns a decoded error if recognized.
pub type ErrorDecoder = fn(&Bytes) -> Option<DecodedRevert>;

#[derive(Clone)]
pub struct TransactionSubmitter {
    tx_lock: Arc<Mutex<()>>,
    gas_buffer: bool,
    decoder: ErrorDecoder,
}

impl TransactionSubmitter {
    pub fn new(tx_lock: Arc<Mutex<()>>, decoder: ErrorDecoder) -> Self {
        Self {
            tx_lock,
            gas_buffer: false,
            decoder,
        }
    }

    pub async fn invoke<P, D>(&self, method: &str, call: CallBuilder<P, D>) -> Result<B256>
    where
        P: Provider + Clone,
        D: alloy::contract::CallDecoder + Clone,
    {
        // Pre-simulate to catch reverts with proper error messages
        if let Err(e) = call.call().await {
            let e = self.decode_error(e);
            bail!("{method} reverted: {e}");
        }

        let (call, gas_limit) = match self.gas_buffer {
            true => {
                let gas = overestimate_gas(&call, self.decoder).await?;
                (call.gas(gas), Some(gas))
            }
            false => (call, None),
        };

        let provider = call.provider.clone();
        let estimate = provider.estimate_eip1559_fees().await?;

        // Our L2 requires a minimum priority fee of 1 wei
        let priority_fee = 1u128;
        let call = call
            .max_priority_fee_per_gas(priority_fee)
            .max_fee_per_gas(estimate.max_fee_per_gas);

        let estimated_gas = call.clone().estimate_gas().await?;

        // Acquire lock and send
        let _guard = self.tx_lock.lock().await;
        let pending = match call.send().await {
            Ok(pending) => pending,
            Err(e) => {
                let e = self.decode_error(e);
                bail!("{method} failed to send: {e}");
            }
        };

        // Wait for receipt
        let receipt = pending.get_receipt().await?;
        let tx_hash = receipt.transaction_hash;

        Self::log_fee_details(
            &provider,
            method,
            tx_hash,
            &receipt,
            estimated_gas,
            estimate.max_priority_fee_per_gas,
        )
        .await;

        // Validate success
        if !receipt.status() {
            if let Some(gas_limit) = gas_limit {
                let used = receipt.gas_used;
                if used >= gas_limit {
                    bail!(
                        "{method} ran out of gas (used {used} of {gas_limit} limit). Tx: {tx_hash:?}"
                    );
                }
            }

            bail!("{method} reverted on-chain. Tx hash: {tx_hash:?}");
        }

        Ok(tx_hash)
    }

    pub fn with_gas_buffer(&self) -> Self {
        let mut this = self.clone();
        this.gas_buffer = true;
        this
    }

    fn decode_error(&self, error: alloy::contract::Error) -> String {
        extract_revert_from_contract_error_with_custom(&error, self.decoder).to_string()
    }

    async fn log_fee_details<P: Provider + Clone>(
        provider: &P,
        method: &str,
        tx_hash: B256,
        receipt: &TransactionReceipt,
        estimated_gas: u64,
        estimated_priority_fee: u128,
    ) {
        // Fetch actual transaction to get the real fee parameters
        let (tx_max_fee, tx_max_priority_fee) =
            match provider.get_transaction_by_hash(tx_hash).await {
                Ok(Some(tx)) => (Some(tx.max_fee_per_gas()), tx.max_priority_fee_per_gas()),
                _ => (None, None),
            };

        // Calculate actual priority fee paid: effective_gas_price - base_fee
        let actual_priority_fee = if let Some(block_num) = receipt.block_number {
            provider
                .get_block_by_number(block_num.into())
                .await
                .ok()
                .flatten()
                .and_then(|b| b.header.base_fee_per_gas)
                .map(|base_fee| receipt.effective_gas_price.saturating_sub(base_fee as u128))
        } else {
            None
        };

        let total_cost = receipt.effective_gas_price * receipt.gas_used as u128;
        let actual_priority_fee = actual_priority_fee.unwrap_or(0);
        if actual_priority_fee < estimated_priority_fee.saturating_sub(1_000_000_000u128) {
            warn!(
                method = %method,
                tx_hash = ?tx_hash,
                effective_gas_price = receipt.effective_gas_price,
                gas_used = receipt.gas_used,
                estimated_gas = ?estimated_gas,
                total_cost,
                tx_max_fee = ?tx_max_fee,
                tx_max_priority_fee = ?tx_max_priority_fee,
                actual_priority_fee = ?actual_priority_fee,
                estimated_priority_fee = ?estimated_priority_fee,
                "💰 transaction gas details (priority fee may be too low)"
            );
        } else {
            info!(
                method = %method,
                tx_hash = ?tx_hash,
                effective_gas_price = receipt.effective_gas_price,
                gas_used = receipt.gas_used,
                estimated_gas = ?estimated_gas,
                total_cost,
                tx_max_fee = ?tx_max_fee,
                tx_max_priority_fee = ?tx_max_priority_fee,
                actual_priority_fee = ?actual_priority_fee,
                estimated_priority_fee = ?estimated_priority_fee,
                "💰 transaction gas details"
            );
        }
    }
}
