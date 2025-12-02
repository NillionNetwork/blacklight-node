use crate::contract_client::ContractConfig;
use crate::contract_client::{
    NilAVRouterClient, SignedWsProvider, StakingOperatorsClient, TESTTokenClient,
};
use ethers::{
    core::types::{Address, U256},
    middleware::{NonceManagerMiddleware, SignerMiddleware},
    providers::{Middleware, Provider, Ws},
    signers::{LocalWallet, Signer},
};
use std::sync::Arc;

pub struct NilAVClient {
    provider: Arc<SignedWsProvider>,
    pub router: NilAVRouterClient,
    pub token: TESTTokenClient,
    pub staking: StakingOperatorsClient,
}

impl NilAVClient {
    pub async fn new(config: ContractConfig, private_key: String) -> anyhow::Result<Self> {
        let rpc_url = &config.rpc_url;
        // Convert HTTP URL to WebSocket URL
        let ws_url = rpc_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");

        // Connect with keepalive enabled (10 second interval)
        let provider = Provider::<Ws>::connect_with_reconnects(&ws_url, usize::MAX).await?;
        let chain_id = provider.get_chainid().await?;

        let wallet = private_key
            .parse::<LocalWallet>()
            .expect("Invalid private key")
            .with_chain_id(chain_id.as_u64());

        // Wrap with SignerMiddleware first, then NonceManagerMiddleware to handle concurrent txs
        let wallet_address = wallet.address();
        let signer_middleware = SignerMiddleware::new(provider, wallet);
        let provider = Arc::new(NonceManagerMiddleware::new(
            signer_middleware,
            wallet_address,
        ));

        let router = NilAVRouterClient::new(provider.clone(), config.clone());
        let token = TESTTokenClient::new(provider.clone(), config.clone());
        let staking = StakingOperatorsClient::new(provider.clone(), config)?;

        Ok(Self {
            provider,
            router,
            token,
            staking,
        })
    }

    /// Get the signer address
    pub fn signer_address(&self) -> Address {
        self.provider.inner().signer().address()
    }

    /// Get the balance of the wallet
    pub async fn get_balance(&self) -> anyhow::Result<U256> {
        let address = self.signer_address();
        Ok(self.provider.get_balance(address, None).await?)
    }
}
