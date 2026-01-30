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
    #[derive(Debug)]
    struct MetadataEntry {
        string metadataKey;
        bytes metadataValue;
    }

    #[sol(rpc)]
    #[derive(Debug)]
    contract IdentityRegistryUpgradeable {
        function register() external returns (uint256);
        function register(string calldata agentURI) external returns (uint256);
        function register(string calldata agentURI, MetadataEntry[] calldata metadata) external returns (uint256);
        function ownerOf(uint256 agentId) external view returns (address);
        function tokenURI(uint256 agentId) external view returns (string memory);
        function getAgentWallet(uint256 agentId) external view returns (address);

        // ERC-721 Transfer event emitted on registration (mint)
        event Transfer(address indexed from, address indexed to, uint256 indexed tokenId);
    }
}

use IdentityRegistryUpgradeable::{IdentityRegistryUpgradeableInstance, Transfer};

pub type IdentityMetadataEntry = MetadataEntry;

/// Client for interacting with the IdentityRegistryUpgradeable contract.
#[derive(Clone)]
pub struct IdentityRegistryClient<P: Provider + Clone> {
    provider: P,
    contract: IdentityRegistryUpgradeableInstance<P>,
    submitter: TransactionSubmitter<crate::common::errors::StandardErrors::StandardErrorsErrors>,
}

impl<P: Provider + Clone> IdentityRegistryClient<P> {
    pub fn new(provider: P, address: Address, tx_lock: Arc<Mutex<()>>) -> Self {
        let contract = IdentityRegistryUpgradeableInstance::new(address, provider.clone());
        let submitter = TransactionSubmitter::new(tx_lock);
        Self {
            provider,
            contract,
            submitter,
        }
    }

    /// Get the contract address.
    pub fn address(&self) -> Address {
        *self.contract.address()
    }

    /// Register a new agent without a URI.
    pub async fn register(&self) -> Result<B256> {
        let call = self.contract.register_0();
        self.submitter.invoke("register", call).await
    }

    /// Register a new agent with a URI.
    pub async fn register_with_uri(&self, agent_uri: String) -> Result<B256> {
        let call = self.contract.register_1(agent_uri);
        self.submitter.invoke("register", call).await
    }

    /// Register a new agent with a URI and metadata.
    pub async fn register_with_metadata(
        &self,
        agent_uri: String,
        metadata: Vec<IdentityMetadataEntry>,
    ) -> Result<B256> {
        let call = self.contract.register_2(agent_uri, metadata);
        self.submitter.invoke("register", call).await
    }

    /// Register a new agent with a URI and return the agent ID.
    /// Parses the Transfer event from the receipt to get the minted token ID.
    pub async fn register_with_uri_and_get_id(&self, agent_uri: String) -> Result<(B256, U256)> {
        let tx_hash = self.register_with_uri(agent_uri).await?;
        let agent_id = self.get_agent_id_from_tx(tx_hash).await?;
        Ok((tx_hash, agent_id))
    }

    /// Get the agent ID from a registration transaction by parsing the Transfer event.
    async fn get_agent_id_from_tx(&self, tx_hash: B256) -> Result<U256> {
        let receipt = self
            .provider
            .get_transaction_receipt(tx_hash)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Transaction receipt not found"))?;

        // Parse Transfer events from the receipt logs
        for log in receipt.inner.logs() {
            if let Ok(transfer) = log.log_decode::<Transfer>() {
                // For minting, `from` is the zero address
                if transfer.inner.from == Address::ZERO {
                    return Ok(transfer.inner.tokenId);
                }
            }
        }

        Err(anyhow::anyhow!(
            "Transfer event not found in transaction receipt"
        ))
    }

    /// Rust stub for the Solidity `GetAgent` semantics: owner + URI + agent wallet.
    pub async fn get_agent(&self, agent_id: U256) -> Result<(Address, String, Address)> {
        let owner = self.contract.ownerOf(agent_id).call().await?;
        let agent_uri = self.contract.tokenURI(agent_id).call().await?;
        let agent_wallet = self.contract.getAgentWallet(agent_id).call().await?;
        Ok((owner, agent_uri, agent_wallet))
    }
}
