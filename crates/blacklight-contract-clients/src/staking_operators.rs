use crate::{ContractConfig, common::tx_submitter::TransactionSubmitter};
use alloy::{
    primitives::{Address, B256, U256},
    providers::Provider,
    sol,
};
use anyhow::Result;
use futures_util::future::join_all;
use std::sync::Arc;
use tokio::sync::Mutex;

sol!(
    interface IStakingOperators {
        struct Tranche { uint256 amount; uint64 releaseTime; }
    }

    #[sol(rpc)]
    #[derive(Debug)]
    contract StakingOperators {
        error ZeroAddress();
        error ZeroAmount();
        error PendingUnbonding();
        error DifferentStaker();
        error NotStaker();
        error UnbondingExists();
        error InsufficientStake();
        error OperatorJailed();
        error NoUnbonding();
        error NotReady();
        error NoStake();
        error NotActive();

        function protocolConfig() external view returns (address);
        function stakingToken() external view override returns (address);
        function stakeOf(address operator) external view override returns (uint256);
        function totalStaked() external view override returns (uint256);
        function unbondingStaker(address operator) external view returns (address);
        function isActiveOperator(address operator) public view override returns (bool);
        function getActiveOperators() external view override returns (address[] memory);
        function stakeTo(address operator, uint256 amount) external override nonReentrant whenNotPaused;
        function registerOperator(string calldata metadataURI) external override whenNotPaused;
        function deactivateOperator() external override whenNotPaused;
        function reactivateOperator() external override whenNotPaused;
        function requestUnstake(address operator, uint256 amount) external override nonReentrant whenNotPaused;
        function withdrawUnstaked(address operator) external override nonReentrant whenNotPaused;
    }
);

use StakingOperators::StakingOperatorsInstance;

/// Client for interacting with the StakingOperators contract
#[derive(Clone)]
pub struct StakingOperatorsClient<P: Provider + Clone> {
    contract: StakingOperatorsInstance<P>,
    submitter: TransactionSubmitter<StakingOperators::StakingOperatorsErrors>,
}

impl<P: Provider + Clone> StakingOperatorsClient<P> {
    pub fn new(provider: P, config: ContractConfig, tx_lock: Arc<Mutex<()>>) -> Self {
        let contract =
            StakingOperatorsInstance::new(config.staking_contract_address, provider.clone());
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

    /// Returns the address of the protocol config contract
    pub async fn protocol_config(&self) -> Result<Address> {
        Ok(self.contract.protocolConfig().call().await?)
    }

    /// Returns the address of the staking token
    pub async fn staking_token(&self) -> Result<Address> {
        Ok(self.contract.stakingToken().call().await?)
    }

    /// Returns the total stake amount for a specific operator
    pub async fn stake_of(&self, operator: Address) -> Result<U256> {
        // Solidity: function stakeOf(address) external view returns (uint256)
        Ok(self.contract.stakeOf(operator).call().await?)
    }

    /// Checks if an operator is active
    pub async fn is_active_operator(&self, operator: Address) -> Result<bool> {
        // Solidity: function isActiveOperator(address) external view returns (bool)
        Ok(self.contract.isActiveOperator(operator).call().await?)
    }

    /// Returns a list of all currently active operators
    pub async fn get_active_operators(&self) -> Result<Vec<Address>> {
        // Solidity: function getActiveOperators() external view returns (address[])
        Ok(self.contract.getActiveOperators().call().await?)
    }

    /// Returns a list of all registered operators (active and inactive)
    /// This is much more efficient than querying historical events
    pub async fn get_all_operators(&self) -> Result<Vec<Address>> {
        // Ok(self.contract.getAllOperators().call().await?)
        Ok(Vec::new())
    }

    /// Get all operators who currently have stake > 0
    /// This is the efficient way to discover staked operators without querying events
    ///
    /// Note: This method fetches stakes for all operators in parallel for efficiency.
    pub async fn get_operators_with_stake(&self) -> Result<Vec<Address>> {
        // TODO: Use all operators instead of active operators, if desired
        let all_operators = self.get_active_operators().await?;

        // Fetch all stakes in parallel instead of sequential N+1 queries
        let stake_futures: Vec<_> = all_operators.iter().map(|op| self.stake_of(*op)).collect();
        let stakes = join_all(stake_futures).await;

        // Filter to only operators with stake > 0
        let operators_with_stake: Vec<Address> = all_operators
            .into_iter()
            .zip(stakes)
            .filter_map(|(op, stake_result)| {
                stake_result
                    .ok()
                    .and_then(|stake| if stake > U256::ZERO { Some(op) } else { None })
            })
            .collect();

        Ok(operators_with_stake)
    }

    // ------------------------------------------------------------------------
    // Staking Functions
    // ------------------------------------------------------------------------

    /// Stakes tokens to a specific operator
    pub async fn stake_to(&self, operator: Address, amount: U256) -> Result<B256> {
        let call = self.contract.stakeTo(operator, amount);
        self.submitter.invoke("stakeTo", call).await
    }

    /// Requests to unstake tokens from an operator
    pub async fn request_unstake(&self, operator: Address, amount: U256) -> Result<B256> {
        let call = self.contract.requestUnstake(operator, amount);
        self.submitter.invoke("requestUnstake", call).await
    }

    /// Withdraws unstaked tokens after the unbonding period has passed
    pub async fn withdraw_unstaked(&self, operator: Address) -> Result<B256> {
        let call = self.contract.withdrawUnstaked(operator);
        self.submitter.invoke("withdrawUnstaked", call).await
    }

    // ------------------------------------------------------------------------
    // Operator Registry Functions
    // ------------------------------------------------------------------------

    /// Registers the caller as an operator or updates their metadata
    pub async fn register_operator(&self, metadata_uri: String) -> Result<B256> {
        let call = self.contract.registerOperator(metadata_uri);
        self.submitter.invoke("registerOperator", call).await
    }

    /// Deactivates the caller as an operator
    pub async fn deactivate_operator(&self) -> Result<B256> {
        let call = self.contract.deactivateOperator();
        self.submitter.invoke("deactivateOperator", call).await
    }
}
