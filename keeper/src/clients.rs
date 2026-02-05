use crate::contracts::{EmissionsController, Erc20, JailingPolicy, RewardPolicy};
use alloy::{
    primitives::{Address, U256},
    providers::DynProvider,
};
use blacklight_contract_clients::{HeartbeatManager, StakingOperators};
use contract_clients_common::ProviderContext;
use erc_8004_contract_clients::ValidationRegistryClient;
use erc_8004_contract_clients::validation_registry::ValidationRegistryUpgradeable;
use std::sync::Arc;
use tokio::sync::Mutex;

pub type HeartbeatManagerInstance = HeartbeatManager::HeartbeatManagerInstance<DynProvider>;
pub type StakingOperatorsInstance = StakingOperators::StakingOperatorsInstance<DynProvider>;
pub type JailingPolicyInstance = JailingPolicy::JailingPolicyInstance<DynProvider>;
pub type EmissionsControllerInstance =
    EmissionsController::EmissionsControllerInstance<DynProvider>;
pub type RewardPolicyInstance = RewardPolicy::RewardPolicyInstance<DynProvider>;
pub type ERC20Instance = Erc20::Erc20Instance<DynProvider>;
pub type ValidationRegistryInstance =
    ValidationRegistryUpgradeable::ValidationRegistryUpgradeableInstance<DynProvider>;

/// WebSocket-based client for L2 keeper duties (heartbeat rounds + jailing)
pub struct L2KeeperClient {
    ctx: ProviderContext,
    heartbeat_manager: HeartbeatManagerInstance,
    staking_operators: StakingOperatorsInstance,
    jailing_policy: Option<JailingPolicyInstance>,
    validation_registry: Option<ValidationRegistryClient<DynProvider>>,
}

impl L2KeeperClient {
    pub async fn new(
        rpc_url: String,
        heartbeat_manager_address: Address,
        staking_operators_address: Address,
        jailing_policy_address: Option<Address>,
        validation_registry_address: Option<Address>,
        private_key: String,
    ) -> anyhow::Result<Self> {
        let ctx = ProviderContext::with_ws_retries(&rpc_url, &private_key, Some(u32::MAX)).await?;
        let provider = ctx.provider().clone();
        let tx_lock = ctx.tx_lock();

        let heartbeat_manager =
            HeartbeatManagerInstance::new(heartbeat_manager_address, provider.clone());
        let staking_operators =
            StakingOperatorsInstance::new(staking_operators_address, provider.clone());
        let jailing_policy =
            jailing_policy_address.map(|addr| JailingPolicyInstance::new(addr, provider.clone()));
        let validation_registry = validation_registry_address
            .map(|addr| ValidationRegistryClient::new(provider.clone(), addr, tx_lock));

        Ok(Self {
            ctx,
            heartbeat_manager,
            staking_operators,
            jailing_policy,
            validation_registry,
        })
    }

    pub fn heartbeat_manager(&self) -> &HeartbeatManagerInstance {
        &self.heartbeat_manager
    }

    pub fn staking_operators(&self) -> &StakingOperatorsInstance {
        &self.staking_operators
    }

    pub fn jailing_policy(&self) -> Option<&JailingPolicyInstance> {
        self.jailing_policy.as_ref()
    }

    pub fn validation_registry(&self) -> Option<&ValidationRegistryClient<DynProvider>> {
        self.validation_registry.as_ref()
    }

    pub fn reward_policy(&self, address: Address) -> RewardPolicyInstance {
        RewardPolicyInstance::new(address, self.ctx.provider().clone())
    }

    pub fn erc20(&self, address: Address) -> ERC20Instance {
        ERC20Instance::new(address, self.ctx.provider().clone())
    }

    pub fn provider(&self) -> DynProvider {
        self.ctx.provider().clone()
    }

    pub fn signer_address(&self) -> Address {
        self.ctx.signer_address()
    }

    /// Shared transaction lock for nonce coordination across all contract clients.
    pub fn tx_lock(&self) -> Arc<Mutex<()>> {
        self.ctx.tx_lock()
    }

    pub async fn get_balance(&self) -> anyhow::Result<U256> {
        self.ctx.get_balance().await
    }
}

/// WebSocket-based client for L1 emissions minting/bridging
pub struct L1EmissionsClient {
    ctx: ProviderContext,
    emissions: EmissionsControllerInstance,
}

impl L1EmissionsClient {
    pub async fn new(
        rpc_url: String,
        emissions_address: Address,
        private_key: String,
    ) -> anyhow::Result<Self> {
        let ctx = ProviderContext::new(&rpc_url, &private_key).await?;
        let emissions = EmissionsControllerInstance::new(emissions_address, ctx.provider().clone());
        Ok(Self { ctx, emissions })
    }

    pub fn emissions(&self) -> &EmissionsControllerInstance {
        &self.emissions
    }

    pub fn provider(&self) -> DynProvider {
        self.ctx.provider().clone()
    }

    pub fn signer_address(&self) -> Address {
        self.ctx.signer_address()
    }

    pub async fn get_balance(&self) -> anyhow::Result<U256> {
        self.ctx.get_balance().await
    }
}
