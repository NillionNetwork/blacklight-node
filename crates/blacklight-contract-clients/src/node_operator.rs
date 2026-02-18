use alloy::{
    primitives::{Address, U256},
    providers::Provider,
    sol,
};
use anyhow::Result;

sol!(
    #[sol(rpc)]
    #[derive(Debug)]
    contract NodeOperator {
        // Errors
        error ZeroAddress();
        error ZeroAmount();
        error ContractNotConfigured();
        error InsufficientStake();
        error BelowMinimumStake();
        error NothingToClaim();
        error FeeTooHigh();
        error FactoryOnly();
        error InvalidUserAssignment();

        // Events
        event NodeAssigned(address indexed user, address indexed node);
        event NodeReleased(address indexed user, address indexed node);
        event Staked(address indexed user, uint256 amount, address indexed node);
        event UnstakeRequested(address indexed user, uint256 amount, address indexed node);
        event UnstakedWithdrawn(address indexed user, uint256 amount, address indexed node);
        event RewardsHarvested(uint256 totalHarvested, uint256 fee);
        event RewardsClaimed(address indexed user, uint256 amount);
        event FeesCollected(uint256 amount);

        // View functions
        function owner() external view returns (address);
        function nodeAddress() external view returns (address);
        function nodeUser() external view returns (address);
        function routerFactory() external view returns (address);
        function stakingOperators() external view returns (address);
        function rewardPolicy() external view returns (address);
        function stakingToken() external view returns (address);
        function rewardToken() external view returns (address);
        function withdrawFeeBps() external view returns (uint256);
        function restakeFeeBps() external view returns (uint256);
        function rewardBehavior() external view returns (uint8);
        function minStake() external view returns (uint256);
    }
);

use NodeOperator::NodeOperatorInstance;

/// Read-only client for interacting with a NodeOperator contract instance
#[derive(Clone)]
pub struct NodeOperatorClient<P: Provider + Clone> {
    contract: NodeOperatorInstance<P>,
}

impl<P: Provider + Clone> NodeOperatorClient<P> {
    pub fn new(provider: P, address: Address) -> Self {
        let contract = NodeOperatorInstance::new(address, provider);
        Self { contract }
    }

    /// Get the contract address
    pub fn address(&self) -> Address {
        *self.contract.address()
    }

    pub async fn owner(&self) -> Result<Address> {
        Ok(self.contract.owner().call().await?)
    }

    pub async fn node_address(&self) -> Result<Address> {
        Ok(self.contract.nodeAddress().call().await?)
    }

    pub async fn node_user(&self) -> Result<Address> {
        Ok(self.contract.nodeUser().call().await?)
    }

    pub async fn router_factory(&self) -> Result<Address> {
        Ok(self.contract.routerFactory().call().await?)
    }

    pub async fn staking_operators(&self) -> Result<Address> {
        Ok(self.contract.stakingOperators().call().await?)
    }

    pub async fn reward_policy(&self) -> Result<Address> {
        Ok(self.contract.rewardPolicy().call().await?)
    }

    pub async fn staking_token(&self) -> Result<Address> {
        Ok(self.contract.stakingToken().call().await?)
    }

    pub async fn reward_token(&self) -> Result<Address> {
        Ok(self.contract.rewardToken().call().await?)
    }

    pub async fn withdraw_fee_bps(&self) -> Result<U256> {
        Ok(self.contract.withdrawFeeBps().call().await?)
    }

    pub async fn restake_fee_bps(&self) -> Result<U256> {
        Ok(self.contract.restakeFeeBps().call().await?)
    }

    pub async fn reward_behavior(&self) -> Result<u8> {
        Ok(self.contract.rewardBehavior().call().await?)
    }

    pub async fn min_stake(&self) -> Result<U256> {
        Ok(self.contract.minStake().call().await?)
    }
}
