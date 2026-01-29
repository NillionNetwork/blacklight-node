use crate::{args::KeeperConfig, clients::L1EmissionsClient};
use alloy::{
    eips::BlockNumberOrTag,
    primitives::{U256, utils::format_ether},
    providers::Provider,
};
use anyhow::{Context, Result, bail};
use tokio::time::interval;
use tracing::{debug, error, info};

pub struct L1Supervisor {
    client: L1EmissionsClient,
    config: KeeperConfig,
}

impl L1Supervisor {
    pub async fn new(config: KeeperConfig) -> Result<Self> {
        let client = L1EmissionsClient::new(
            config.l1_rpc_url.clone(),
            config.l1_emissions_controller_address,
            config.private_key.clone(),
        )
        .await
        .context("Failed to create L1 client")?;
        Ok(Self { client, config })
    }

    pub fn spawn(self) {
        info!("Starting L1 supervisor");
        tokio::spawn(self.run());
    }

    async fn run(self) {
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
        let emissions = self.client.emissions();
        let minted_epochs = emissions.mintedEpochs().call().await?;
        let total_epochs = emissions.epochs().call().await?;
        if minted_epochs >= total_epochs {
            return Ok(());
        }

        let ready_at = emissions.nextEpochReadyAt().call().await?;
        let latest = self
            .client
            .provider()
            .get_block_by_number(BlockNumberOrTag::Latest)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Missing latest block"))?;
        let now = U256::from(latest.header.timestamp);

        if now < ready_at {
            debug!(
                ready_at = ?ready_at,
                now = ?now,
                "Next emission not ready"
            );
            return Ok(());
        }

        info!(
            minted_epochs = ?minted_epochs,
            total_epochs = ?total_epochs,
            "Minting and bridging next emission epoch"
        );

        let call = emissions
            .mintAndBridgeNextEpoch()
            .value(self.config.l1_bridge_value);
        match call.send().await {
            Ok(pending) => {
                let receipt = pending.get_receipt().await?;
                let tx_hash = receipt.transaction_hash;
                let balance = format_ether(self.client.get_balance().await?);
                info!("Emission minted and bridged on tx {tx_hash}, have {balance} ETH left");
                Ok(())
            }
            Err(e) => {
                bail!("Failed to mint tokens: {e}")
            }
        }
    }
}
