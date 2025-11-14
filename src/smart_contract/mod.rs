use crate::types::Htx;
use ethers::{
    contract::abigen,
    core::types::{Address, H256, U256},
    middleware::SignerMiddleware,
    providers::{Http, Middleware, Provider},
    signers::{LocalWallet, Signer},
};
use std::sync::Arc;

// Generate type-safe contract bindings from ABI
abigen!(
    NilAVRouter,
    "./out/NilAVRouter.sol/NilAVRouter.json",
    event_derives(serde::Deserialize, serde::Serialize)
);

pub type SignedProvider = SignerMiddleware<Provider<Http>, LocalWallet>;

/// Configuration for connecting to the NilAVRouter contract
pub struct ContractConfig {
    pub contract_address: Address,
    pub rpc_url: String,
}

pub type PrivateKey = String;

impl ContractConfig {
    /// Create a new config for the deployed contract
    pub fn new(rpc_url: String, contract_address: Address) -> Self {
        Self {
            contract_address,
            rpc_url,
        }
    }

    pub fn anvil_config() -> Self {
        Self {
            contract_address: "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0"
                .parse::<Address>()
                .expect("Invalid contract address"),
            rpc_url: "http://127.0.0.1:8545".to_string(),
        }
    }
}

/// Client for interacting with the NilAVRouter contract
pub struct NilAVClient {
    // The contract instance to interact with
    // It is used to call contract functions and get the contract address
    contract: NilAVRouter<SignedProvider>,
    // The provider to interact with the blockchain
    // It is wrapped in an Arc to allow for concurrent access
    // It is used to sign transactions and get the signer address
    provider: Arc<SignedProvider>,
}

impl NilAVClient {
    /// Create a new client from configuration
    pub async fn new(config: ContractConfig, private_key: PrivateKey) -> anyhow::Result<Self> {
        let provider = Provider::<Http>::try_from(&config.rpc_url)?;
        let chain_id = provider.get_chainid().await?;

        let wallet = private_key
            .parse::<LocalWallet>()
            .expect("Invalid private key")
            .with_chain_id(chain_id.as_u64());
        let provider = Arc::new(SignerMiddleware::new(provider, wallet));
        let contract = NilAVRouter::new(config.contract_address, provider.clone());
        Ok(Self { contract, provider })
    }
    /// Get the contract address
    pub fn address(&self) -> Address {
        self.contract.address()
    }

    /// Get the signer address
    pub fn signer_address(&self) -> Address {
        self.provider.signer().address()
    }

    // ------------------------------------------------------------------------
    // Node Management Functions
    // ------------------------------------------------------------------------

    /// Register a new nilAV node
    pub async fn register_node(&self, node: Address) -> anyhow::Result<H256> {
        let call = self.contract.register_node(node);
        let tx = call.send().await?;
        let receipt = tx.await?;
        let receipt = receipt.ok_or_else(|| anyhow::anyhow!("No transaction receipt"))?;
        Ok(receipt.transaction_hash)
    }

    /// Deregister a nilAV node
    pub async fn deregister_node(&self, node: Address) -> anyhow::Result<H256> {
        let call = self.contract.deregister_node(node);
        let tx = call.send().await?;
        let receipt = tx.await?;
        let receipt = receipt.ok_or_else(|| anyhow::anyhow!("No transaction receipt"))?;
        Ok(receipt.transaction_hash)
    }

    /// Get the total number of registered nodes
    pub async fn node_count(&self) -> anyhow::Result<U256> {
        Ok(self.contract.node_count().call().await?)
    }

    /// Get all registered nodes
    pub async fn get_nodes(&self) -> anyhow::Result<Vec<Address>> {
        Ok(self.contract.get_nodes().call().await?)
    }

    /// Check if an address is a registered node
    pub async fn is_node(&self, address: Address) -> anyhow::Result<bool> {
        Ok(self.contract.is_node(address).call().await?)
    }

    /// Get node at specific index
    pub async fn get_node_at_index(&self, index: U256) -> anyhow::Result<Address> {
        Ok(self.contract.nodes(index).call().await?)
    }

    // ------------------------------------------------------------------------
    // HTX Submission and Verification
    // ------------------------------------------------------------------------

    /// Submit an HTX for verification
    pub async fn submit_htx(&self, htx: &Htx) -> anyhow::Result<(H256, H256)> {
        let call = self.contract.submit_htx(htx.try_into()?);
        let tx = call.send().await?;
        let receipt = tx.await?;
        let receipt = receipt.ok_or_else(|| anyhow::anyhow!("No transaction receipt"))?;

        // Extract htxId from logs
        let htx_id = if let Some(log) = receipt.logs.first() {
            log.topics
                .get(1)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("No htxId in logs"))?
        } else {
            return Err(anyhow::anyhow!("No logs in receipt"));
        };

        Ok((receipt.transaction_hash, htx_id))
    }

    /// Respond to an HTX assignment (called by assigned node)
    pub async fn respond_htx(&self, htx_id: H256, result: bool) -> anyhow::Result<H256> {
        let htx_id_bytes: [u8; 32] = htx_id.into();
        let call = self.contract.respond_htx(htx_id_bytes, result);
        let tx = call.send().await?;
        let receipt = tx.await?;
        let receipt = receipt.ok_or_else(|| anyhow::anyhow!("No transaction receipt"))?;
        Ok(receipt.transaction_hash)
    }

    /// Get assignment details for an HTX
    pub async fn get_assignment(&self, htx_id: H256) -> anyhow::Result<Assignment> {
        let htx_id_bytes: [u8; 32] = htx_id.into();
        Ok(self.contract.get_assignment(htx_id_bytes).call().await?)
    }

    /// Get assignment details using the assignments mapping
    pub async fn get_assignment_direct(
        &self,
        htx_id: H256,
    ) -> anyhow::Result<(Address, bool, bool)> {
        let htx_id_bytes: [u8; 32] = htx_id.into();
        Ok(self.contract.assignments(htx_id_bytes).call().await?)
    }

    // ------------------------------------------------------------------------
    // Event Monitoring
    // ------------------------------------------------------------------------

    /// Get all HTXSubmitted events
    pub async fn get_htx_submitted_events(&self) -> anyhow::Result<Vec<HtxsubmittedFilter>> {
        let events = self
            .contract
            .event::<HtxsubmittedFilter>()
            .from_block(0)
            .query()
            .await?;
        Ok(events)
    }

    /// Get all HTXAssigned events
    pub async fn get_htx_assigned_events(&self) -> anyhow::Result<Vec<HtxassignedFilter>> {
        let events = self
            .contract
            .event::<HtxassignedFilter>()
            .from_block(0)
            .query()
            .await?;
        Ok(events)
    }

    /// Get all HTXResponded events
    pub async fn get_htx_responded_events(&self) -> anyhow::Result<Vec<HtxrespondedFilter>> {
        let events = self
            .contract
            .event::<HtxrespondedFilter>()
            .from_block(0)
            .query()
            .await?;
        Ok(events)
    }

    /// Get all NodeRegistered events
    pub async fn get_node_registered_events(&self) -> anyhow::Result<Vec<NodeRegisteredFilter>> {
        let events = self
            .contract
            .event::<NodeRegisteredFilter>()
            .from_block(0)
            .query()
            .await?;
        Ok(events)
    }

    /// Get all NodeDeregistered events
    pub async fn get_node_deregistered_events(
        &self,
    ) -> anyhow::Result<Vec<NodeDeregisteredFilter>> {
        let events = self
            .contract
            .event::<NodeDeregisteredFilter>()
            .from_block(0)
            .query()
            .await?;
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use crate::types::{
        Builder, BuilderMeasurement, Htx, NilCcMeasurement, NilCcOperator, WorkloadId,
    };

    use super::*;
    use std::env;

    #[test]
    fn test_config_creation() {
        let config = ContractConfig::new(
            "http://localhost:8545".to_string(),
            "0x89c1312Cedb0B0F67e4913D2076bd4a860652B69"
                .to_string()
                .parse::<Address>()
                .unwrap(),
        );
        assert_eq!(
            config.contract_address,
            "0x89c1312Cedb0B0F67e4913D2076bd4a860652B69"
                .parse::<Address>()
                .unwrap()
        );
    }

    #[test]
    fn test_contract_address_parsing() {
        let addr_str = "0x89c1312Cedb0B0F67e4913D2076bd4a860652B69";
        let addr = addr_str.parse::<Address>();
        assert!(addr.is_ok(), "Contract address should parse correctly");
    }

    // Helper function to create a test client
    // Note: These tests require a local Ethereum node (e.g., Hardhat, Ganache, or Anvil)
    async fn create_test_client() -> Result<NilAVClient, Box<dyn std::error::Error>> {
        let rpc_url =
            env::var("TEST_RPC_URL").unwrap_or_else(|_| "http://localhost:8545".to_string());
        let private_key = env::var("TEST_PRIVATE_KEY").ok();

        let config = ContractConfig::new(
            rpc_url,
            "0x89c1312Cedb0B0F67e4913D2076bd4a860652B69"
                .parse::<Address>()
                .unwrap(),
        );
        let client = NilAVClient::new(config, private_key.unwrap()).await?;
        Ok(client)
    }

    #[tokio::test]
    #[ignore] // Requires a running Ethereum node
    async fn test_node_registration() -> Result<(), Box<dyn std::error::Error>> {
        let client = create_test_client().await?;

        // Generate a test address
        let test_node: Address = "0x742d35Cc6634C0532925a3b844Bc454e4438f44e".parse()?;

        // Check initial state
        let initial_count = client.node_count().await?;
        let is_registered = client.is_node(test_node).await?;

        println!("Initial node count: {}", initial_count);
        println!("Test node initially registered: {}", is_registered);

        // Register the node if not already registered
        if !is_registered {
            let tx_hash = client.register_node(test_node).await?;
            println!("Node registered, tx: {:?}", tx_hash);

            // Verify registration
            let is_now_registered = client.is_node(test_node).await?;
            assert!(is_now_registered, "Node should be registered");

            let new_count = client.node_count().await?;
            assert_eq!(
                new_count,
                initial_count + 1,
                "Node count should increase by 1"
            );
        }

        // Get all nodes
        let nodes = client.get_nodes().await?;
        assert!(
            nodes.contains(&test_node),
            "Registered nodes should contain test node"
        );

        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires a running Ethereum node
    async fn test_node_deregistration() -> Result<(), Box<dyn std::error::Error>> {
        let client = create_test_client().await?;
        let test_node: Address = "0x742d35Cc6634C0532925a3b844Bc454e4438f44e".parse()?;

        // Ensure node is registered first
        let is_registered = client.is_node(test_node).await?;
        if !is_registered {
            client.register_node(test_node).await?;
        }

        let count_before = client.node_count().await?;

        // Deregister the node
        let tx_hash = client.deregister_node(test_node).await?;
        println!("Node deregistered, tx: {:?}", tx_hash);

        // Verify deregistration
        let is_still_registered = client.is_node(test_node).await?;
        assert!(!is_still_registered, "Node should be deregistered");

        let count_after = client.node_count().await?;
        assert_eq!(
            count_after,
            count_before - 1,
            "Node count should decrease by 1"
        );

        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires a running Ethereum node
    async fn test_htx_submission() -> Result<(), Box<dyn std::error::Error>> {
        let client = create_test_client().await?;

        // Ensure at least one node is registered
        let node_count = client.node_count().await?;
        if node_count.is_zero() {
            let test_node: Address = "0x742d35Cc6634C0532925a3b844Bc454e4438f44e".parse()?;
            client.register_node(test_node).await?;
            println!("Registered test node for HTX submission test");
        }

        // Create test HTX data
        let htx = Htx {
            workload_id: WorkloadId {
                current: 1,
                previous: 0,
            },
            nil_cc_operator: NilCcOperator {
                id: 1,
                name: "test".into(),
            },
            builder: Builder {
                id: 1,
                name: "test".into(),
            },
            nil_cc_measurement: NilCcMeasurement {
                url: "https://test.com".into(),
                nilcc_version: "0.0.0".into(),
                cpu_count: 1,
                gpus: 0,
            },
            builder_measurement: BuilderMeasurement {
                url: "https://test.com".into(),
            },
        };

        // Submit HTX
        let (tx_hash, htx_id) = client.submit_htx(&htx).await?;
        println!("HTX submitted, tx: {:?}, htx_id: {:?}", tx_hash, htx_id);

        // Verify assignment was created
        let assignment = client.get_assignment(htx_id).await?;
        println!(
            "Assignment: node={}, responded={}, result={}",
            assignment.node, assignment.responded, assignment.result
        );

        assert_ne!(
            assignment.node,
            Address::zero(),
            "Should have assigned node"
        );
        assert!(!assignment.responded, "Should not have responded yet");

        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires a running Ethereum node
    async fn test_event_queries() -> Result<(), Box<dyn std::error::Error>> {
        let client = create_test_client().await?;

        // Query all event types
        let submitted_events = client.get_htx_submitted_events().await?;
        println!("HTX Submitted events: {}", submitted_events.len());

        let assigned_events = client.get_htx_assigned_events().await?;
        println!("HTX Assigned events: {}", assigned_events.len());

        let responded_events = client.get_htx_responded_events().await?;
        println!("HTX Responded events: {}", responded_events.len());

        let registered_events = client.get_node_registered_events().await?;
        println!("Node Registered events: {}", registered_events.len());

        let deregistered_events = client.get_node_deregistered_events().await?;
        println!("Node Deregistered events: {}", deregistered_events.len());

        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires a running Ethereum node
    async fn test_multiple_node_registration() -> Result<(), Box<dyn std::error::Error>> {
        let client = create_test_client().await?;

        let test_nodes = vec![
            "0x742d35Cc6634C0532925a3b844Bc454e4438f44e".parse::<Address>()?,
            "0x5A4863E2441b4Fc7F4b1b6b0F3b9d6cB5a0f3e2d".parse::<Address>()?,
            "0x1234567890123456789012345678901234567890".parse::<Address>()?,
        ];

        let initial_count = client.node_count().await?;

        // Register multiple nodes
        for node in &test_nodes {
            let is_registered = client.is_node(*node).await?;
            if !is_registered {
                client.register_node(*node).await?;
                println!("Registered node: {}", node);
            }
        }

        // Verify all nodes are registered
        let nodes = client.get_nodes().await?;
        for node in &test_nodes {
            assert!(
                nodes.contains(node),
                "Should contain registered node {}",
                node
            );
        }

        let final_count = client.node_count().await?;
        assert!(
            final_count >= initial_count,
            "Node count should not decrease"
        );

        Ok(())
    }
}
