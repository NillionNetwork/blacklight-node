//! Round-starter duty (N6, C4a): crank `startRound` for enqueued heartbeats whose
//! round 1 has not started. The keeper holds ROUND_STARTER_ROLE so rounds start
//! promptly; anyone can crank after `startRoundDelay`, so this duty is a subsidy,
//! not a gatekeeper.

use crate::{clients::L2KeeperClient, l2::KeeperState, metrics};
use alloy::primitives::{B256, Bytes};
use contract_clients_common::chain_profile::FeeStrategy;
use contract_clients_common::tx_submitter::TransactionSubmitter;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::Mutex;
use tracing::{info, warn};

/// Heartbeat status enum value for `Pending` in HeartbeatManager.
const STATUS_PENDING: u8 = 1;

pub(crate) struct RoundStarter {
    client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
    submitter: TransactionSubmitter,
    /// Heartbeats confirmed on-chain as no longer startable (started, finalized, or
    /// expired) — skipped without further chain reads.
    settled: HashSet<B256>,
}

impl RoundStarter {
    pub(crate) fn new(
        client: Arc<L2KeeperClient>,
        state: Arc<Mutex<KeeperState>>,
        fee_strategy: FeeStrategy,
    ) -> Self {
        let submitter = TransactionSubmitter::new(
            client.tx_lock(),
            blacklight_contract_clients::errors::blacklight_error_decoder,
        )
        .with_fee_strategy(fee_strategy);
        Self {
            client,
            state,
            submitter,
            settled: HashSet::new(),
        }
    }

    pub(crate) async fn process_round_starts(&mut self) -> anyhow::Result<()> {
        let candidates = {
            let state = self.state.lock().await;
            collect_start_candidates(&state, &self.settled)
        };

        for (heartbeat_key, raw_htx) in candidates {
            // State-machine guard: confirm on-chain that round 1 has not started (covers
            // rounds started by other crankers or before our lookback window).
            let hb = self
                .client
                .heartbeat_manager()
                .heartbeats(heartbeat_key)
                .call()
                .await;
            let (status, current_round) = match hb {
                Ok(hb) => (hb.status, hb.currentRound),
                Err(e) => {
                    warn!(heartbeat_key = ?heartbeat_key, error = %e, "Failed to read heartbeat state");
                    continue;
                }
            };
            if status != STATUS_PENDING || current_round != 0 {
                self.settled.insert(heartbeat_key);
                continue;
            }

            info!(heartbeat_key = ?heartbeat_key, "Starting round 1");
            let call = self
                .client
                .heartbeat_manager()
                .startRound(heartbeat_key, raw_htx);
            match self.submitter.invoke("startRound", call).await {
                Ok(tx_hash) => {
                    info!(
                        heartbeat_key = ?heartbeat_key,
                        tx_hash = ?tx_hash,
                        "Round started"
                    );
                    self.settled.insert(heartbeat_key);
                    metrics::get().l2.escalations.inc_round_starts();
                }
                Err(e) => {
                    // RoundAlreadyStarted from a racing cranker is benign; anything else
                    // is retried next tick.
                    warn!(heartbeat_key = ?heartbeat_key, error = %e, "startRound failed");
                }
            }
        }

        Ok(())
    }
}

/// Enqueued heartbeats with no round 1 observed: these are the startRound candidates.
fn collect_start_candidates(state: &KeeperState, settled: &HashSet<B256>) -> Vec<(B256, Bytes)> {
    let started: HashSet<B256> = state.rounds.keys().map(|key| key.heartbeat_key).collect();

    let mut candidates: HashMap<B256, Bytes> = HashMap::new();
    for (heartbeat_key, raw_htx) in state.raw_htx_by_heartbeat.iter() {
        if started.contains(heartbeat_key) || settled.contains(heartbeat_key) {
            continue;
        }
        candidates.insert(*heartbeat_key, raw_htx.clone());
    }
    candidates.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::l2::{KeeperState, RoundKey, RoundState};

    fn key(byte: u8) -> B256 {
        B256::repeat_byte(byte)
    }

    #[test]
    fn enqueued_without_round_is_a_candidate() {
        let mut state = KeeperState::default();
        state
            .raw_htx_by_heartbeat
            .insert(key(1), Bytes::from(vec![1, 2, 3]));

        let candidates = collect_start_candidates(&state, &HashSet::new());
        assert_eq!(candidates, vec![(key(1), Bytes::from(vec![1, 2, 3]))]);
    }

    #[test]
    fn started_round_is_not_double_cranked() {
        let mut state = KeeperState::default();
        state
            .raw_htx_by_heartbeat
            .insert(key(1), Bytes::from(vec![1]));
        state.rounds.insert(
            RoundKey {
                heartbeat_key: key(1),
                round: 1,
            },
            RoundState::default(),
        );

        let candidates = collect_start_candidates(&state, &HashSet::new());
        assert!(
            candidates.is_empty(),
            "started heartbeat must not be cranked"
        );
    }

    #[test]
    fn settled_heartbeats_are_skipped() {
        let mut state = KeeperState::default();
        state
            .raw_htx_by_heartbeat
            .insert(key(1), Bytes::from(vec![1]));
        let mut settled = HashSet::new();
        settled.insert(key(1));

        let candidates = collect_start_candidates(&state, &settled);
        assert!(candidates.is_empty());
    }

    #[test]
    fn escalated_rounds_do_not_reappear_as_candidates() {
        // a heartbeat with round 2 tracked (escalation) is by definition started
        let mut state = KeeperState::default();
        state
            .raw_htx_by_heartbeat
            .insert(key(2), Bytes::from(vec![9]));
        state.rounds.insert(
            RoundKey {
                heartbeat_key: key(2),
                round: 2,
            },
            RoundState::default(),
        );

        let candidates = collect_start_candidates(&state, &HashSet::new());
        assert!(candidates.is_empty());
    }
}
