use crate::{clients::L2KeeperClient, l2::KeeperState, metrics};
use alloy::primitives::{B256, Bytes};
use blacklight_contract_clients::heartbeat_manager::HeartbeatManagerErrors;
use contract_clients_common::errors::decode_any_error;
use contract_clients_common::tx_submitter::TransactionSubmitter;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use tracing::{info, warn};

pub(crate) struct RoundEscalator {
    client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
    submitter: TransactionSubmitter<HeartbeatManagerErrors>,
}

impl RoundEscalator {
    pub(crate) fn new(client: Arc<L2KeeperClient>, state: Arc<Mutex<KeeperState>>) -> Self {
        let submitter = TransactionSubmitter::new(client.tx_lock());
        Self {
            client,
            state,
            submitter,
        }
    }

    pub(crate) async fn process_escalations(&self, block_timestamp: u64) -> anyhow::Result<()> {
        let candidates = {
            let state = self.state.lock().await;
            let mut best_rounds: HashMap<B256, (u8, u64, Bytes)> = HashMap::new();

            for (key, round) in state.rounds.iter() {
                if round.outcome.is_some() {
                    continue;
                }
                if block_timestamp <= round.deadline {
                    continue;
                }
                let deadline = round.deadline;
                let raw_htx = round.raw_htx.clone();
                let entry = best_rounds
                    .entry(key.heartbeat_key)
                    .or_insert_with(|| (key.round, deadline, raw_htx.clone()));
                if key.round > entry.0 {
                    *entry = (key.round, deadline, raw_htx);
                }
            }
            best_rounds
        };

        for (heartbeat_key, (round, deadline, raw_htx)) in candidates {
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

            match self.submitter.invoke("escalateOrExpire", call).await {
                Ok(tx_hash) => {
                    info!(
                        heartbeat_key = ?heartbeat_key,
                        tx_hash = ?tx_hash,
                        "Escalate/expire confirmed"
                    );
                    metrics::get().l2.escalations.inc_escalations();
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
