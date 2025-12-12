use alloy::{
    primitives::{Address, B256, U256},
    providers::Provider,
    sol,
};
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::Mutex;

// Generate type-safe contract bindings from ABI
sol!(
    #[sol(rpc)]
    #[derive(Debug)]
    TESTToken,
    "./contracts/out/TESTToken.sol/TESTToken.json"
);

// Optional: bring the instance & events into scope
use TESTToken::TESTTokenInstance;
// You’ll also get event types generated from the ABI, e.g.
// use NilAVRouter::{Htxsubmitted, Htxassigned, Htxresponded};

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
        Ok(self.contract.totalSupply().call().await?)
    }

    /// Returns the token balance of an account
    pub async fn balance_of(&self, account: Address) -> anyhow::Result<U256> {
        Ok(self.contract.balanceOf(account).call().await?)
    }

    /// Returns the remaining number of tokens that spender is allowed to spend on behalf of owner
    pub async fn allowance(&self, owner: Address, spender: Address) -> anyhow::Result<U256> {
        Ok(self.contract.allowance(owner, spender).call().await?)
    }

    // ------------------------------------------------------------------------
    // ERC20 Transaction Functions
    // ------------------------------------------------------------------------

    /// Transfers tokens to a recipient
    pub async fn transfer(&self, to: Address, amount: U256) -> anyhow::Result<B256> {
        let call = self.contract.transfer(to, amount);

        // Pre-simulate to catch errors with proper messages
        if let Err(e) = call.call().await {
            return Err(Self::decode_error(e));
        }

        let _guard = self.tx_lock.lock().await;
        let pending = call.send().await.map_err(Self::decode_error)?;
        let receipt = pending.get_receipt().await?;

        if !receipt.status() {
            if let Err(e) = call.call().await {
                let decoded = super::errors::decode_any_error(&e);
                return Err(anyhow::anyhow!(
                    "transfer reverted: {}. Tx hash: {:?}",
                    decoded,
                    receipt.transaction_hash
                ));
            }
            return Err(anyhow::anyhow!(
                "transfer reverted on-chain. Tx hash: {:?}",
                receipt.transaction_hash
            ));
        }
        Ok(receipt.transaction_hash)
    }

    /// Approves a spender to spend tokens on behalf of the caller
    pub async fn approve(&self, spender: Address, amount: U256) -> anyhow::Result<B256> {
        let call = self.contract.approve(spender, amount);

        // Pre-simulate to catch errors with proper messages
        if let Err(e) = call.call().await {
            return Err(Self::decode_error(e));
        }

        let _guard = self.tx_lock.lock().await;
        let pending = call.send().await.map_err(Self::decode_error)?;
        let receipt = pending.get_receipt().await?;

        if !receipt.status() {
            if let Err(e) = call.call().await {
                let decoded = super::errors::decode_any_error(&e);
                return Err(anyhow::anyhow!(
                    "approve reverted: {}. Tx hash: {:?}",
                    decoded,
                    receipt.transaction_hash
                ));
            }
            return Err(anyhow::anyhow!(
                "approve reverted on-chain. Tx hash: {:?}",
                receipt.transaction_hash
            ));
        }
        Ok(receipt.transaction_hash)
    }

    /// Mints new tokens (requires owner privileges)
    pub async fn mint(&self, to: Address, amount: U256) -> anyhow::Result<B256> {
        let call = self.contract.mint(to, amount);

        // Pre-simulate to catch errors with proper messages
        if let Err(e) = call.call().await {
            return Err(Self::decode_error(e));
        }

        let _guard = self.tx_lock.lock().await;
        let pending = call.send().await.map_err(Self::decode_error)?;
        let receipt = pending.get_receipt().await?;

        if !receipt.status() {
            if let Err(e) = call.call().await {
                let decoded = super::errors::decode_any_error(&e);
                return Err(anyhow::anyhow!(
                    "mint reverted: {}. Tx hash: {:?}",
                    decoded,
                    receipt.transaction_hash
                ));
            }
            return Err(anyhow::anyhow!(
                "mint reverted on-chain. Tx hash: {:?}",
                receipt.transaction_hash
            ));
        }
        Ok(receipt.transaction_hash)
    }

    // ------------------------------------------------------------------------
    // Error Handling
    // ------------------------------------------------------------------------

    /// Decode contract errors into human-readable messages
    fn decode_error<E: std::fmt::Display + std::fmt::Debug>(e: E) -> anyhow::Error {
        let error_str = e.to_string();
        let decoded = super::errors::decode_any_error(&e);

        // If we successfully decoded a revert, use that
        if !matches!(decoded, super::errors::DecodedRevert::NoRevertData(_)) {
            return anyhow::anyhow!("Contract reverted: {}", decoded);
        }

        // Common error patterns
        if error_str.contains("insufficient funds") {
            anyhow::anyhow!("Insufficient ETH for gas. Please fund the account.")
        } else if error_str.contains("replacement transaction underpriced") {
            anyhow::anyhow!("Transaction underpriced. A pending transaction may be blocking.")
        } else if error_str.contains("nonce too low") {
            anyhow::anyhow!("Nonce too low. A transaction may have been confirmed already.")
        } else {
            anyhow::anyhow!("Transaction failed: {}", e)
        }
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
        F: FnMut(TESTToken::Transfer) -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        let event_stream = self.contract.event_filter::<TESTToken::Transfer>();
        let subscription = event_stream.subscribe().await?;
        let mut events = subscription.into_stream();

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok((event, _log)) => {
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
