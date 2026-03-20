use alloy::{
    primitives::{Address, B256, U256},
    providers::Provider,
    sol,
};
use anyhow::Result;
use contract_clients_common::errors::AllErrorsErrors;
use contract_clients_common::tx_submitter::TransactionSubmitter;
use std::sync::Arc;
use tokio::sync::Mutex;

sol!(
    #[sol(rpc)]
    #[derive(Debug)]
    contract NodeOperatorFactory {
        // Errors
        error ZeroAddress();
        error NoBoundNodeOperator();
        error InvalidNodeOperator();
        error NoFreeNodeOperator();
        error NodeAlreadyRegistered();
        error NodeNotRegistered();
        error NodeCurrentlyAssigned();
        error FactoryNotConfigured();
        error InsufficientFees();

        // Events
        event NodeOperatorCreated(address indexed node, address indexed nodeOperator);
        event NodeRemoved(address indexed node, address indexed nodeOperator);
        event UserBoundToNodeOperator(address indexed user, address indexed nodeOperator);
        event UserUnboundFromNodeOperator(address indexed user, address indexed nodeOperator);
        event FeesWithdrawn(uint256 amount, address indexed to);

        // Public state getters
        function stakingOperators() external view returns (address);
        function rewardPolicy() external view returns (address);
        function token() external view returns (address);
        function defaultWithdrawFeeBps() external view returns (uint256);
        function defaultRestakeFeeBps() external view returns (uint256);
        function minStake() external view returns (uint256);

        // Bidirectional lookups
        function operatorToNode(address operator) external view returns (address);
        function userToOperator(address user) external view returns (address);
        function userToNode(address user) external view returns (address);
        function nodeToUser(address node) external view returns (address);

        // Config setters (onlyOwner)
        function setStakingOperators(address addr) external;
        function setRewardPolicy(address addr) external;
        function setToken(address addr) external;
        function setDefaultModeFeeBps(uint256 withdrawBps, uint256 restakeBps) external;
        function setOperatorModeFeeBps(address operatorAddr, uint256 withdrawBps, uint256 restakeBps) external;
        function setMinStake(uint256 newMinStake) external;
        function withdrawFees(uint256 amount, address to) external;

        // Node management (onlyOwner)
        function addNode(address node) external returns (address);
        function addNodes(address[] calldata nodes) external;
        function removeNode(address node) external;

        // User staking
        function stake(uint256 amount) external;
        function requestUnstake(uint256 amount) external;
        function withdrawUnstaked() external;
        function claimRewards() external;
        function setMyRewardBehavior(uint8 behavior) external;
        function pendingRewards(address user) external view returns (uint256);

        // Harvest rewards
        function harvestRewards(address operatorAddr) external;
        function harvestAllRewards() external;

        // View functions
        function allNodes() external view returns (address[] memory);
        function nodeCount() external view returns (uint256);
        function allNodeOperators() external view returns (address[] memory);
        function isFreeNode(address node) external view returns (bool);
        function freeNodeCount() external view returns (uint256);
        function nodeToOperator(address node) external view returns (address);
        function myRewardBehavior(address user) external view returns (uint8);
        function operatorModeFeeBps(address operatorAddr) external view returns (uint256 withdrawBps, uint256 restakeBps);
        function predictNodeOperatorAddress(address node) external view returns (address);
    }
);

use NodeOperatorFactory::NodeOperatorFactoryInstance;

/// Client for interacting with the NodeOperatorFactory contract
#[derive(Clone)]
pub struct NodeOperatorFactoryClient<P: Provider + Clone> {
    contract: NodeOperatorFactoryInstance<P>,
    submitter: TransactionSubmitter<AllErrorsErrors>,
}

impl<P: Provider + Clone> NodeOperatorFactoryClient<P> {
    pub fn new(provider: P, address: Address, tx_lock: Arc<Mutex<()>>) -> Self {
        let contract = NodeOperatorFactoryInstance::new(address, provider);
        let submitter = TransactionSubmitter::new(tx_lock);
        Self {
            contract,
            submitter,
        }
    }

    /// Get the contract address
    pub fn address(&self) -> Address {
        *self.contract.address()
    }

    // ------------------------------------------------------------------------
    // View Functions
    // ------------------------------------------------------------------------

    pub async fn staking_operators(&self) -> Result<Address> {
        Ok(self.contract.stakingOperators().call().await?)
    }

    pub async fn reward_policy(&self) -> Result<Address> {
        Ok(self.contract.rewardPolicy().call().await?)
    }

    pub async fn token(&self) -> Result<Address> {
        Ok(self.contract.token().call().await?)
    }

    pub async fn default_withdraw_fee_bps(&self) -> Result<U256> {
        Ok(self.contract.defaultWithdrawFeeBps().call().await?)
    }

    pub async fn default_restake_fee_bps(&self) -> Result<U256> {
        Ok(self.contract.defaultRestakeFeeBps().call().await?)
    }

    pub async fn min_stake(&self) -> Result<U256> {
        Ok(self.contract.minStake().call().await?)
    }

    pub async fn node_to_operator(&self, node: Address) -> Result<Address> {
        Ok(self.contract.nodeToOperator(node).call().await?)
    }

    pub async fn operator_to_node(&self, operator: Address) -> Result<Address> {
        Ok(self.contract.operatorToNode(operator).call().await?)
    }

    pub async fn user_to_operator(&self, user: Address) -> Result<Address> {
        Ok(self.contract.userToOperator(user).call().await?)
    }

    pub async fn user_to_node(&self, user: Address) -> Result<Address> {
        Ok(self.contract.userToNode(user).call().await?)
    }

    pub async fn node_to_user(&self, node: Address) -> Result<Address> {
        Ok(self.contract.nodeToUser(node).call().await?)
    }

    pub async fn is_free_node(&self, node: Address) -> Result<bool> {
        Ok(self.contract.isFreeNode(node).call().await?)
    }

    pub async fn node_count(&self) -> Result<U256> {
        Ok(self.contract.nodeCount().call().await?)
    }

    pub async fn free_node_count(&self) -> Result<U256> {
        Ok(self.contract.freeNodeCount().call().await?)
    }

    pub async fn all_nodes(&self) -> Result<Vec<Address>> {
        Ok(self.contract.allNodes().call().await?)
    }

    pub async fn all_node_operators(&self) -> Result<Vec<Address>> {
        Ok(self.contract.allNodeOperators().call().await?)
    }

    pub async fn pending_rewards(&self, user: Address) -> Result<U256> {
        Ok(self.contract.pendingRewards(user).call().await?)
    }

    pub async fn my_reward_behavior(&self, user: Address) -> Result<u8> {
        Ok(self.contract.myRewardBehavior(user).call().await?)
    }

    pub async fn operator_mode_fee_bps(&self, operator: Address) -> Result<(U256, U256)> {
        let result = self.contract.operatorModeFeeBps(operator).call().await?;
        Ok((result.withdrawBps, result.restakeBps))
    }

    pub async fn predict_node_operator_address(&self, node: Address) -> Result<Address> {
        Ok(self
            .contract
            .predictNodeOperatorAddress(node)
            .call()
            .await?)
    }

    // ------------------------------------------------------------------------
    // Owner Config Functions
    // ------------------------------------------------------------------------

    pub async fn set_staking_operators(&self, addr: Address) -> Result<B256> {
        let call = self.contract.setStakingOperators(addr);
        self.submitter.invoke("setStakingOperators", call).await
    }

    pub async fn set_reward_policy(&self, addr: Address) -> Result<B256> {
        let call = self.contract.setRewardPolicy(addr);
        self.submitter.invoke("setRewardPolicy", call).await
    }

    pub async fn set_token(&self, addr: Address) -> Result<B256> {
        let call = self.contract.setToken(addr);
        self.submitter.invoke("setToken", call).await
    }

    pub async fn set_default_mode_fee_bps(
        &self,
        withdraw_bps: U256,
        restake_bps: U256,
    ) -> Result<B256> {
        let call = self
            .contract
            .setDefaultModeFeeBps(withdraw_bps, restake_bps);
        self.submitter.invoke("setDefaultModeFeeBps", call).await
    }

    pub async fn set_operator_mode_fee_bps(
        &self,
        operator: Address,
        withdraw_bps: U256,
        restake_bps: U256,
    ) -> Result<B256> {
        let call = self
            .contract
            .setOperatorModeFeeBps(operator, withdraw_bps, restake_bps);
        self.submitter.invoke("setOperatorModeFeeBps", call).await
    }

    pub async fn set_min_stake(&self, amount: U256) -> Result<B256> {
        let call = self.contract.setMinStake(amount);
        self.submitter.invoke("setMinStake", call).await
    }

    // ------------------------------------------------------------------------
    // Owner Node Management
    // ------------------------------------------------------------------------

    pub async fn add_node(&self, node: Address) -> Result<B256> {
        let call = self.contract.addNode(node);
        self.submitter.invoke("addNode", call).await
    }

    pub async fn add_nodes(&self, nodes: Vec<Address>) -> Result<B256> {
        let call = self.contract.addNodes(nodes);
        self.submitter.invoke("addNodes", call).await
    }

    pub async fn remove_node(&self, node: Address) -> Result<B256> {
        let call = self.contract.removeNode(node);
        self.submitter.invoke("removeNode", call).await
    }

    // ------------------------------------------------------------------------
    // Owner Rewards
    // ------------------------------------------------------------------------

    pub async fn harvest_rewards(&self, operator: Address) -> Result<B256> {
        let call = self.contract.harvestRewards(operator);
        self.submitter.invoke("harvestRewards", call).await
    }

    pub async fn harvest_all_rewards(&self) -> Result<B256> {
        let call = self.contract.harvestAllRewards();
        self.submitter.invoke("harvestAllRewards", call).await
    }

    pub async fn withdraw_fees(&self, amount: U256, to: Address) -> Result<B256> {
        let call = self.contract.withdrawFees(amount, to);
        self.submitter.invoke("withdrawFees", call).await
    }

    // ------------------------------------------------------------------------
    // User Staking
    // ------------------------------------------------------------------------

    pub async fn stake(&self, amount: U256) -> Result<B256> {
        let call = self.contract.stake(amount);
        self.submitter.invoke("stake", call).await
    }

    pub async fn request_unstake(&self, amount: U256) -> Result<B256> {
        let call = self.contract.requestUnstake(amount);
        self.submitter.invoke("requestUnstake", call).await
    }

    pub async fn withdraw_unstaked(&self) -> Result<B256> {
        let call = self.contract.withdrawUnstaked();
        self.submitter.invoke("withdrawUnstaked", call).await
    }

    pub async fn claim_rewards(&self) -> Result<B256> {
        let call = self.contract.claimRewards();
        self.submitter.invoke("claimRewards", call).await
    }

    pub async fn set_my_reward_behavior(&self, behavior: u8) -> Result<B256> {
        let call = self.contract.setMyRewardBehavior(behavior);
        self.submitter.invoke("setMyRewardBehavior", call).await
    }
}
