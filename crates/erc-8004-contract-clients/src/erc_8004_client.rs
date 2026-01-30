use crate::{ContractConfig, IdentityRegistryClient, ValidationRegistryClient};
use alloy::{
    network::{Ethereum, EthereumWallet, NetworkWallet},
    primitives::{Address, B256, TxKind, U256},
    providers::{DynProvider, Provider, ProviderBuilder, WsConnect},
    rpc::types::TransactionRequest,
    signers::local::PrivateKeySigner,
};
use std::sync::Arc;
use tokio::sync::Mutex;

/// High-level wrapper bundling ERC-8004 contract clients with a shared Alloy provider.
#[derive(Clone)]
pub struct Erc8004Client {
    provider: DynProvider,
    wallet: EthereumWallet,
    pub identity_registry: IdentityRegistryClient<DynProvider>,
    pub validation_registry: ValidationRegistryClient<DynProvider>,
}

impl Erc8004Client {
    pub async fn new(config: ContractConfig, private_key: String) -> anyhow::Result<Self> {
        let rpc_url = config.rpc_url.clone();
        let ws_url = rpc_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");

        let ws = WsConnect::new(ws_url);
        let signer: PrivateKeySigner = private_key.parse::<PrivateKeySigner>()?;
        let wallet = EthereumWallet::from(signer);

        // Build a provider that can sign transactions, then erase the concrete type
        let provider: DynProvider = ProviderBuilder::new()
            .wallet(wallet.clone())
            .with_simple_nonce_management()
            .with_gas_estimation()
            .connect_ws(ws)
            .await?
            .erased();

        let tx_lock = Arc::new(Mutex::new(()));

        // Instantiate contract clients using the shared provider
        let identity_registry = IdentityRegistryClient::new(
            provider.clone(),
            config.identity_registry_contract_address,
            tx_lock.clone(),
        );
        let validation_registry = ValidationRegistryClient::new(
            provider.clone(),
            config.validation_registry_contract_address,
            tx_lock.clone(),
        );

        Ok(Self {
            provider,
            wallet,
            identity_registry,
            validation_registry,
        })
    }

    /// Get the signer address
    pub fn signer_address(&self) -> Address {
        <EthereumWallet as NetworkWallet<Ethereum>>::default_signer_address(&self.wallet)
    }

    /// Get the balance of the wallet
    pub async fn get_balance(&self) -> anyhow::Result<U256> {
        let address = self.signer_address();
        Ok(self.provider.get_balance(address).await?)
    }

    /// Get the balance of a specific address
    pub async fn get_balance_of(&self, address: Address) -> anyhow::Result<U256> {
        Ok(self.provider.get_balance(address).await?)
    }

    /// Send ETH to an address
    pub async fn send_eth(&self, to: Address, amount: U256) -> anyhow::Result<B256> {
        let tx = TransactionRequest {
            to: Some(TxKind::Call(to)),
            value: Some(amount),
            max_priority_fee_per_gas: Some(0),
            ..Default::default()
        };

        let tx_hash = self.provider.send_transaction(tx).await?.watch().await?;

        Ok(tx_hash)
    }

    /// Get the current block number
    pub async fn get_block_number(&self) -> anyhow::Result<u64> {
        Ok(self.provider.get_block_number().await?)
    }
}
