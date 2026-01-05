use alloy::{
    contract::CallBuilder, primitives::B256, providers::Provider, sol_types::SolInterface,
};
use anyhow::{anyhow, Result};
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::Mutex;

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

        // Acquire lock and send
        let _guard = self.tx_lock.lock().await;
        let pending = call.send().await.map_err(|e| {
            let e = self.decode_error(e);
            anyhow!("{method} failed to send: {e}")
        })?;

        // Wait for receipt
        let receipt = pending.get_receipt().await?;
        let tx_hash = receipt.transaction_hash;

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
}
