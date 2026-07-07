use crate::chain_profile::FeeStrategy;
use alloy::{
    network::{Ethereum, EthereumWallet, NetworkWallet},
    primitives::{Address, B256, TxKind, U256},
    providers::{DynProvider, Provider, ProviderBuilder, WsConnect},
    rpc::types::TransactionRequest,
    signers::local::PrivateKeySigner,
};
use std::sync::Arc;
use std::time::Duration;
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
    fee_strategy: FeeStrategy,
}

impl ProviderContext {
    /// Create a new provider context with a WebSocket connection.
    pub async fn new(rpc_url: &str, private_key: &str) -> anyhow::Result<Self> {
        Self::with_ws_retries(rpc_url, private_key, None).await
    }

    /// Create a new provider context with an HTTP connection.
    pub fn new_http(rpc_url: &str, private_key: &str) -> anyhow::Result<Self> {
        let signer: PrivateKeySigner = private_key.parse::<PrivateKeySigner>()?;
        let wallet = EthereumWallet::from(signer);

        let provider: DynProvider = ProviderBuilder::new()
            .wallet(wallet.clone())
            .with_simple_nonce_management()
            .with_gas_estimation()
            .connect_http(rpc_url.parse()?)
            .erased();

        let tx_lock = Arc::new(Mutex::new(()));

        Ok(Self {
            provider,
            wallet,
            tx_lock,
            fee_strategy: FeeStrategy::default(),
        })
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
            fee_strategy: FeeStrategy::default(),
        })
    }

    /// Use a specific fee strategy for value transfers (N4/N7); the default keeps the
    /// pre-profile behaviour.
    pub fn with_fee_strategy(mut self, fee_strategy: FeeStrategy) -> Self {
        self.fee_strategy = fee_strategy;
        self
    }

    /// The configured fee strategy.
    pub fn fee_strategy(&self) -> &FeeStrategy {
        &self.fee_strategy
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

    /// Send ETH to an address and wait for the receipt.
    pub async fn send_eth(&self, to: Address, amount: U256) -> anyhow::Result<B256> {
        let (max_fee, priority_fee) = match &self.fee_strategy {
            // pre-N4 behaviour, byte-identical
            FeeStrategy::L2MinPriority => {
                let gas_price = self.provider.get_gas_price().await?;
                let max_fee = std::cmp::max(gas_price, 1);
                (max_fee, std::cmp::min(1, max_fee))
            }
            FeeStrategy::Eip1559 {
                max_fee_cap_gwei, ..
            } => {
                let estimate = self.provider.estimate_eip1559_fees().await?;
                if let Some(cap_gwei) = max_fee_cap_gwei {
                    let cap_wei = *cap_gwei as u128 * 1_000_000_000;
                    if estimate.max_fee_per_gas > cap_wei {
                        anyhow::bail!(
                            "estimated max fee {} wei exceeds cap {} wei; queuing for retry",
                            estimate.max_fee_per_gas,
                            cap_wei
                        );
                    }
                }
                (estimate.max_fee_per_gas, estimate.max_priority_fee_per_gas)
            }
        };
        let tx = TransactionRequest {
            to: Some(TxKind::Call(to)),
            value: Some(amount),
            max_fee_per_gas: Some(max_fee),
            max_priority_fee_per_gas: Some(priority_fee),
            ..Default::default()
        };

        let pending = self.provider.send_transaction(tx).await?;
        let tx_hash = *pending.tx_hash();
        self.wait_for_receipt(tx_hash).await?;
        Ok(tx_hash)
    }

    /// Poll for a transaction receipt with a timeout.
    async fn wait_for_receipt(&self, tx_hash: B256) -> anyhow::Result<()> {
        let timeout = Duration::from_secs(60);
        let poll_interval = Duration::from_millis(500);
        let start = std::time::Instant::now();

        loop {
            if let Some(receipt) = self.provider.get_transaction_receipt(tx_hash).await? {
                if !receipt.status() {
                    anyhow::bail!("transaction {tx_hash} reverted");
                }
                return Ok(());
            }
            if start.elapsed() > timeout {
                anyhow::bail!("timeout waiting for receipt of {tx_hash}");
            }
            tokio::time::sleep(poll_interval).await;
        }
    }

    /// Get the current block number.
    pub async fn get_block_number(&self) -> anyhow::Result<u64> {
        Ok(self.provider.get_block_number().await?)
    }
}
