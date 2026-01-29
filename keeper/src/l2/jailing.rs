use crate::{
    clients::L2KeeperClient,
    l2::{KeeperState, RoundKey},
};
use alloy::primitives::Address;
use anyhow::bail;
use blacklight_contract_clients::common::errors::decode_any_error;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

pub(crate) struct Jailer {
    client: Arc<L2KeeperClient>,
    state: Arc<Mutex<KeeperState>>,
}

impl Jailer {
    pub(crate) fn new(client: Arc<L2KeeperClient>, state: Arc<Mutex<KeeperState>>) -> Self {
        Self { client, state }
    }

    pub(crate) async fn enforce(&self, key: RoundKey, members: Vec<Address>) -> anyhow::Result<()> {
        let Some(policy) = self.client.jailing_policy() else {
            return Ok(());
        };

        info!(
            heartbeat_key = ?key.heartbeat_key,
            round = key.round,
            members = members.len(),
            "Enforcing jailing"
        );

        if let Err(e) = policy
            .recordRound(key.heartbeat_key, key.round)
            .send()
            .await
        {
            warn!(
                heartbeat_key = ?key.heartbeat_key,
                round = key.round,
                error = %decode_any_error(&e),
                "Failed to record round in jailing policy"
            );
        }

        match policy
            .enforceJailFromMembers(key.heartbeat_key, key.round, members)
            .send()
            .await
        {
            Ok(pending) => {
                let receipt = pending.get_receipt().await?;
                info!(
                    heartbeat_key = ?key.heartbeat_key,
                    round = key.round,
                    tx_hash = ?receipt.transaction_hash,
                    "Jailing enforced"
                );
                let mut state = self.state.lock().await;
                if let Some(round_state) = state.rounds.get_mut(&key) {
                    round_state.jailing_done = true;
                }
                Ok(())
            }
            Err(e) => {
                bail!("Failed to enforce failing: {}", decode_any_error(&e))
            }
        }
    }
}
