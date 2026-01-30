use crate::common::tx_submitter::TransactionSubmitter;
use alloy::{
    primitives::{Address, B256, U256},
    providers::Provider,
    sol,
};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

sol! {
    #[sol(rpc)]
    #[derive(Debug)]
    contract ValidationRegistryUpgradeable {
        function validationRequest(
            address validatorAddress,
            uint256 agentId,
            string calldata requestURI,
            bytes32 requestHash,
            uint64 snapshotId
        ) external;
    }
}

use ValidationRegistryUpgradeable::ValidationRegistryUpgradeableInstance;

/// Client for interacting with the ValidationRegistryUpgradeable contract.
#[derive(Clone)]
pub struct ValidationRegistryClient<P: Provider + Clone> {
    contract: ValidationRegistryUpgradeableInstance<P>,
    submitter: TransactionSubmitter<crate::common::errors::StandardErrors::StandardErrorsErrors>,
}

impl<P: Provider + Clone> ValidationRegistryClient<P> {
    pub fn new(provider: P, address: Address, tx_lock: Arc<Mutex<()>>) -> Self {
        let contract = ValidationRegistryUpgradeableInstance::new(address, provider);
        let submitter = TransactionSubmitter::new(tx_lock);
        Self {
            contract,
            submitter,
        }
    }

    /// Get the contract address.
    pub fn address(&self) -> Address {
        *self.contract.address()
    }

    /// Rust stub for the Solidity `requestValidation` semantics (no snapshotId).
    pub async fn request_validation(
        &self,
        validator_address: Address,
        agent_id: U256,
        request_uri: String,
        request_hash: B256,
    ) -> Result<B256> {
        let call = self.contract.validationRequest(
            validator_address,
            agent_id,
            request_uri,
            request_hash,
            0,
        );
        self.submitter.invoke("validationRequest", call).await
    }

    /// Full validation request with snapshot ID (delegates to `validationRequest`).
    pub async fn validation_request(
        &self,
        validator_address: Address,
        agent_id: U256,
        request_uri: String,
        request_hash: B256,
        snapshot_id: u64,
    ) -> Result<B256> {
        let call = self.contract.validationRequest(
            validator_address,
            agent_id,
            request_uri,
            request_hash,
            snapshot_id,
        );
        self.submitter.invoke("validationRequest", call).await
    }
}
