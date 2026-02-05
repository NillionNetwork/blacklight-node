use alloy::{
    network::{Ethereum, EthereumWallet, NetworkWallet},
    primitives::{Address, B256, TxKind, U256},
    providers::{DynProvider, Provider, ProviderBuilder, WsConnect},
    rpc::types::TransactionRequest,
    signers::local::PrivateKeySigner,
};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared provider context that holds an Alloy provider, wallet, and transaction lock.
///
/// When multiple contract clients (e.g. `BlacklightClient` and `Erc8004Client`) are
/// instantiated with the same private key, they should share a single `ProviderContext`
/// to avoid nonce conflicts. Cloning a `ProviderContext` shares the underlying state.
#[derive(Clone)]
pub struct ProviderContext {
    provider: DynProvider,
    wallet: EthereumWallet,
    tx_lock: Arc<Mutex<()>>,
}

impl ProviderContext {
    /// Create a new provider context with a WebSocket connection.
    pub async fn new(rpc_url: &str, private_key: &str) -> anyhow::Result<Self> {
        Self::with_ws_retries(rpc_url, private_key, None).await
    }

    /// Create a new provider context with configurable WebSocket retry count.
    ///
    /// If `max_ws_retries` is `None`, the default retry behaviour from Alloy is used
    /// (no explicit retry limit set).
    pub async fn with_ws_retries(
        rpc_url: &str,
        private_key: &str,
        max_ws_retries: Option<u32>,
    ) -> anyhow::Result<Self> {
        let ws_url = rpc_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");

        let mut ws = WsConnect::new(ws_url);
        if let Some(retries) = max_ws_retries {
            ws = ws.with_max_retries(retries);
        }

        let signer: PrivateKeySigner = private_key.parse::<PrivateKeySigner>()?;
        let wallet = EthereumWallet::from(signer);

        let provider: DynProvider = ProviderBuilder::new()
            .wallet(wallet.clone())
            .with_simple_nonce_management()
            .with_gas_estimation()
            .connect_ws(ws)
            .await?
            .erased();

        let tx_lock = Arc::new(Mutex::new(()));

        Ok(Self {
            provider,
            wallet,
            tx_lock,
        })
    }

    /// Reference to the underlying provider.
    pub fn provider(&self) -> &DynProvider {
        &self.provider
    }

    /// Reference to the wallet.
    pub fn wallet(&self) -> &EthereumWallet {
        &self.wallet
    }

    /// Shared transaction lock.
    pub fn tx_lock(&self) -> Arc<Mutex<()>> {
        self.tx_lock.clone()
    }

    /// Get the default signer address from the wallet.
    pub fn signer_address(&self) -> Address {
        <EthereumWallet as NetworkWallet<Ethereum>>::default_signer_address(&self.wallet)
    }

    /// Get the ETH balance of the signer address.
    pub async fn get_balance(&self) -> anyhow::Result<U256> {
        let address = self.signer_address();
        Ok(self.provider.get_balance(address).await?)
    }

    /// Get the ETH balance of a specific address.
    pub async fn get_balance_of(&self, address: Address) -> anyhow::Result<U256> {
        Ok(self.provider.get_balance(address).await?)
    }

    /// Send ETH to an address.
    pub async fn send_eth(&self, to: Address, amount: U256) -> anyhow::Result<B256> {
        let tx = TransactionRequest {
            to: Some(TxKind::Call(to)),
            value: Some(amount),
            max_priority_fee_per_gas: Some(1),
            ..Default::default()
        };

        let tx_hash = self.provider.send_transaction(tx).await?.watch().await?;
        Ok(tx_hash)
    }

    /// Get the current block number.
    pub async fn get_block_number(&self) -> anyhow::Result<u64> {
        Ok(self.provider.get_block_number().await?)
    }
}
