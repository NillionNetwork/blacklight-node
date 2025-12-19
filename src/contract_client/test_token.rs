use alloy::{
    primitives::{Address, B256, U256},
    providers::Provider,
    sol,
};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::contract_client::common::event_helper::listen_events;
use crate::contract_client::common::tx_helper::send_and_confirm;
use anyhow::Result;

// Generate type-safe contract bindings from ABI
sol!(
    #[sol(rpc)]
    #[derive(Debug)]
    TESTToken,
    "./contracts/out/TESTToken.sol/TESTToken.json"
);

// Optional: bring the instance & events into scope
use TESTToken::TESTTokenInstance;

/// WebSocket-based client for interacting with the TESTToken ERC20 contract
#[derive(Clone)]
pub struct TESTTokenClient<P: Provider + Clone> {
    contract: TESTTokenInstance<P>,
    tx_lock: Arc<Mutex<()>>,
}

impl<P: Provider + Clone> TESTTokenClient<P> {
    /// Create a new WebSocket client from configuration
    pub fn new(
        provider: P,
        config: crate::contract_client::ContractConfig,
        tx_lock: Arc<Mutex<()>>,
    ) -> Self {
        let contract_address = config.token_contract_address;
        let contract = TESTTokenInstance::new(contract_address, provider.clone());
        Self { contract, tx_lock }
    }

    /// Get the contract address
    pub fn address(&self) -> Address {
        *self.contract.address()
    }

    // ------------------------------------------------------------------------
    // ERC20 Standard Functions
    // ------------------------------------------------------------------------

    /// Returns the name of the token
    pub async fn name(&self) -> Result<String> {
        Ok(self.contract.name().call().await?)
    }

    /// Returns the symbol of the token
    pub async fn symbol(&self) -> Result<String> {
        Ok(self.contract.symbol().call().await?)
    }

    /// Returns the number of decimals the token uses
    pub async fn decimals(&self) -> Result<u8> {
        Ok(self.contract.decimals().call().await?)
    }

    /// Returns the total token supply
    pub async fn total_supply(&self) -> Result<U256> {
        Ok(self.contract.totalSupply().call().await?)
    }

    /// Returns the token balance of an account
    pub async fn balance_of(&self, account: Address) -> Result<U256> {
        Ok(self.contract.balanceOf(account).call().await?)
    }

    /// Returns the remaining number of tokens that spender is allowed to spend on behalf of owner
    pub async fn allowance(&self, owner: Address, spender: Address) -> Result<U256> {
        Ok(self.contract.allowance(owner, spender).call().await?)
    }

    // ------------------------------------------------------------------------
    // ERC20 Transaction Functions
    // ------------------------------------------------------------------------

    /// Transfers tokens to a recipient
    pub async fn transfer(&self, to: Address, amount: U256) -> Result<B256> {
        let call = self.contract.transfer(to, amount);
        send_and_confirm(call, &self.tx_lock, "transfer").await
    }

    /// Approves a spender to spend tokens on behalf of the caller
    pub async fn approve(&self, spender: Address, amount: U256) -> Result<B256> {
        let call = self.contract.approve(spender, amount);
        send_and_confirm(call, &self.tx_lock, "approve").await
    }

    /// Mints new tokens (requires owner privileges)
    pub async fn mint(&self, to: Address, amount: U256) -> Result<B256> {
        let call = self.contract.mint(to, amount);
        send_and_confirm(call, &self.tx_lock, "mint").await
    }

    // ------------------------------------------------------------------------
    // Event Listening Functions
    // ------------------------------------------------------------------------

    /// Start listening for Transfer events (including mints where from == address(0))
    pub async fn listen_transfer_events<F, Fut>(self: Arc<Self>, callback: F) -> Result<()>
    where
        F: FnMut(TESTToken::Transfer) -> Fut + Send,
        Fut: std::future::Future<Output = Result<()>> + Send,
    {
        let event_stream = self.contract.event_filter::<TESTToken::Transfer>();
        let subscription = event_stream.subscribe().await?;
        let events = subscription.into_stream();

        listen_events(events, "Transfer", callback).await
    }
}
