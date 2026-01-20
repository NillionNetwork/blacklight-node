use crate::contract_client::{
    ContractConfig, HeartbeatManagerClient, NilTokenClient, StakingOperatorsClient,
};

use alloy::{
    network::{Ethereum, EthereumWallet, NetworkWallet},
    primitives::{Address, TxKind, B256, U256},
    providers::{DynProvider, Provider, ProviderBuilder, WsConnect},
    rpc::types::TransactionRequest,
    signers::local::PrivateKeySigner,
};
use std::sync::Arc;
use tokio::sync::Mutex;

/// High-level wrapper bundling all contract clients with a shared Alloy provider.
#[derive(Clone)]
pub struct NilUVClient {
    provider: DynProvider,
    wallet: EthereumWallet,
    pub manager: HeartbeatManagerClient<DynProvider>,
    pub token: NilTokenClient<DynProvider>,
    pub staking: StakingOperatorsClient<DynProvider>,
}

impl NilUVClient {
    pub async fn new(config: ContractConfig, private_key: String) -> anyhow::Result<Self> {
        let rpc_url = config.rpc_url.clone();
        let ws_url = rpc_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");

        // Build WS transport with configurable retries
        let ws = WsConnect::new(ws_url).with_max_retries(config.max_ws_retries);
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
        let manager =
            HeartbeatManagerClient::new(provider.clone(), config.clone(), tx_lock.clone());
        let token = NilTokenClient::new(provider.clone(), config.clone(), tx_lock.clone());
        let staking = StakingOperatorsClient::new(provider.clone(), config, tx_lock.clone());

        Ok(Self {
            provider,
            wallet,
            manager,
            token,
            staking,
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
            ..Default::default()
        };

        let tx_hash = self.provider.send_transaction(tx).await?.watch().await?;

        Ok(tx_hash)
    }
}
