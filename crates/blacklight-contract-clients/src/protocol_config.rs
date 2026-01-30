use crate::common::tx_submitter::TransactionSubmitter;
use alloy::{
    primitives::{Address, B256},
    providers::Provider,
    sol,
};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

sol!(
    #[sol(rpc)]
    #[derive(Debug)]
    contract ProtocolConfig {
        error ZeroAddress();
        error InvalidBps(uint256 bps);
        error InvalidCommitteeCap(uint32 base, uint32 max);
        error InvalidMaxVoteBatchSize(uint256 maxBatch);
        error InvalidModuleAddress(address module);
        error ZeroQuorumBps();
        error ZeroVerificationBps();
        error ZeroResponseWindow();
        error ZeroJailDuration();
        error DurationTooLarge(uint256 duration);

        event NodeVersionUpdated(string oldVersion, string newVersion);

        function nodeVersion() external view returns (string memory);
        function setNodeVersion(string calldata newVersion) external;
        function rewardPolicy() external view override returns (address);
    }
);

use ProtocolConfig::ProtocolConfigInstance;

/// Client for interacting with the ProtocolConfig contract
#[derive(Clone)]
pub struct ProtocolConfigClient<P: Provider + Clone> {
    contract: ProtocolConfigInstance<P>,
    submitter: TransactionSubmitter<ProtocolConfig::ProtocolConfigErrors>,
}

impl<P: Provider + Clone> ProtocolConfigClient<P> {
    /// Create a new ProtocolConfigClient
    pub fn new(provider: P, contract_address: Address, tx_lock: Arc<Mutex<()>>) -> Self {
        let contract = ProtocolConfigInstance::new(contract_address, provider);
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

    /// Returns the current node version string
    pub async fn node_version(&self) -> Result<String> {
        Ok(self.contract.nodeVersion().call().await?)
    }

    /// Returns the rewards policy address.
    pub async fn rewards_policy_address(&self) -> Result<Address> {
        Ok(self.contract.rewardPolicy().call().await?)
    }

    // ------------------------------------------------------------------------
    // Admin Functions (owner only)
    // ------------------------------------------------------------------------

    /// Sets the node version (owner only)
    pub async fn set_node_version(&self, new_version: String) -> Result<B256> {
        let call = self.contract.setNodeVersion(new_version);
        self.submitter.invoke("setNodeVersion", call).await
    }
}
