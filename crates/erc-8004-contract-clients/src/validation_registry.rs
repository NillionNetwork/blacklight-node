use alloy::{
    primitives::{Address, B256, U256},
    providers::Provider,
    sol,
};
use anyhow::Result;
use contract_clients_common::tx_submitter::TransactionSubmitter;
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

        function validationResponse(
            bytes32 requestHash,
            uint8 response,
            string calldata responseURI,
            bytes32 responseHash,
            string calldata tag
        ) external;

        event ValidationRequest(
            address indexed validatorAddress,
            uint256 indexed agentId,
            string requestURI,
            bytes32 indexed requestHash
        );

        event ValidationResponse(
            address indexed validatorAddress,
            uint256 indexed agentId,
            bytes32 indexed requestHash,
            uint8 response,
            string responseURI,
            bytes32 responseHash,
            string tag
        );
    }
}

use ValidationRegistryUpgradeable::ValidationRegistryUpgradeableInstance;

// Event type re-exports
pub type ValidationRequestEvent = ValidationRegistryUpgradeable::ValidationRequest;
pub type ValidationResponseEvent = ValidationRegistryUpgradeable::ValidationResponse;

/// Client for interacting with the ValidationRegistryUpgradeable contract.
#[derive(Clone)]
pub struct ValidationRegistryClient<P: Provider + Clone> {
    contract: ValidationRegistryUpgradeableInstance<P>,
    submitter: TransactionSubmitter,
}

impl<P: Provider + Clone> ValidationRegistryClient<P> {
    pub fn new(provider: P, address: Address, tx_lock: Arc<Mutex<()>>) -> Self {
        let contract = ValidationRegistryUpgradeableInstance::new(address, provider);
        let submitter = TransactionSubmitter::new(tx_lock, |_| None);
        Self {
            contract,
            submitter,
        }
    }

    /// Get the contract address.
    pub fn address(&self) -> Address {
        *self.contract.address()
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

    /// Submit a validation response.
    ///
    /// # Arguments
    /// * `request_hash` - The request hash identifying the validation request
    /// * `response` - Response value 0-100 (0=invalid, 100=valid)
    /// * `response_uri` - Optional URI pointing to response details
    /// * `response_hash` - Hash of the response data (can be zero)
    /// * `tag` - Tag identifying the response source (e.g., "heartbeat")
    pub async fn validation_response(
        &self,
        request_hash: B256,
        response: u8,
        response_uri: String,
        response_hash: B256,
        tag: String,
    ) -> Result<B256> {
        let call = self.contract.validationResponse(
            request_hash,
            response,
            response_uri,
            response_hash,
            tag,
        );
        self.submitter.invoke("validationResponse", call).await
    }
}
