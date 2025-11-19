use crate::types::Htx;
use ethers::{
    contract::abigen,
    core::types::{Address, H256, U256},
    middleware::{NonceManagerMiddleware, SignerMiddleware},
    providers::{Middleware, Provider, StreamExt, Ws},
    signers::{LocalWallet, Signer},
};
use std::sync::Arc;

/// Decode a Solidity Error(string) revert message from hex data
/// Returns the decoded error message if it's a standard Error(string), otherwise None
pub fn decode_error_string(revert_data: &str) -> Option<String> {
    // Remove 0x prefix if present
    let data = revert_data.strip_prefix("0x").unwrap_or(revert_data);

    // Error(string) selector is 0x08c379a0
    const ERROR_STRING_SELECTOR: &str = "08c379a0";

    if !data.starts_with(ERROR_STRING_SELECTOR) {
        return None;
    }

    // Skip selector (8 hex chars = 4 bytes)
    let encoded = &data[8..];

    // ABI-encode format for string:
    // - offset (32 bytes = 64 hex chars) - should be 0x20
    // - length (32 bytes = 64 hex chars)
    // - string data (padded to 32-byte boundary)

    if encoded.len() < 128 {
        return None; // Need at least offset + length
    }

    // Parse offset (should be 0x20 = 32)
    let offset_hex = &encoded[0..64];
    if let Ok(offset) = u64::from_str_radix(offset_hex, 16) {
        if offset != 32 {
            return None;
        }
    } else {
        return None;
    }

    // Parse length
    let length_hex = &encoded[64..128];
    if let Ok(length) = u64::from_str_radix(length_hex, 16) {
        // Extract string data (skip offset and length, start at byte 64 = char 128)
        let string_data_hex = &encoded[128..];
        let string_bytes = hex::decode(string_data_hex).ok()?;

        // Take only the length bytes (ignore padding)
        if length as usize <= string_bytes.len() {
            if let Ok(decoded) = String::from_utf8(string_bytes[..length as usize].to_vec()) {
                return Some(decoded);
            }
        }
    }

    None
}

// Generate type-safe contract bindings from ABI
abigen!(
    NilAVRouter,
    "./out/NilAVRouter.sol/NilAVRouter.json",
    event_derives(serde::Deserialize, serde::Serialize)
);

pub type SignedWsProvider = NonceManagerMiddleware<SignerMiddleware<Provider<Ws>, LocalWallet>>;

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

/// WebSocket-based client for real-time event streaming and contract interaction
pub struct NilAVWsClient {
    contract: NilAVRouter<SignedWsProvider>,
    provider: Arc<SignedWsProvider>,
}

impl NilAVWsClient {
    /// Create a new WebSocket client from configuration
    pub async fn new(config: ContractConfig, private_key: PrivateKey) -> anyhow::Result<Self> {
        // Convert HTTP URL to WebSocket URL
        let ws_url = config
            .rpc_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");

        let provider = Provider::<Ws>::connect(&ws_url).await?;
        let chain_id = provider.get_chainid().await?;

        let wallet = private_key
            .parse::<LocalWallet>()
            .expect("Invalid private key")
            .with_chain_id(chain_id.as_u64());

        // Wrap with SignerMiddleware first, then NonceManagerMiddleware to handle concurrent txs
        let wallet_address = wallet.address();
        let signer_middleware = SignerMiddleware::new(provider, wallet);
        let provider = Arc::new(NonceManagerMiddleware::new(
            signer_middleware,
            wallet_address,
        ));
        let contract = NilAVRouter::new(config.contract_address, provider.clone());

        Ok(Self { contract, provider })
    }

    /// Create WebSocket client with anvil defaults
    pub async fn anvil_ws(private_key: PrivateKey) -> anyhow::Result<Self> {
        let config = ContractConfig {
            contract_address: "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0"
                .parse::<Address>()
                .expect("Invalid contract address"),
            rpc_url: "ws://127.0.0.1:8545".to_string(),
        };
        Self::new(config, private_key).await
    }
    /// Get the contract address
    pub fn address(&self) -> Address {
        self.contract.address()
    }

    /// Get the signer address
    pub fn signer_address(&self) -> Address {
        self.provider.inner().signer().address()
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
        let tx = call.send().await.map_err(|e| {
            // Try to decode error message from revert data
            let error_msg = e.to_string();

            // Try different patterns for finding revert data
            let revert_data = error_msg
                .split("reverted with data: ")
                .nth(1)
                .or_else(|| error_msg.split("revert data: ").nth(1))
                .or_else(|| {
                    // Look for hex data starting with 0x08c379a0 (Error(string) selector)
                    error_msg.find("0x08c379a0").and_then(|start| {
                        // Find the end of the hex string (stop at first non-hex char after 0x)
                        let remaining = &error_msg[start..];
                        let end = remaining
                            .char_indices()
                            .skip(2) // Skip "0x"
                            .find(|(_, c)| !c.is_ascii_hexdigit())
                            .map(|(i, _)| i)
                            .unwrap_or(remaining.len());
                        Some(&error_msg[start..start + end])
                    })
                });

            if let Some(data) = revert_data {
                if let Some(decoded) = decode_error_string(data.trim()) {
                    return anyhow::anyhow!("Contract call reverted: {}", decoded);
                }
            }

            e.into()
        })?;
        let receipt = tx.await?;
        let receipt = receipt.ok_or_else(|| anyhow::anyhow!("No transaction receipt"))?;
        Ok(receipt.transaction_hash)
    }

    /// Get assignment details for an HTX
    pub async fn get_assignment(&self, htx_id: H256) -> anyhow::Result<Assignment> {
        let htx_id_bytes: [u8; 32] = htx_id.into();
        Ok(self.contract.get_assignment(htx_id_bytes).call().await?)
    }

    /// Get HTX bytes for an HTX ID
    pub async fn get_htx(&self, htx_id: H256) -> anyhow::Result<Vec<u8>> {
        let htx_id_bytes: [u8; 32] = htx_id.into();
        let bytes = self.contract.get_htx(htx_id_bytes).call().await?;
        Ok(bytes.to_vec())
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

    /// Get the current block number
    pub async fn get_block_number(&self) -> anyhow::Result<u64> {
        let block_number = self.provider.get_block_number().await?;
        Ok(block_number.as_u64())
    }

    // ------------------------------------------------------------------------
    // Real-time Event Streaming
    // ------------------------------------------------------------------------

    /// Start listening for HTX assigned events and process them with a callback
    pub async fn listen_htx_assigned_events<F, Fut>(
        self: Arc<Self>,
        mut callback: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(HtxassignedFilter) -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        let event_stream = self.contract.event::<HtxassignedFilter>();
        let mut events = event_stream.stream().await?;

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok(event) => {
                    if let Err(e) = callback(event).await {
                        eprintln!("Error processing HTX assigned event: {}", e);
                    }
                }
                Err(e) => {
                    eprintln!("Error receiving HTX assigned event: {}", e);
                }
            }
        }
        Ok(())
    }

    /// Start listening for HTX assigned events for a specific node
    pub async fn listen_htx_assigned_for_node<F, Fut>(
        self: Arc<Self>,
        node_address: Address,
        mut callback: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(HtxassignedFilter) -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        let event_stream = self.contract.event::<HtxassignedFilter>();
        let mut events = event_stream.stream().await?;

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok(event) if event.node == node_address => {
                    if let Err(e) = callback(event).await {
                        eprintln!("Error processing HTX assigned event: {}", e);
                    }
                }
                Ok(_) => {
                    // Event for different node, ignore
                }
                Err(e) => {
                    eprintln!("Error receiving HTX assigned event: {}", e);
                }
            }
        }
        Ok(())
    }

    /// Start listening for HTX submitted events
    pub async fn listen_htx_submitted_events<F, Fut>(
        self: Arc<Self>,
        mut callback: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(HtxsubmittedFilter) -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        let event_stream = self.contract.event::<HtxsubmittedFilter>();
        let mut events = event_stream.stream().await?;

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok(event) => {
                    if let Err(e) = callback(event).await {
                        eprintln!("Error processing HTX submitted event: {}", e);
                    }
                }
                Err(e) => {
                    eprintln!("Error receiving HTX submitted event: {}", e);
                }
            }
        }
        Ok(())
    }

    /// Start listening for HTX responded events
    pub async fn listen_htx_responded_events<F, Fut>(
        self: Arc<Self>,
        mut callback: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(HtxrespondedFilter) -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        let event_stream = self.contract.event::<HtxrespondedFilter>();
        let mut events = event_stream.stream().await?;

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok(event) => {
                    if let Err(e) = callback(event).await {
                        eprintln!("Error processing HTX responded event: {}", e);
                    }
                }
                Err(e) => {
                    eprintln!("Error receiving HTX responded event: {}", e);
                }
            }
        }
        Ok(())
    }

    /// Start listening for node registration events
    pub async fn listen_node_registered_events<F, Fut>(
        self: Arc<Self>,
        mut callback: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(NodeRegisteredFilter) -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        let event_stream = self.contract.event::<NodeRegisteredFilter>();
        let mut events = event_stream.stream().await?;

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok(event) => {
                    if let Err(e) = callback(event).await {
                        eprintln!("Error processing node registered event: {}", e);
                    }
                }
                Err(e) => {
                    eprintln!("Error receiving node registered event: {}", e);
                }
            }
        }
        Ok(())
    }

    /// Start listening for node deregistration events
    pub async fn listen_node_deregistered_events<F, Fut>(
        self: Arc<Self>,
        mut callback: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(NodeDeregisteredFilter) -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        let event_stream = self.contract.event::<NodeDeregisteredFilter>();
        let mut events = event_stream.stream().await?;

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok(event) => {
                    if let Err(e) = callback(event).await {
                        eprintln!("Error processing node deregistered event: {}", e);
                    }
                }
                Err(e) => {
                    eprintln!("Error receiving node deregistered event: {}", e);
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------------
    // Historical Event Query Methods
    // ------------------------------------------------------------------------

    /// Get all HTX submitted events from contract history
    pub async fn get_htx_submitted_events(&self) -> anyhow::Result<Vec<HtxsubmittedFilter>> {
        let events = self
            .contract
            .event::<HtxsubmittedFilter>()
            .from_block(0)
            .query()
            .await?;
        Ok(events)
    }

    /// Get all HTX assigned events from contract history
    pub async fn get_htx_assigned_events(&self) -> anyhow::Result<Vec<HtxassignedFilter>> {
        let events = self
            .contract
            .event::<HtxassignedFilter>()
            .from_block(0)
            .query()
            .await?;
        Ok(events)
    }

    /// Get all HTX responded events from contract history
    pub async fn get_htx_responded_events(&self) -> anyhow::Result<Vec<HtxrespondedFilter>> {
        let events = self
            .contract
            .event::<HtxrespondedFilter>()
            .from_block(0)
            .query()
            .await?;
        Ok(events)
    }

    /// Get all node registered events from contract history
    pub async fn get_node_registered_events(&self) -> anyhow::Result<Vec<NodeRegisteredFilter>> {
        let events = self
            .contract
            .event::<NodeRegisteredFilter>()
            .from_block(0)
            .query()
            .await?;
        Ok(events)
    }

    /// Get all node deregistered events from contract history
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
    use super::*;
    use crate::types::{Builder, BuilderMeasurement, NilCcMeasurement, NilCcOperator, WorkloadId};
    use std::env;

    #[test]
    fn test_decode_error_string() {
        // Test with the actual error from the user
        let error_data = "0x08c379a0000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000124e696c41563a20756e6b6e6f776e204854580000000000000000000000000000";
        let decoded = decode_error_string(error_data);
        assert_eq!(decoded, Some("NilAV: unknown HTX".to_string()));

        // Test without 0x prefix
        let error_data_no_prefix = &error_data[2..];
        let decoded2 = decode_error_string(error_data_no_prefix);
        assert_eq!(decoded2, Some("NilAV: unknown HTX".to_string()));

        // Test with invalid selector
        let invalid = "0x12345678000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000124e696c41563a20756e6b6e6f776e204854580000000000000000000000000000";
        assert_eq!(decode_error_string(invalid), None);

        // Test with empty string
        let empty_error = "0x08c379a000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000000";
        let decoded_empty = decode_error_string(empty_error);
        assert_eq!(decoded_empty, Some("".to_string()));
    }

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

    // Helper function to create a test WebSocket client
    // Note: These tests require a local Ethereum node (e.g., Hardhat, Ganache, or Anvil)
    async fn create_test_client() -> Result<NilAVWsClient, Box<dyn std::error::Error>> {
        let rpc_url =
            env::var("TEST_RPC_URL").unwrap_or_else(|_| "http://localhost:8545".to_string());
        let private_key = env::var("TEST_PRIVATE_KEY").ok();

        let config = ContractConfig::new(
            rpc_url,
            "0x89c1312Cedb0B0F67e4913D2076bd4a860652B69"
                .parse::<Address>()
                .unwrap(),
        );
        let client = NilAVWsClient::new(config, private_key.unwrap()).await?;
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

    // Note: Event streaming tests would require a more complex setup with actual WebSocket connections
    // and are better tested in integration tests

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
