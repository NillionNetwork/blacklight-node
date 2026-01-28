use crate::contracts::{EmissionsController, Erc20, JailingPolicy, RewardPolicy};
use alloy::{
    network::{Ethereum, EthereumWallet, NetworkWallet},
    primitives::{Address, U256},
    providers::{DynProvider, Provider, ProviderBuilder, WsConnect},
    signers::local::PrivateKeySigner,
};
use blacklight_contract_clients::HearbeatManager;

pub type HeartbeatManagerInstance = HearbeatManager::HearbeatManagerInstance<DynProvider>;
pub type JailingPolicyInstance = JailingPolicy::JailingPolicyInstance<DynProvider>;
pub type EmissionsControllerInstance =
    EmissionsController::EmissionsControllerInstance<DynProvider>;
pub type RewardPolicyInstance = RewardPolicy::RewardPolicyInstance<DynProvider>;
pub type ERC20Instance = Erc20::Erc20Instance<DynProvider>;

async fn connect_ws(
    rpc_url: &str,
    private_key: &str,
) -> anyhow::Result<(DynProvider, EthereumWallet)> {
    let ws_url = rpc_url
        .replace("http://", "ws://")
        .replace("https://", "wss://");
    let ws = WsConnect::new(ws_url).with_max_retries(u32::MAX);
    let signer: PrivateKeySigner = private_key.parse::<PrivateKeySigner>()?;
    let wallet = EthereumWallet::from(signer);

    let provider: DynProvider = ProviderBuilder::new()
        .wallet(wallet.clone())
        .with_simple_nonce_management()
        .with_gas_estimation()
        .connect_ws(ws)
        .await?
        .erased();

    Ok((provider, wallet))
}

/// WebSocket-based client for L2 keeper duties (heartbeat rounds + jailing)
pub struct L2KeeperClient {
    heartbeat_manager: HeartbeatManagerInstance,
    jailing_policy: Option<JailingPolicyInstance>,
    provider: DynProvider,
    wallet: EthereumWallet,
}

impl L2KeeperClient {
    pub async fn new(
        rpc_url: String,
        heartbeat_manager_address: Address,
        jailing_policy_address: Option<Address>,
        private_key: String,
    ) -> anyhow::Result<Self> {
        let (provider, wallet) = connect_ws(&rpc_url, &private_key).await?;
        let heartbeat_manager =
            HeartbeatManagerInstance::new(heartbeat_manager_address, provider.clone());
        let jailing_policy =
            jailing_policy_address.map(|addr| JailingPolicyInstance::new(addr, provider.clone()));

        Ok(Self {
            heartbeat_manager,
            jailing_policy,
            provider,
            wallet,
        })
    }

    pub fn heartbeat_manager(&self) -> &HeartbeatManagerInstance {
        &self.heartbeat_manager
    }

    pub fn jailing_policy(&self) -> Option<&JailingPolicyInstance> {
        self.jailing_policy.as_ref()
    }

    pub fn reward_policy(&self, address: Address) -> RewardPolicyInstance {
        RewardPolicyInstance::new(address, self.provider.clone())
    }

    pub fn erc20(&self, address: Address) -> ERC20Instance {
        ERC20Instance::new(address, self.provider.clone())
    }

    pub fn provider(&self) -> DynProvider {
        self.provider.clone()
    }

    pub fn signer_address(&self) -> Address {
        <EthereumWallet as NetworkWallet<Ethereum>>::default_signer_address(&self.wallet)
    }

    pub async fn get_balance(&self) -> anyhow::Result<U256> {
        Ok(self.provider.get_balance(self.signer_address()).await?)
    }
}

/// WebSocket-based client for L1 emissions minting/bridging
pub struct L1EmissionsClient {
    emissions: EmissionsControllerInstance,
    provider: DynProvider,
    wallet: EthereumWallet,
}

impl L1EmissionsClient {
    pub async fn new(
        rpc_url: String,
        emissions_address: Address,
        private_key: String,
    ) -> anyhow::Result<Self> {
        let (provider, wallet) = connect_ws(&rpc_url, &private_key).await?;
        let emissions = EmissionsControllerInstance::new(emissions_address, provider.clone());
        Ok(Self {
            emissions,
            provider,
            wallet,
        })
    }

    pub fn emissions(&self) -> &EmissionsControllerInstance {
        &self.emissions
    }

    pub fn provider(&self) -> DynProvider {
        self.provider.clone()
    }

    pub fn signer_address(&self) -> Address {
        <EthereumWallet as NetworkWallet<Ethereum>>::default_signer_address(&self.wallet)
    }

    pub async fn get_balance(&self) -> anyhow::Result<U256> {
        Ok(self.provider.get_balance(self.signer_address()).await?)
    }
}
