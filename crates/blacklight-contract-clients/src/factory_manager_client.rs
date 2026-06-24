use crate::{NodeOperatorFactoryClient, StakingOperatorsClient};
use alloy::{
    primitives::{Address, B256, U256},
    providers::DynProvider,
};
use contract_clients_common::ProviderContext;

/// High-level wrapper bundling the factory and staking clients with a shared provider.
///
/// Follows the same pattern as [`crate::BlacklightClient`]: a single [`ProviderContext`]
/// owns the provider, wallet, and nonce tracker, preventing nonce conflicts when
/// multiple contract calls go through the same owner key.
#[derive(Clone)]
pub struct FactoryManagerClient {
    ctx: ProviderContext,
    pub factory: NodeOperatorFactoryClient<DynProvider>,
    pub staking: StakingOperatorsClient<DynProvider>,
}

impl FactoryManagerClient {
    /// Create a new client from an RPC URL, private key, and factory address.
    ///
    /// Resolves the `StakingOperators` address on-chain from the factory contract.
    pub async fn new(
        rpc_url: &str,
        private_key: &str,
        factory_address: Address,
    ) -> anyhow::Result<Self> {
        let ctx = ProviderContext::new_http(rpc_url, private_key)?;
        Self::from_context(ctx, factory_address).await
    }

    /// Create a client from an existing [`ProviderContext`].
    ///
    /// Use this when you want to share the same provider, wallet, and nonce
    /// tracker across multiple clients.
    pub async fn from_context(
        ctx: ProviderContext,
        factory_address: Address,
    ) -> anyhow::Result<Self> {
        let provider = ctx.provider().clone();
        let tx_lock = ctx.tx_lock();

        let factory =
            NodeOperatorFactoryClient::new(provider.clone(), factory_address, tx_lock.clone());

        let staking_ops_addr = factory.staking_operators().await?;
        let staking =
            StakingOperatorsClient::at_address(provider.clone(), staking_ops_addr, tx_lock);

        Ok(Self {
            ctx,
            factory,
            staking,
        })
    }

    /// Reference to the underlying provider context.
    pub fn ctx(&self) -> &ProviderContext {
        &self.ctx
    }

    /// Get the signer address.
    pub fn signer_address(&self) -> Address {
        self.ctx.signer_address()
    }

    /// Send ETH to an address.
    pub async fn send_eth(&self, to: Address, amount: U256) -> anyhow::Result<B256> {
        self.ctx.send_eth(to, amount).await
    }
}
