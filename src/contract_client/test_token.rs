use ethers::{
    contract::abigen,
    core::types::{Address, U256},
};
use std::sync::Arc;

use crate::contract_client::SignedWsProvider;

// Generate type-safe contract bindings from ABI
abigen!(
    TESTToken,
    "./contracts/out/TESTToken.sol/TESTToken.json",
    event_derives(serde::Deserialize, serde::Serialize)
);

/// WebSocket-based client for interacting with the TESTToken ERC20 contract
pub struct TESTTokenClient {
    contract: TESTToken<SignedWsProvider>,
}

impl TESTTokenClient {
    /// Create a new WebSocket client from configuration
    pub fn new(
        provider: Arc<SignedWsProvider>,
        config: crate::contract_client::ContractConfig,
    ) -> Self {
        let contract_address = config.token_contract_address;
        let contract = TESTToken::new(contract_address, provider.clone());
        Self { contract }
    }

    /// Get the contract address
    pub fn address(&self) -> Address {
        self.contract.address()
    }

    // ------------------------------------------------------------------------
    // ERC20 Standard Functions
    // ------------------------------------------------------------------------

    /// Returns the name of the token
    pub async fn name(&self) -> anyhow::Result<String> {
        Ok(self.contract.name().call().await?)
    }

    /// Returns the symbol of the token
    pub async fn symbol(&self) -> anyhow::Result<String> {
        Ok(self.contract.symbol().call().await?)
    }

    /// Returns the number of decimals the token uses
    pub async fn decimals(&self) -> anyhow::Result<u8> {
        Ok(self.contract.decimals().call().await?)
    }

    /// Returns the total token supply
    pub async fn total_supply(&self) -> anyhow::Result<U256> {
        Ok(self.contract.total_supply().call().await?)
    }

    /// Returns the token balance of an account
    pub async fn balance_of(&self, account: Address) -> anyhow::Result<U256> {
        Ok(self.contract.balance_of(account).call().await?)
    }

    /// Returns the remaining number of tokens that spender is allowed to spend on behalf of owner
    pub async fn allowance(&self, owner: Address, spender: Address) -> anyhow::Result<U256> {
        Ok(self.contract.allowance(owner, spender).call().await?)
    }

    // ------------------------------------------------------------------------
    // ERC20 Transaction Functions
    // ------------------------------------------------------------------------

    /// Transfers tokens to a recipient
    pub async fn transfer(&self, to: Address, amount: U256) -> anyhow::Result<ethers::types::H256> {
        let call = self.contract.transfer(to, amount);
        let tx = call.send().await?;
        let receipt = tx.await?;
        let receipt = receipt.ok_or_else(|| anyhow::anyhow!("No transaction receipt"))?;
        Ok(receipt.transaction_hash)
    }

    /// Approves a spender to spend tokens on behalf of the caller
    pub async fn approve(
        &self,
        spender: Address,
        amount: U256,
    ) -> anyhow::Result<ethers::types::H256> {
        let call = self.contract.approve(spender, amount);
        let tx = call.send().await?;
        let receipt = tx.await?;
        let receipt = receipt.ok_or_else(|| anyhow::anyhow!("No transaction receipt"))?;
        Ok(receipt.transaction_hash)
    }

    /// Mints new tokens (requires owner privileges)
    pub async fn mint(&self, to: Address, amount: U256) -> anyhow::Result<ethers::types::H256> {
        let call = self.contract.mint(to, amount);
        let tx = call.send().await?;
        let receipt = tx.await?;
        let receipt = receipt.ok_or_else(|| anyhow::anyhow!("No transaction receipt"))?;
        Ok(receipt.transaction_hash)
    }

    // ------------------------------------------------------------------------
    // Event Listening Functions
    // ------------------------------------------------------------------------

    /// Start listening for Transfer events (including mints where from == address(0))
    pub async fn listen_transfer_events<F, Fut>(
        self: Arc<Self>,
        mut callback: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(TransferFilter) -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        use ethers::providers::StreamExt;

        let event_stream = self.contract.event::<TransferFilter>();
        let mut events = event_stream.subscribe().await?;

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok(event) => {
                    if let Err(e) = callback(event).await {
                        tracing::error!("Error processing Transfer event: {}", e);
                    }
                }
                Err(e) => {
                    tracing::error!("Error receiving Transfer event: {}", e);
                }
            }
        }
        Ok(())
    }
}
