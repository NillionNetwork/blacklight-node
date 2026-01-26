use alloy::{
    consensus::Transaction, contract::CallBuilder, primitives::B256, providers::Provider,
    rpc::types::TransactionReceipt, sol_types::SolInterface,
};
use anyhow::{anyhow, Result};
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

#[derive(Clone)]
pub(crate) struct TransactionSubmitter<S> {
    tx_lock: Arc<Mutex<()>>,
    gas_limit: Option<u64>,
    _decoder: PhantomData<S>,
}

impl<S: SolInterface + Debug + Clone> TransactionSubmitter<S> {
    pub(crate) fn new(tx_lock: Arc<Mutex<()>>) -> Self {
        Self {
            tx_lock,
            gas_limit: None,
            _decoder: PhantomData,
        }
    }

    pub(crate) async fn invoke<P, D>(&self, method: &str, call: CallBuilder<P, D>) -> Result<B256>
    where
        P: Provider + Clone,
        D: alloy::contract::CallDecoder + Clone,
    {
        // Pre-simulate to catch reverts with proper error messages
        if let Err(e) = call.call().await {
            let e = self.decode_error(e);
            return Err(anyhow!("{method} reverted: {e}"));
        }

        let call = match self.gas_limit {
            Some(gas) => call.gas(gas),
            None => call,
        };

        let provider = call.provider.clone();
        let gas_price = provider.get_gas_price().await?;

        let priority_fee = 0u128;
        let call = call
            .max_priority_fee_per_gas(priority_fee)
            .max_fee_per_gas(gas_price);
        let estimated_gas = call.clone().estimate_gas().await?;

        // Acquire lock and send
        let _guard = self.tx_lock.lock().await;
        let pending = call.send().await.map_err(|e| {
            let e = self.decode_error(e);
            anyhow!("{method} failed to send: {e}")
        })?;

        // Wait for receipt
        let receipt = pending.get_receipt().await?;
        let tx_hash = receipt.transaction_hash;

        Self::log_fee_details(&provider, method, tx_hash, &receipt, estimated_gas).await;

        // Validate success
        if !receipt.status() {
            if let Some(limit) = self.gas_limit {
                let used = receipt.gas_used;
                if used >= limit {
                    return Err(anyhow!(
                        "{method} ran out of gas (used {used} of {limit} limit). Tx: {tx_hash:?}"
                    ));
                }
            }

            return Err(anyhow!("{method} reverted on-chain. Tx hash: {tx_hash:?}"));
        }

        Ok(tx_hash)
    }

    pub(crate) fn with_gas_limit(&self, limit: u64) -> Self {
        let mut this = self.clone();
        this.gas_limit = Some(limit);
        this
    }

    fn decode_error(&self, error: alloy::contract::Error) -> String {
        match error.try_decode_into_interface_error::<S>() {
            Ok(error) => format!("{error:?}"),
            Err(error) => super::errors::decode_any_error(&error).to_string(),
        }
    }

    async fn log_fee_details<P: Provider + Clone>(
        provider: &P,
        method: &str,
        tx_hash: B256,
        receipt: &TransactionReceipt,
        estimated_gas: u64,
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

        if actual_priority_fee.unwrap_or(0) > 0 {
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
                "💰 transaction gas details (priority fee detected)"
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
                "💰 transaction gas details"
            );
        }
    }
}
