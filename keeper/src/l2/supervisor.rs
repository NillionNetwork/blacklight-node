use crate::{
    args::KeeperConfig,
    clients::L2KeeperClient,
    l2::{
        KeeperState, escalator::RoundEscalator, events::EventListener, jailing::Jailer,
        rewards::RewardsDistributor,
    },
    metrics,
};
use alloy::{eips::BlockId, providers::Provider};
use anyhow::Context;
use std::sync::Arc;
use tokio::{sync::Mutex, time::interval};
use tracing::error;

pub struct L2Supervisor {
    client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
    jailer: Jailer,
    rewards_distributor: RewardsDistributor,
    round_escalator: RoundEscalator,
}

impl L2Supervisor {
    pub async fn new(
        client: Arc<L2KeeperClient>,
        state: Arc<Mutex<KeeperState>>,
    ) -> anyhow::Result<Self> {
        let jailer = Jailer::new(client.clone(), state.clone());
        let rewards_distributor = RewardsDistributor::new(client.clone(), state.clone());
        let round_escalator = RoundEscalator::new(client.clone(), state.clone());
        Ok(Self {
            client,
            state,
            jailer,
            rewards_distributor,
            round_escalator,
        })
    }

    pub(crate) async fn spawn(self, config: KeeperConfig) -> anyhow::Result<()> {
        let latest_block = self
            .client
            .provider()
            .get_block_number()
            .await
            .context("Failed to find latest block")?;
        let from_block = latest_block.saturating_sub(config.lookback_blocks);

        // Process historic events from current block - lookback until now
        let event_listener = EventListener::new(self.client.heartbeat_manager().clone());
        event_listener
            .process_historical_events(from_block, latest_block, &mut *self.state.lock().await)
            .await
            .context("Failed to process historical events")?;

        // Now spawn to process any new blocks after latest_block
        event_listener
            .spawn(latest_block.saturating_add(1), self.state.clone())
            .await
            .context("Failed tp spawn event listener")?;

        tokio::spawn(self.run(config));
        Ok(())
    }

    async fn run(mut self, config: KeeperConfig) {
        let mut ticker = interval(config.tick_interval);
        loop {
            ticker.tick().await;

            let block_timestamp = match self.client.provider().get_block(BlockId::latest()).await {
                Ok(Some(block)) => {
                    metrics::get().l2.escalations.set_block(block.header.number);
                    block.header.timestamp
                }
                Ok(None) => {
                    error!("No latest block found (is the chain working?)");
                    continue;
                }
                Err(e) => {
                    error!("Failed to fetch latest block: {e}");
                    continue;
                }
            };

            if let Err(e) = self.rewards_distributor.sync_state().await {
                error!("Error syncing state: {e}");
            }

            if let Err(e) = self
                .round_escalator
                .process_escalations(block_timestamp)
                .await
            {
                error!("Failed to process escalations: {e}");
            }

            self.process_rounds(block_timestamp).await;

            match self
                .client
                .provider()
                .get_balance(self.client.signer_address())
                .await
            {
                Ok(balance) => metrics::get().l2.eth.set_funds(balance),
                Err(e) => error!("Failed to get our balance: {e}"),
            };
        }
    }

    async fn process_rounds(&mut self, block_timestamp: u64) {
        let mut reward_jobs = Vec::new();
        let mut jail_jobs = Vec::new();
        let has_jailing_policy = self.client.jailing_policy().is_some();

        {
            let state = self.state.lock().await;
            for (key, round) in state.rounds.iter() {
                if let Some(outcome) = round.outcome {
                    if !round.rewards_done && !round.members.is_empty() {
                        reward_jobs.push((*key, outcome, round.members.clone()));
                    }
                    if has_jailing_policy && !round.jailing_done && !round.members.is_empty() {
                        jail_jobs.push((*key, round.members.clone()));
                    }
                }
            }
        }

        for (key, outcome, members) in reward_jobs {
            if outcome == 0 {
                continue;
            }
            if let Err(e) = self
                .rewards_distributor
                .distribute_rewards(block_timestamp, key, outcome, members)
                .await
            {
                error!("Failed to process jailing: {e}");
            }
        }

        for (key, members) in jail_jobs {
            if let Err(e) = self.jailer.enforce(key, members).await {
                error!(
                    heartbeat_key = ?key.heartbeat_key,
                    round = key.round,
                    "Jailing enforcement failed: {e}"
                );
            }
        }
    }
}
