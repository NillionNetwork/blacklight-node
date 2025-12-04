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
    pub router: Arc<NilAVRouterClient>,
    pub token: Arc<TESTTokenClient>,
    pub staking: Arc<StakingOperatorsClient>,
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

        let router = Arc::new(NilAVRouterClient::new(provider.clone(), config.clone()));
        let token = Arc::new(TESTTokenClient::new(provider.clone(), config.clone()));
        let staking = Arc::new(StakingOperatorsClient::new(
            provider.clone(),
            config.clone(),
        ));

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

    /// Get the balance of a specific address
    pub async fn get_balance_of(&self, address: Address) -> anyhow::Result<U256> {
        Ok(self.provider.get_balance(address, None).await?)
    }

    /// Send ETH to an address
    pub async fn send_eth(&self, to: Address, amount: U256) -> anyhow::Result<ethers::types::H256> {
        use ethers::providers::Middleware;
        use ethers::types::TransactionRequest;

        let tx = TransactionRequest::new().to(to).value(amount);

        let pending_tx = self.provider.send_transaction(tx, None).await?;
        let receipt = pending_tx.await?;
        let receipt = receipt.ok_or_else(|| anyhow::anyhow!("No transaction receipt"))?;
        Ok(receipt.transaction_hash)
    }
}
