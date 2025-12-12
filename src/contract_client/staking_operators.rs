use alloy::{
    primitives::{Address, B256, U256},
    providers::Provider,
    sol,
};
use std::sync::Arc;
use tokio::sync::Mutex;

// Generate type-safe contract bindings from ABI
sol!(
    #[sol(rpc)]
    #[derive(Debug)]
    StakingOperators,
    "./contracts/out/StakingOperators.sol/StakingOperators.json"
);

// Bring the instance type into scope
use StakingOperators::StakingOperatorsInstance;

/// WebSocket-based client for interacting with the StakingOperators contract
#[derive(Clone)]
pub struct StakingOperatorsClient<P: Provider + Clone> {
    contract: StakingOperatorsInstance<P>,
    tx_lock: Arc<Mutex<()>>,
}

impl<P: Provider + Clone> StakingOperatorsClient<P> {
    /// Create a new WebSocket client from configuration
    pub fn new(
        provider: P,
        config: crate::contract_client::ContractConfig,
        tx_lock: Arc<Mutex<()>>,
    ) -> Self {
        let contract =
            StakingOperatorsInstance::new(config.staking_contract_address, provider.clone());

        Self { contract, tx_lock }
    }

    /// Get the contract address
    pub fn address(&self) -> Address {
        *self.contract.address()
    }

    // ------------------------------------------------------------------------
    // View Functions
    // ------------------------------------------------------------------------

    /// Returns the address of the staking token
    pub async fn staking_token(&self) -> anyhow::Result<Address> {
        // Solidity: function stakingToken() external view returns (address)
        Ok(self.contract.stakingToken().call().await?)
    }

    /// Returns the total stake amount for a specific operator
    pub async fn stake_of(&self, operator: Address) -> anyhow::Result<U256> {
        // Solidity: function stakeOf(address) external view returns (uint256)
        Ok(self.contract.stakeOf(operator).call().await?)
    }

    /// Checks if an operator is active
    pub async fn is_active_operator(&self, operator: Address) -> anyhow::Result<bool> {
        // Solidity: function isActiveOperator(address) external view returns (bool)
        Ok(self.contract.isActiveOperator(operator).call().await?)
    }

    /// Returns a list of all currently active operators
    pub async fn get_active_operators(&self) -> anyhow::Result<Vec<Address>> {
        // Solidity: function getActiveOperators() external view returns (address[])
        Ok(self.contract.getActiveOperators().call().await?)
    }

    /// Returns a list of all registered operators (active and inactive)
    /// This is much more efficient than querying historical events
    pub async fn get_all_operators(&self) -> anyhow::Result<Vec<Address>> {
        // Solidity: function getAllOperators() external view returns (address[])
        Ok(self.contract.getAllOperators().call().await?)
    }

    /// Get all operators who currently have stake > 0
    /// This is the efficient way to discover staked operators without querying events
    pub async fn get_operators_with_stake(&self) -> anyhow::Result<Vec<Address>> {
        // TODO: Use all operators instead of active operators, if desired
        let all_operators = self.get_active_operators().await?;
        let mut operators_with_stake = Vec::new();

        for operator in all_operators {
            let stake = self.stake_of(operator).await?;
            if stake > U256::from(0u8) {
                operators_with_stake.push(operator);
            }
        }

        Ok(operators_with_stake)
    }

    // ------------------------------------------------------------------------
    // Staking Functions
    // ------------------------------------------------------------------------

    /// Stakes tokens to a specific operator
    pub async fn stake_to(&self, operator: Address, amount: U256) -> anyhow::Result<B256> {
        let call = self.contract.stakeTo(operator, amount);

        // Pre-simulate to catch errors with proper messages
        if let Err(e) = call.call().await {
            return Err(Self::decode_error(e));
        }

        let _guard = self.tx_lock.lock().await;
        let pending = call.send().await.map_err(Self::decode_error)?;
        let receipt = pending.get_receipt().await?;

        if !receipt.status() {
            // Re-simulate to get the error message
            if let Err(e) = call.call().await {
                let decoded = super::errors::decode_any_error(&e);
                return Err(anyhow::anyhow!(
                    "stakeTo reverted: {}. Tx hash: {:?}",
                    decoded,
                    receipt.transaction_hash
                ));
            }
            return Err(anyhow::anyhow!(
                "stakeTo reverted on-chain. Tx hash: {:?}",
                receipt.transaction_hash
            ));
        }
        Ok(receipt.transaction_hash)
    }

    /// Requests to unstake tokens from an operator
    pub async fn request_unstake(&self, operator: Address, amount: U256) -> anyhow::Result<B256> {
        let call = self.contract.requestUnstake(operator, amount);

        // Pre-simulate to catch errors with proper messages
        if let Err(e) = call.call().await {
            return Err(Self::decode_error(e));
        }

        let _guard = self.tx_lock.lock().await;
        let pending = call.send().await.map_err(Self::decode_error)?;
        let receipt = pending.get_receipt().await?;

        if !receipt.status() {
            // Re-simulate to get the error message
            if let Err(e) = call.call().await {
                let decoded = super::errors::decode_any_error(&e);
                return Err(anyhow::anyhow!(
                    "requestUnstake reverted: {}. Tx hash: {:?}",
                    decoded,
                    receipt.transaction_hash
                ));
            }
            return Err(anyhow::anyhow!(
                "requestUnstake reverted on-chain. Tx hash: {:?}",
                receipt.transaction_hash
            ));
        }
        Ok(receipt.transaction_hash)
    }

    /// Withdraws unstaked tokens after the unbonding period has passed
    pub async fn withdraw_unstaked(&self, operator: Address) -> anyhow::Result<B256> {
        let call = self.contract.withdrawUnstaked(operator);

        // Pre-simulate to catch errors with proper messages
        if let Err(e) = call.call().await {
            return Err(Self::decode_error(e));
        }

        let _guard = self.tx_lock.lock().await;
        let pending = call.send().await.map_err(Self::decode_error)?;
        let receipt = pending.get_receipt().await?;

        if !receipt.status() {
            // Re-simulate to get the error message
            if let Err(e) = call.call().await {
                let decoded = super::errors::decode_any_error(&e);
                return Err(anyhow::anyhow!(
                    "withdrawUnstaked reverted: {}. Tx hash: {:?}",
                    decoded,
                    receipt.transaction_hash
                ));
            }
            return Err(anyhow::anyhow!(
                "withdrawUnstaked reverted on-chain. Tx hash: {:?}",
                receipt.transaction_hash
            ));
        }
        Ok(receipt.transaction_hash)
    }

    // ------------------------------------------------------------------------
    // Operator Registry Functions
    // ------------------------------------------------------------------------

    /// Registers the caller as an operator or updates their metadata
    pub async fn register_operator(&self, metadata_uri: String) -> anyhow::Result<B256> {
        let call = self.contract.registerOperator(metadata_uri);

        // Pre-simulate to catch errors with proper messages
        if let Err(e) = call.call().await {
            return Err(Self::decode_error(e));
        }

        let _guard = self.tx_lock.lock().await;
        let pending = call.send().await.map_err(Self::decode_error)?;
        let receipt = pending.get_receipt().await?;

        if !receipt.status() {
            // Re-simulate to get the error message
            if let Err(e) = call.call().await {
                let decoded = super::errors::decode_any_error(&e);
                return Err(anyhow::anyhow!(
                    "registerOperator reverted: {}. Tx hash: {:?}",
                    decoded,
                    receipt.transaction_hash
                ));
            }
            return Err(anyhow::anyhow!(
                "registerOperator reverted on-chain. Tx hash: {:?}",
                receipt.transaction_hash
            ));
        }
        Ok(receipt.transaction_hash)
    }

    /// Deactivates the caller as an operator
    pub async fn deactivate_operator(&self) -> anyhow::Result<B256> {
        let call = self.contract.deactivateOperator();

        // Pre-simulate to catch errors with proper messages
        if let Err(e) = call.call().await {
            return Err(Self::decode_error(e));
        }

        let _guard = self.tx_lock.lock().await;
        let pending = call.send().await.map_err(Self::decode_error)?;
        let receipt = pending.get_receipt().await?;

        if !receipt.status() {
            // Re-simulate to get the error message
            if let Err(e) = call.call().await {
                let decoded = super::errors::decode_any_error(&e);
                return Err(anyhow::anyhow!(
                    "deactivateOperator reverted: {}. Tx hash: {:?}",
                    decoded,
                    receipt.transaction_hash
                ));
            }
            return Err(anyhow::anyhow!(
                "deactivateOperator reverted on-chain. Tx hash: {:?}",
                receipt.transaction_hash
            ));
        }
        Ok(receipt.transaction_hash)
    }

    // ------------------------------------------------------------------------
    // Error Handling
    // ------------------------------------------------------------------------

    /// Decode contract errors into human-readable messages
    fn decode_error<E: std::fmt::Display + std::fmt::Debug>(e: E) -> anyhow::Error {
        let error_str = e.to_string();
        let decoded = super::errors::decode_any_error(&e);

        // If we successfully decoded a revert, use that
        if !matches!(decoded, super::errors::DecodedRevert::NoRevertData(_)) {
            return anyhow::anyhow!("Contract reverted: {}", decoded);
        }

        // Common error patterns
        if error_str.contains("insufficient funds") {
            anyhow::anyhow!("Insufficient ETH for gas. Please fund the account.")
        } else if error_str.contains("replacement transaction underpriced") {
            anyhow::anyhow!("Transaction underpriced. A pending transaction may be blocking.")
        } else if error_str.contains("nonce too low") {
            anyhow::anyhow!("Nonce too low. A transaction may have been confirmed already.")
        } else {
            anyhow::anyhow!("Transaction failed: {}", e)
        }
    }
}
