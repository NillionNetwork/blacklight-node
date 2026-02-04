use crate::{l2::KeeperState, metrics};
use alloy::hex;
use alloy::primitives::B256;
use alloy::providers::Provider;
use anyhow::Context;
use erc_8004_contract_clients::validation_registry::ValidationRegistryUpgradeable::ValidationRegistryUpgradeableInstance;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

/// Submits validation responses for finalized ERC-8004 validation rounds.
pub struct ValidationResponder<P: Provider + Clone> {
    registry: ValidationRegistryUpgradeableInstance<P>,
    state: Arc<Mutex<KeeperState>>,
}

impl<P: Provider + Clone> ValidationResponder<P> {
    pub fn new(
        registry: ValidationRegistryUpgradeableInstance<P>,
        state: Arc<Mutex<KeeperState>>,
    ) -> Self {
        Self { registry, state }
    }

    /// Process pending validation responses.
    ///
    /// Called from the tick loop to submit validation responses for any
    /// validations that have received an outcome but haven't been responded to yet.
    pub async fn process_responses(&self) -> anyhow::Result<()> {
        // Collect jobs to process outside the lock
        let jobs: Vec<_> = {
            let state = self.state.lock().await;
            let pending_count = state.erc8004.pending_validations.len();
            info!(
                pending_count,
                "Processing ERC-8004 responses: checking tracked validations"
            );

            state
                .erc8004
                .pending_validations
                .iter()
                .filter(|(_, info)| info.outcome.is_some() && !info.response_submitted)
                .map(|(key, info)| (*key, info.request_hash, info.outcome.unwrap()))
                .collect()
        };

        info!(
            ready_count = jobs.len(),
            "Found validations ready for response submission"
        );

        if jobs.is_empty() {
            debug!(
                "No ERC-8004 validations ready for response (waiting for outcomes or already submitted)"
            );
            return Ok(());
        }

        for (heartbeat_key, request_hash, outcome) in jobs {
            info!(
                request_hash = %hex::encode(request_hash),
                outcome,
                "Submitting ERC-8004 validation response"
            );
            match self.submit_response(request_hash, outcome).await {
                Ok(tx_hash) => {
                    // Mark as submitted
                    let mut state = self.state.lock().await;
                    if let Some(info) = state.erc8004.pending_validations.get_mut(&heartbeat_key) {
                        info.response_submitted = true;
                    }
                    metrics::get().erc8004.inc_responses_submitted();
                    info!(
                        request_hash = ?request_hash,
                        outcome,
                        tx_hash = ?tx_hash,
                        "Submitted ERC-8004 validation response"
                    );
                }
                Err(e) => {
                    error!(
                        request_hash = ?request_hash,
                        outcome,
                        "Failed to submit ERC-8004 validation response: {e}"
                    );
                }
            }
        }

        Ok(())
    }

    async fn submit_response(&self, request_hash: B256, outcome: u8) -> anyhow::Result<B256> {
        let pending = self
            .registry
            .validationResponse(
                request_hash,
                outcome,
                String::new(),           // responseURI - empty
                B256::ZERO,              // responseHash - zero
                "heartbeat".to_string(), // tag
            )
            .send()
            .await
            .map_err(|e| {
                error!(
                    request_hash = %request_hash,
                    outcome,
                    error = %e,
                    "validationResponse transaction failed"
                );
                anyhow::anyhow!("Failed to send validationResponse: {e}")
            })?;

        let receipt = pending
            .get_receipt()
            .await
            .context("Failed to get transaction receipt")?;

        Ok(receipt.transaction_hash)
    }
}
