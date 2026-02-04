use crate::{ContractConfig, IdentityRegistryClient, ValidationRegistryClient};
use alloy::{
    primitives::{Address, B256, U256},
    providers::DynProvider,
};
use contract_clients_common::ProviderContext;

/// High-level wrapper bundling ERC-8004 contract clients with a shared Alloy provider.
#[derive(Clone)]
pub struct Erc8004Client {
    ctx: ProviderContext,
    pub identity_registry: IdentityRegistryClient<DynProvider>,
    pub validation_registry: ValidationRegistryClient<DynProvider>,
}

impl Erc8004Client {
    pub async fn new(config: ContractConfig, private_key: String) -> anyhow::Result<Self> {
        let ctx = ProviderContext::new(&config.rpc_url, &private_key).await?;
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
        let identity_registry = IdentityRegistryClient::new(
            provider.clone(),
            config.identity_registry_contract_address,
            tx_lock.clone(),
        );
        let validation_registry = ValidationRegistryClient::new(
            provider.clone(),
            config.validation_registry_contract_address,
            tx_lock,
        );

        Ok(Self {
            ctx,
            identity_registry,
            validation_registry,
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

    /// Get the current block number
    pub async fn get_block_number(&self) -> anyhow::Result<u64> {
        self.ctx.get_block_number().await
    }
}
