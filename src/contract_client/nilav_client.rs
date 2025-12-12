use crate::contract_client::{
    ContractConfig, NilAVRouterClient, StakingOperatorsClient, TESTTokenClient,
};

use alloy::{
    network::{Ethereum, EthereumWallet, NetworkWallet},
    primitives::{Address, B256, TxKind, U256},
    providers::{DynProvider, Provider, ProviderBuilder, WsConnect},
    rpc::types::TransactionRequest,
    signers::local::PrivateKeySigner,
};

/// High-level wrapper bundling all contract clients with a shared Alloy provider.
pub struct NilAVClient {
    provider: DynProvider,
    wallet: EthereumWallet,
    pub router: NilAVRouterClient<DynProvider>,
    pub token: TESTTokenClient<DynProvider>,
    pub staking: StakingOperatorsClient<DynProvider>,
}

impl NilAVClient {
    pub async fn new(config: ContractConfig, private_key: String) -> anyhow::Result<Self> {
        let rpc_url = config.rpc_url.clone();
        let ws_url = rpc_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");

        // Build WS transport and signer wallet
        let ws = WsConnect::new(ws_url).with_max_retries(u32::MAX);
        let signer: PrivateKeySigner = private_key.parse::<PrivateKeySigner>()?;
        let wallet = EthereumWallet::from(signer);

        // Build a provider that can sign transactions, then erase the concrete type
        let provider: DynProvider = ProviderBuilder::new()
            .wallet(wallet.clone())
            .with_simple_nonce_management()
            .connect_ws(ws)
            .await?
            .erased();
        // Instantiate contract clients using the shared provider
        let router = NilAVRouterClient::new(provider.clone(), config.clone());
        let token = TESTTokenClient::new(provider.clone(), config.clone());
        let staking = StakingOperatorsClient::new(provider.clone(), config);

        Ok(Self {
            provider,
            wallet,
            router,
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
