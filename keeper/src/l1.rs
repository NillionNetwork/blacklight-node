use crate::{
    args::KeeperConfig,
    clients::{L1EmissionsClient, L2KeeperClient, RewardPolicyInstance},
    metrics,
};
use alloy::{eips::BlockNumberOrTag, primitives::U256, providers::Provider};
use anyhow::{Context, Result, bail};
use blacklight_contract_clients::ProtocolConfig::ProtocolConfigInstance;
use std::{sync::Arc, time::Duration};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

pub struct EmissionsSupervisor {
    l1_client: L1EmissionsClient,
    l2_client: Arc<L2KeeperClient>,
    config: KeeperConfig,
}

impl EmissionsSupervisor {
    pub async fn new(config: KeeperConfig, l2_client: Arc<L2KeeperClient>) -> Result<Self> {
        let l1_client = L1EmissionsClient::new(
            config.l1_rpc_url.clone(),
            config.l1_emissions_controller_address,
            config.private_key.clone(),
        )
        .await
        .context("Failed to create L1 client")?;
        Ok(Self {
            l1_client,
            l2_client,
            config,
        })
    }

    pub fn spawn(self) {
        info!("Starting L1 supervisor");
        tokio::spawn(self.run());
    }

    async fn run(self) {
        // Start by publishing this so we don't have to wait for an epoch to export it.
        self.publish_balance_metric().await;

        let mut ticker = interval(self.config.emissions_interval);
        loop {
            ticker.tick().await;
            match self.try_process_emissions().await {
                Ok(_) => {}
                Err(e) => {
                    error!("Failed to process emissions: {e}");
                }
            };
        }
    }

    async fn try_process_emissions(&self) -> anyhow::Result<()> {
        if !self.is_next_epoch_ready().await? {
            debug!("Next epoch is not ready yet");
            return Ok(());
        }
        if !self.is_l2_budget_depleted().await? {
            metrics::get().l1.epochs.set_blocked(true);
            warn!("Next epoch is ready but budget is not depleted yet");
            return Ok(());
        }
        metrics::get().l1.epochs.set_blocked(false);

        let emissions = self.l1_client.emissions();
        info!("Epoch ready for minting, making sure spendable budget is 0");

        info!("Minting and bridging next emission epoch");

        let call = emissions
            .mintAndBridgeNextEpoch()
            .value(self.config.l1_bridge_value);
        match call.send().await {
            Ok(pending) => {
                let receipt = pending.get_receipt().await?;
                let tx_hash = receipt.transaction_hash;
                self.publish_balance_metric().await;

                info!("Emission minted and bridged on tx {tx_hash}");
                Ok(())
            }
            Err(e) => {
                bail!("Failed to mint tokens: {e}")
            }
        }
    }

    async fn is_next_epoch_ready(&self) -> anyhow::Result<bool> {
        let emissions = self.l1_client.emissions();
        let minted_epochs = emissions.mintedEpochs().call().await?;
        let total_epochs = emissions.epochs().call().await?;
        metrics::get().l1.epochs.set_total(total_epochs);
        metrics::get().l1.epochs.set_minted(minted_epochs);

        if minted_epochs >= total_epochs {
            return Ok(false);
        }

        let ready_at = emissions.nextEpochReadyAt().call().await?;
        let latest = self
            .l1_client
            .provider()
            .get_block_by_number(BlockNumberOrTag::Latest)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Missing latest block"))?;
        let now = U256::from(latest.header.timestamp);

        if now < ready_at {
            let missing = ready_at.saturating_sub(now);
            let missing = match u64::try_from(missing) {
                Ok(missing) => Duration::from_secs(missing),
                Err(_) => Duration::MAX,
            };
            info!(
                ready_at = ?ready_at,
                now = ?now,
                "Next emission will be ready in {missing:?}"
            );
            return Ok(false);
        }
        Ok(true)
    }

    async fn is_l2_budget_depleted(&self) -> anyhow::Result<bool> {
        let protocol_config_address = self
            .l2_client
            .staking_operators()
            .protocolConfig()
            .call()
            .await
            .context("Failed to get protocol config address")?;
        let protocol_config =
            ProtocolConfigInstance::new(protocol_config_address, self.l2_client.provider());
        let reward_policy_address = protocol_config
            .rewardPolicy()
            .call()
            .await
            .context("Failed to get reward policy contract address")?;
        let reward_policy =
            RewardPolicyInstance::new(reward_policy_address, self.l2_client.provider());
        let spendable_budget = reward_policy
            .spendableBudget()
            .call()
            .await
            .context("Failed to get spendable budget")?;
        let remaining = reward_policy
            .streamRemaining()
            .call()
            .await
            .context("Failed to get stream remaining")?;
        let budget = spendable_budget.saturating_add(remaining);
        Ok(budget == U256::ZERO)
    }

    async fn publish_balance_metric(&self) {
        match self.l1_client.get_balance().await {
            Ok(balance) => {
                metrics::get().l1.eth.set_funds(balance);
            }
            Err(e) => {
                error!("Failed to fetch balance: {e}");
            }
        };
    }
}
