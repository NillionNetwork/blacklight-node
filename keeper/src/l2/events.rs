use crate::{
    clients::HeartbeatManagerInstance,
    erc8004::{ValidationRequestInfo, events::on_round_finalized},
    l2::{KeeperState, RoundKey},
    metrics,
};
use alloy::{primitives::B256, rpc::types::Log, sol_types::SolEvent};
use anyhow::Context;
use blacklight_contract_clients::{
    HeartbeatManager::SlashingCallbackFailed,
    heartbeat_manager::{
        HeartbeatEnqueuedEvent, RewardDistributionAbandonedEvent, RewardsDistributedEvent,
        RoundFinalizedEvent, RoundStartedEvent, SlashingCallbackFailedEvent,
    },
    htx::Erc8004Htx,
};
use futures_util::{Stream, StreamExt};
use std::{pin::pin, sync::Arc};
use tokio::sync::Mutex;
use tracing::{debug, error, info};

pub(crate) struct EventListener {
    manager: HeartbeatManagerInstance,
}

impl EventListener {
    pub(crate) fn new(manager: HeartbeatManagerInstance) -> Self {
        Self { manager }
    }

    pub(crate) async fn process_historical_events(
        &self,
        from_block: u64,
        to_block: u64,
        state: &mut KeeperState,
    ) -> anyhow::Result<()> {
        let heartbeats_enqueued = self
            .query_events::<HeartbeatEnqueuedEvent>(from_block, to_block)
            .await?;
        let rounds_started = self
            .query_events::<RoundStartedEvent>(from_block, to_block)
            .await?;
        let rounds_finalized = self
            .query_events::<RoundFinalizedEvent>(from_block, to_block)
            .await?;
        let rewards_done = self
            .query_events::<RewardsDistributedEvent>(from_block, to_block)
            .await?;
        let rewards_abandoned = self
            .query_events::<RewardDistributionAbandonedEvent>(from_block, to_block)
            .await?;

        for (event, _log) in heartbeats_enqueued {
            // enqueued-but-unstarted heartbeats inside the lookback window feed the
            // startRound duty after a keeper restart
            state
                .raw_htx_by_heartbeat
                .insert(event.heartbeatKey, event.rawHTX.clone());
        }
        for (event, _log) in rounds_started {
            let key = RoundKey {
                heartbeat_key: event.heartbeatKey,
                round: event.round,
            };
            let entry = state.rounds.entry(key).or_default();
            entry.members = event.members;
            entry.raw_htx = event.rawHTX.clone();
            entry.deadline = event.deadline;
        }
        for (event, _log) in rounds_finalized {
            let key = RoundKey {
                heartbeat_key: event.heartbeatKey,
                round: event.round,
            };
            let entry = state.rounds.entry(key).or_default();
            entry.outcome = Some(event.outcome);
            on_round_finalized(&mut state.erc8004, event.heartbeatKey, event.outcome);
        }
        for (event, _log) in rewards_done {
            let key = RoundKey {
                heartbeat_key: event.heartbeatKey,
                round: event.round,
            };
            let entry = state.rounds.entry(key).or_default();
            entry.rewards_done = true;
        }
        for (event, _log) in rewards_abandoned {
            let key = RoundKey {
                heartbeat_key: event.heartbeatKey,
                round: event.round,
            };
            let entry = state.rounds.entry(key).or_default();
            entry.rewards_done = true;
        }

        info!(
            from_block,
            heartbeats = state.raw_htx_by_heartbeat.len(),
            rounds = state.rounds.len(),
            "Loaded historical keeper state"
        );

        Ok(())
    }

    pub(crate) async fn spawn(
        self,
        from_block: u64,
        state: Arc<Mutex<KeeperState>>,
    ) -> anyhow::Result<()> {
        let heartbeat_enqueued = self.subscribe(from_block).await?;
        let round_started = self.subscribe(from_block).await?;
        let round_finalized = self.subscribe(from_block).await?;
        let rewards_distributed = self.subscribe(from_block).await?;
        let rewards_distribution_abandoned = self.subscribe(from_block).await?;
        let slashing_callback_failed = self.subscribe(from_block).await?;
        tokio::spawn(Self::process_heartbeat_enqueued(
            heartbeat_enqueued,
            state.clone(),
        ));
        tokio::spawn(Self::process_round_started(round_started, state.clone()));
        tokio::spawn(Self::process_round_finalized(
            round_finalized,
            state.clone(),
        ));
        tokio::spawn(Self::process_rewards_distributed(
            rewards_distributed,
            state.clone(),
        ));
        tokio::spawn(Self::process_rewards_distribution_abandoned(
            rewards_distribution_abandoned,
            state.clone(),
        ));
        tokio::spawn(Self::process_slashing_callback_failed(
            slashing_callback_failed,
            self.manager.clone(),
        ));
        Ok(())
    }

    async fn query_events<E: SolEvent>(
        &self,
        from_block: u64,
        to_block: u64,
    ) -> anyhow::Result<Vec<(E, Log)>> {
        let events = self
            .manager
            .event_filter::<E>()
            .from_block(from_block)
            .to_block(to_block)
            .query()
            .await?;
        Ok(events)
    }

    async fn subscribe<E: SolEvent + 'static>(
        &self,
        from_block: u64,
    ) -> anyhow::Result<impl Stream<Item = E> + 'static> {
        // Pull the name out of the signature so we can expose a properly labelled metric
        let event_name = E::SIGNATURE
            .split_once('(')
            .map(|(name, _)| name)
            .unwrap_or(E::SIGNATURE);
        let stream = self
            .manager
            .event_filter::<E>()
            .from_block(from_block)
            .subscribe()
            .await
            .context("Failed to subscribe to events")?
            .into_stream()
            .filter_map(async move |e| match e {
                Ok((event, _)) => {
                    metrics::get().l2.events.inc_events_received(event_name);
                    Some(event)
                }
                Err(e) => {
                    error!("Failed to receive {} event: {e}", E::SIGNATURE);
                    None
                }
            });
        Ok(stream)
    }

    async fn process_heartbeat_enqueued(
        events: impl Stream<Item = HeartbeatEnqueuedEvent>,
        state: Arc<Mutex<KeeperState>>,
    ) {
        let mut events = pin!(events);
        while let Some(event) = events.next().await {
            let mut guard = state.lock().await;
            guard
                .raw_htx_by_heartbeat
                .insert(event.heartbeatKey, event.rawHTX.clone());
            debug!(heartbeat_key = ?event.heartbeatKey, "Heartbeat enqueued");
        }
    }

    async fn process_round_started(
        events: impl Stream<Item = RoundStartedEvent>,
        state: Arc<Mutex<KeeperState>>,
    ) {
        let mut events = pin!(events);
        while let Some(event) = events.next().await {
            let key = RoundKey {
                heartbeat_key: event.heartbeatKey,
                round: event.round,
            };
            let mut guard = state.lock().await;
            guard
                .raw_htx_by_heartbeat
                .insert(event.heartbeatKey, event.rawHTX.clone());
            let entry = guard.rounds.entry(key).or_default();
            entry.members = event.members.clone();
            entry.raw_htx = event.rawHTX;
            entry.deadline = event.deadline;
            info!(
                heartbeat_key = ?event.heartbeatKey,
                round = event.round,
                deadline = event.deadline,
                members = entry.members.len(),
                "Round started"
            );

            // Detect ERC-8004 HTXs and track them with HeartbeatManager's heartbeat_key
            if let Ok(erc8004_htx) = Erc8004Htx::try_decode(&entry.raw_htx) {
                // Only add if not already tracked (first round)
                if event.round == 1
                    && !guard
                        .erc8004
                        .pending_validations
                        .contains_key(&event.heartbeatKey)
                {
                    let info = ValidationRequestInfo::new(
                        erc8004_htx.validator_address,
                        erc8004_htx.agent_id,
                        erc8004_htx.request_uri.clone(),
                        erc8004_htx.request_hash,
                    );
                    guard
                        .erc8004
                        .pending_validations
                        .insert(event.heartbeatKey, info);
                    metrics::get()
                        .l2
                        .erc8004
                        .set_requests_tracked(guard.erc8004.pending_validations.len() as u64);
                    info!(
                        heartbeat_key = %event.heartbeatKey,
                        validator = %erc8004_htx.validator_address,
                        agent_id = %erc8004_htx.agent_id,
                        request_hash = %erc8004_htx.request_hash,
                        "ERC-8004 validation tracked from RoundStarted"
                    );
                }
            }
        }
    }

    async fn process_round_finalized(
        events: impl Stream<Item = RoundFinalizedEvent>,
        state: Arc<Mutex<KeeperState>>,
    ) {
        let mut events = pin!(events);
        while let Some(event) = events.next().await {
            let key = RoundKey {
                heartbeat_key: event.heartbeatKey,
                round: event.round,
            };
            let mut guard = state.lock().await;
            let entry = guard.rounds.entry(key).or_default();
            entry.outcome = Some(event.outcome);
            info!(
                heartbeat_key = ?event.heartbeatKey,
                round = event.round,
                outcome = event.outcome,
                "Round finalized"
            );

            // Notify ERC-8004 state about the round finalization
            on_round_finalized(&mut guard.erc8004, event.heartbeatKey, event.outcome);
        }
    }

    async fn process_rewards_distributed(
        events: impl Stream<Item = RewardsDistributedEvent>,
        state: Arc<Mutex<KeeperState>>,
    ) {
        let mut events = pin!(events);
        while let Some(event) = events.next().await {
            let key = RoundKey {
                heartbeat_key: event.heartbeatKey,
                round: event.round,
            };
            let mut guard = state.lock().await;
            let entry = guard.rounds.entry(key).or_default();
            entry.rewards_done = true;
            info!(
                heartbeat_key = ?event.heartbeatKey,
                round = event.round,
                voter_count = ?event.voterCount,
                "Rewards distributed"
            );
        }
    }

    async fn process_rewards_distribution_abandoned(
        events: impl Stream<Item = RewardDistributionAbandonedEvent>,
        state: Arc<Mutex<KeeperState>>,
    ) {
        let mut events = pin!(events);
        while let Some(event) = events.next().await {
            let key = RoundKey {
                heartbeat_key: event.heartbeatKey,
                round: event.round,
            };
            let mut guard = state.lock().await;
            let entry = guard.rounds.entry(key).or_default();
            entry.rewards_done = true;
            info!(
                heartbeat_key = ?event.heartbeatKey,
                round = event.round,
                "Reward distribution abandoned"
            );
        }
    }

    async fn process_slashing_callback_failed(
        events: impl Stream<Item = SlashingCallbackFailedEvent>,
        manager: HeartbeatManagerInstance,
    ) {
        let mut events = pin!(events);
        while let Some(event) = events.next().await {
            info!(
                heartbeat_key = ?event.heartbeatKey,
                round = event.round,
                "Slashing callback failed, retrying"
            );

            let SlashingCallbackFailed {
                heartbeatKey: heartbeat_key,
                round,
                ..
            } = event;
            if let Err(e) = Self::retry_slashing(&manager, heartbeat_key, round).await {
                error!("Failed to retry slashing for key {heartbeat_key} and round {round}: {e}");
            }
        }
    }

    async fn retry_slashing(
        manager: &HeartbeatManagerInstance,
        heartbeat_key: B256,
        round: u8,
    ) -> anyhow::Result<()> {
        let pending = manager
            .retrySlashing(heartbeat_key, round)
            .send()
            .await
            .context("Failed to retry slashing")?;
        let receipt = pending
            .get_receipt()
            .await
            .context("Failed to get tx receipt")?;
        info!(
            heartbeat_key = ?heartbeat_key,
            round,
            tx_hash = ?receipt.transaction_hash,
            "Slashing callback retried"
        );
        Ok(())
    }
}
