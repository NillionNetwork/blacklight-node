use crate::{clients::L2KeeperClient, l2::KeeperState};
use alloy::primitives::{B256, Bytes};
use blacklight_contract_clients::common::errors::decode_any_error;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use tracing::{info, warn};

pub(crate) struct RoundEscalator {
    client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
}

impl RoundEscalator {
    pub(crate) fn new(client: Arc<L2KeeperClient>, state: Arc<Mutex<KeeperState>>) -> Self {
        Self { client, state }
    }

    pub(crate) async fn process_escalations(&self, block_timestamp: u64) -> anyhow::Result<()> {
        let (candidates, fallback_heartbeats) = {
            let state = self.state.lock().await;
            let mut best_rounds: HashMap<B256, (u8, u64, Bytes)> = HashMap::new();
            let mut fallback = Vec::new();

            for (key, round) in state.rounds.iter() {
                if round.outcome.is_some() {
                    continue;
                }
                let deadline = match round.deadline {
                    Some(value) => value,
                    None => continue,
                };
                let raw_htx = round
                    .raw_htx
                    .clone()
                    .or_else(|| state.raw_htx_by_heartbeat.get(&key.heartbeat_key).cloned());
                let Some(raw_htx) = raw_htx else {
                    continue;
                };

                let entry = best_rounds
                    .entry(key.heartbeat_key)
                    .or_insert_with(|| (key.round, deadline, raw_htx.clone()));
                if key.round > entry.0 {
                    *entry = (key.round, deadline, raw_htx);
                }
            }

            if best_rounds.is_empty() {
                fallback = state
                    .raw_htx_by_heartbeat
                    .iter()
                    .map(|(k, v)| (*k, v.clone()))
                    .collect();
            }

            (
                best_rounds
                    .into_iter()
                    .map(|(k, (round, deadline, raw_htx))| (k, round, deadline, raw_htx))
                    .collect::<Vec<_>>(),
                fallback,
            )
        };

        for (heartbeat_key, round, deadline, raw_htx) in candidates {
            if block_timestamp <= deadline {
                continue;
            }

            info!(
                heartbeat_key = ?heartbeat_key,
                round,
                deadline,
                "Escalating or expiring round"
            );
            let call = self
                .client
                .heartbeat_manager()
                .escalateOrExpire(heartbeat_key, raw_htx.clone());

            match call.send().await {
                Ok(pending) => {
                    let receipt = pending.get_receipt().await?;
                    info!(
                        heartbeat_key = ?heartbeat_key,
                        tx_hash = ?receipt.transaction_hash,
                        "Escalate/expire confirmed"
                    );
                }
                Err(e) => {
                    warn!(
                        heartbeat_key = ?heartbeat_key,
                        error = %decode_any_error(&e),
                        "Escalate/expire failed"
                    );
                }
            }
        }

        for (heartbeat_key, raw_htx) in fallback_heartbeats {
            let should_escalate = self
                .client
                .heartbeat_manager()
                .isPastDeadline(heartbeat_key)
                .call()
                .await?;
            if !should_escalate {
                continue;
            }

            info!(heartbeat_key = ?heartbeat_key, "Escalating or expiring round");
            let call = self
                .client
                .heartbeat_manager()
                .escalateOrExpire(heartbeat_key, raw_htx.clone());

            match call.send().await {
                Ok(pending) => {
                    let receipt = pending.get_receipt().await?;
                    info!(
                        heartbeat_key = ?heartbeat_key,
                        tx_hash = ?receipt.transaction_hash,
                        "Escalate/expire confirmed"
                    );
                }
                Err(e) => {
                    warn!(
                        heartbeat_key = ?heartbeat_key,
                        error = %decode_any_error(&e),
                        "Escalate/expire failed"
                    );
                }
            }
        }

        Ok(())
    }
}
