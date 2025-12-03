use ethers::{
    contract::abigen,
    core::types::{Address, U256},
    middleware::{NonceManagerMiddleware, SignerMiddleware},
    providers::{Provider, Ws},
    signers::LocalWallet,
};
use std::sync::Arc;

// Generate type-safe contract bindings from ABI
abigen!(
    StakingOperators,
    "./contracts/out/StakingOperators.sol/StakingOperators.json",
    event_derives(serde::Deserialize, serde::Serialize)
);

pub type SignedWsProvider = NonceManagerMiddleware<SignerMiddleware<Provider<Ws>, LocalWallet>>;

/// WebSocket-based client for interacting with the StakingOperators contract
pub struct StakingOperatorsClient {
    contract: StakingOperators<SignedWsProvider>,
}

impl StakingOperatorsClient {
    /// Create a new WebSocket client from configuration
    pub fn new(
        provider: Arc<SignedWsProvider>,
        config: crate::contract_client::ContractConfig,
    ) -> Self {
        let contract = StakingOperators::new(config.staking_contract_address, provider.clone());

        Self { contract }
    }

    /// Get the contract address
    pub fn address(&self) -> Address {
        self.contract.address()
    }

    // ------------------------------------------------------------------------
    // View Functions
    // ------------------------------------------------------------------------

    /// Returns the address of the staking token
    pub async fn staking_token(&self) -> anyhow::Result<Address> {
        Ok(self.contract.staking_token().call().await?)
    }

    /// Returns the total stake amount for a specific operator
    pub async fn stake_of(&self, operator: Address) -> anyhow::Result<U256> {
        Ok(self.contract.stake_of(operator).call().await?)
    }

    /// Checks if an operator is active
    pub async fn is_active_operator(&self, operator: Address) -> anyhow::Result<bool> {
        Ok(self.contract.is_active_operator(operator).call().await?)
    }

    /// Returns a list of all currently active operators
    pub async fn get_active_operators(&self) -> anyhow::Result<Vec<Address>> {
        Ok(self.contract.get_active_operators().call().await?)
    }

    // ------------------------------------------------------------------------
    // Event Query Functions
    // ------------------------------------------------------------------------

    /// Get historical Staked events
    /// Set lookback_blocks to u64::MAX to search entire history
    pub async fn get_staked_events(&self, lookback_blocks: u64) -> anyhow::Result<Vec<StakedToFilter>> {
        use ethers::providers::Middleware;

        let from_block = if lookback_blocks == u64::MAX {
            0
        } else {
            let provider = self.contract.client();
            let current_block = provider.get_block_number().await?.as_u64();
            current_block.saturating_sub(lookback_blocks)
        };

        let events = self
            .contract
            .event::<StakedToFilter>()
            .from_block(from_block)
            .query()
            .await?;
        Ok(events)
    }

    // ------------------------------------------------------------------------
    // Staking Functions
    // ------------------------------------------------------------------------

    /// Stakes tokens to a specific operator
    pub async fn stake_to(
        &self,
        operator: Address,
        amount: U256,
    ) -> anyhow::Result<ethers::types::H256> {
        let call = self.contract.stake_to(operator, amount);
        let tx = call.send().await?;
        let receipt = tx.await?;
        let receipt = receipt.ok_or_else(|| anyhow::anyhow!("No transaction receipt"))?;
        Ok(receipt.transaction_hash)
    }

    /// Requests to unstake tokens from an operator
    pub async fn request_unstake(
        &self,
        operator: Address,
        amount: U256,
    ) -> anyhow::Result<ethers::types::H256> {
        let call = self.contract.request_unstake(operator, amount);
        let tx = call.send().await?;
        let receipt = tx.await?;
        let receipt = receipt.ok_or_else(|| anyhow::anyhow!("No transaction receipt"))?;
        Ok(receipt.transaction_hash)
    }

    /// Withdraws unstaked tokens after the unbonding period has passed
    pub async fn withdraw_unstaked(
        &self,
        operator: Address,
    ) -> anyhow::Result<ethers::types::H256> {
        let call = self.contract.withdraw_unstaked(operator);
        let tx = call.send().await?;
        let receipt = tx.await?;
        let receipt = receipt.ok_or_else(|| anyhow::anyhow!("No transaction receipt"))?;
        Ok(receipt.transaction_hash)
    }

    // ------------------------------------------------------------------------
    // Operator Registry Functions
    // ------------------------------------------------------------------------

    /// Registers the caller as an operator or updates their metadata
    pub async fn register_operator(
        &self,
        metadata_uri: String,
    ) -> anyhow::Result<ethers::types::H256> {
        let call = self.contract.register_operator(metadata_uri);
        let tx = call.send().await?;
        let receipt = tx.await?;
        let receipt = receipt.ok_or_else(|| anyhow::anyhow!("No transaction receipt"))?;
        Ok(receipt.transaction_hash)
    }

    /// Deactivates the caller as an operator
    pub async fn deactivate_operator(&self) -> anyhow::Result<ethers::types::H256> {
        let call = self.contract.deactivate_operator();
        let tx = call.send().await?;
        let receipt = tx.await?;
        let receipt = receipt.ok_or_else(|| anyhow::anyhow!("No transaction receipt"))?;
        Ok(receipt.transaction_hash)
    }
}
