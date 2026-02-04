use crate::{
    ContractConfig, HeartbeatManagerClient, NilTokenClient, ProtocolConfigClient,
    StakingOperatorsClient,
};
use alloy::{
    primitives::{Address, B256, U256},
    providers::DynProvider,
};
use contract_clients_common::ProviderContext;

/// High-level wrapper bundling all contract clients with a shared Alloy provider.
#[derive(Clone)]
pub struct BlacklightClient {
    ctx: ProviderContext,
    pub manager: HeartbeatManagerClient<DynProvider>,
    pub token: NilTokenClient<DynProvider>,
    pub staking: StakingOperatorsClient<DynProvider>,
    pub protocol_config: ProtocolConfigClient<DynProvider>,
}

impl BlacklightClient {
    pub async fn new(config: ContractConfig, private_key: String) -> anyhow::Result<Self> {
        let ctx = ProviderContext::with_ws_retries(
            &config.rpc_url,
            &private_key,
            Some(config.max_ws_retries),
        )
        .await?;

        Self::from_context(ctx, config).await
    }

    /// Create a client from an existing [`ProviderContext`].
    ///
    /// Use this when you want to share the same provider, wallet, and nonce
    /// tracker across multiple clients (e.g. `BlacklightClient` and `Erc8004Client`).
    pub async fn from_context(
        ctx: ProviderContext,
        config: ContractConfig,
    ) -> anyhow::Result<Self> {
        let provider = ctx.provider().clone();
        let tx_lock = ctx.tx_lock();

        // Instantiate contract clients using the shared provider
        let manager =
            HeartbeatManagerClient::new(provider.clone(), config.clone(), tx_lock.clone());
        let token = NilTokenClient::new(provider.clone(), config.clone(), tx_lock.clone());
        let staking = StakingOperatorsClient::new(provider.clone(), config, tx_lock.clone());

        let protocol_config_address = staking.protocol_config().await?;
        let protocol_config =
            ProtocolConfigClient::new(provider.clone(), protocol_config_address, tx_lock);

        Ok(Self {
            ctx,
            manager,
            token,
            staking,
            protocol_config,
        })
    }

    /// Get the signer address
    pub fn signer_address(&self) -> Address {
        self.ctx.signer_address()
    }

    /// Get the balance of the wallet
    pub async fn get_balance(&self) -> anyhow::Result<U256> {
        self.ctx.get_balance().await
    }

    /// Get the balance of a specific address
    pub async fn get_balance_of(&self, address: Address) -> anyhow::Result<U256> {
        self.ctx.get_balance_of(address).await
    }

    /// Send ETH to an address
    pub async fn send_eth(&self, to: Address, amount: U256) -> anyhow::Result<B256> {
        self.ctx.send_eth(to, amount).await
    }
}
