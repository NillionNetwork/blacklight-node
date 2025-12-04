use crate::config::consts::ERROR_STRING_SELECTOR;
use ethers::core::types::Address;
pub use ethers::middleware::{NonceManagerMiddleware, SignerMiddleware};
use ethers::providers::{Provider, Ws};
use ethers::signers::LocalWallet;

pub type SignedWsProvider = NonceManagerMiddleware<SignerMiddleware<Provider<Ws>, LocalWallet>>;

// ============================================================================
// Module Declarations
// ============================================================================

pub mod nilav_client;
pub mod nilav_router;
pub mod staking_operators;
pub mod test_token;

// ============================================================================
// Client Type Re-exports
// ============================================================================

pub use nilav_client::NilAVClient;
pub use nilav_router::NilAVRouterClient;
pub use staking_operators::StakingOperatorsClient;
pub use test_token::TESTTokenClient;

// ============================================================================
// Contract Event Type Re-exports
// ============================================================================

// NilAVRouter events
pub use nilav_router::{
    Assignment, HtxassignedFilter, HtxrespondedFilter, HtxsubmittedFilter, NilAVRouter,
    NodeDeregisteredFilter, NodeRegisteredFilter,
};

// StakingOperators events
pub use staking_operators::{
    JailedFilter, OperatorDeactivatedFilter, OperatorRegisteredFilter, SlashedFilter,
    StakedToFilter, StakingOperators, UnstakeDelayUpdatedFilter, UnstakeRequestedFilter,
    UnstakedWithdrawnFilter,
};

// TESTToken events
pub use test_token::{ApprovalFilter, OwnershipTransferredFilter, TESTToken, TransferFilter};

// ============================================================================
// Type Aliases
// ============================================================================

/// Type alias for private key strings
pub type PrivateKey = String;

// ============================================================================
// Contract Configuration
// ============================================================================

/// Configuration for connecting to NilAV smart contracts
///
/// Contains addresses for all three contracts in the system:
/// - NilAVRouter: Main routing and HTX verification logic
/// - StakingOperators: Operator registration and staking
/// - TESTToken: Test token for staking (mainnet will use real token)
#[derive(Clone, Debug)]
pub struct ContractConfig {
    pub router_contract_address: Address,
    pub staking_contract_address: Address,
    pub token_contract_address: Address,
    pub rpc_url: String,
}

impl ContractConfig {
    /// Create a new configuration for deployed contracts
    ///
    /// # Arguments
    /// * `rpc_url` - Ethereum RPC endpoint (HTTP or WebSocket)
    /// * `router_contract_address` - Address of deployed NilAVRouter contract
    /// * `staking_contract_address` - Address of deployed StakingOperators contract
    /// * `token_contract_address` - Address of deployed TESTToken contract
    pub fn new(
        rpc_url: String,
        router_contract_address: Address,
        staking_contract_address: Address,
        token_contract_address: Address,
    ) -> Self {
        Self {
            router_contract_address,
            staking_contract_address,
            token_contract_address,
            rpc_url,
        }
    }

    /// Create a configuration with Anvil local testnet defaults
    ///
    /// Note: This uses placeholder addresses. Actual Anvil deployment
    /// addresses will differ based on deployment order.
    pub fn anvil_config() -> Self {
        Self {
            router_contract_address: "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0"
                .parse::<Address>()
                .expect("Invalid contract address"),
            staking_contract_address: "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0"
                .parse::<Address>()
                .expect("Invalid contract address"),
            token_contract_address: "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0"
                .parse::<Address>()
                .expect("Invalid contract address"),
            rpc_url: "http://127.0.0.1:8545".to_string(),
        }
    }
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Decode a Solidity `Error(string)` revert message from hex-encoded calldata
///
/// Solidity reverts with `revert("message")` are encoded as:
/// - 4-byte selector: `0x08c379a0` (keccak256("Error(string)"))
/// - ABI-encoded string parameter:
///   - 32 bytes: offset (always 0x20 for single string)
///   - 32 bytes: string length
///   - N bytes: UTF-8 string data (padded to 32-byte boundary)
///
/// # Arguments
/// * `revert_data` - Hex-encoded revert data (with or without "0x" prefix)
///
/// # Returns
/// * `Some(String)` - Decoded error message if valid Error(string) format
/// * `None` - If not a standard Error(string) revert or decoding fails
///
/// # Example
/// ```ignore
/// let error = "0x08c379a0..."; // "NilAV: unknown HTX"
/// let msg = decode_error_string(error);
/// assert_eq!(msg, Some("NilAV: unknown HTX".to_string()));
/// ```
pub fn decode_error_string(revert_data: &str) -> Option<String> {
    // Strip "0x" prefix if present
    let data = revert_data.strip_prefix("0x").unwrap_or(revert_data);

    // Check for Error(string) selector
    if !data.starts_with(ERROR_STRING_SELECTOR) {
        return None;
    }

    // Skip 4-byte selector (8 hex chars)
    let encoded = &data[8..];

    // Need at least 128 hex chars (64 bytes) for offset + length
    if encoded.len() < 128 {
        return None;
    }

    // Parse offset (first 32 bytes, should be 0x20 = 32)
    let offset_hex = &encoded[0..64];
    let offset = u64::from_str_radix(offset_hex, 16).ok()?;
    if offset != 32 {
        return None; // Invalid offset for single string parameter
    }

    // Parse string length (next 32 bytes)
    let length_hex = &encoded[64..128];
    let length = u64::from_str_radix(length_hex, 16).ok()? as usize;

    // Decode string data (remaining bytes after offset + length)
    let string_data_hex = &encoded[128..];
    let string_bytes = hex::decode(string_data_hex).ok()?;

    // Extract only the actual string (ignore padding)
    if length <= string_bytes.len() {
        String::from_utf8(string_bytes[..length].to_vec()).ok()
    } else {
        None
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Builder, BuilderMeasurement, NilCcMeasurement, NilCcOperator, WorkloadId};
    use std::env;

    // ------------------------------------------------------------------------
    // Unit Tests - Error Decoding
    // ------------------------------------------------------------------------

    #[test]
    fn test_decode_error_string() {
        // Test decoding a real contract error: "NilAV: unknown HTX"
        let error_data = "0x08c379a0000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000124e696c41563a20756e6b6e6f776e204854580000000000000000000000000000";
        let decoded = decode_error_string(error_data);
        assert_eq!(decoded, Some("NilAV: unknown HTX".to_string()));

        // Test without "0x" prefix
        let error_data_no_prefix = &error_data[2..];
        let decoded2 = decode_error_string(error_data_no_prefix);
        assert_eq!(decoded2, Some("NilAV: unknown HTX".to_string()));

        // Test with invalid function selector
        let invalid = "0x12345678000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000124e696c41563a20756e6b6e6f776e204854580000000000000000000000000000";
        assert_eq!(decode_error_string(invalid), None);

        // Test with empty error string
        let empty_error = "0x08c379a000000000000000000000000000000000000000000000000000000000000000200000000000000000000000000000000000000000000000000000000000000000";
        let decoded_empty = decode_error_string(empty_error);
        assert_eq!(decoded_empty, Some("".to_string()));
    }

    // ------------------------------------------------------------------------
    // Unit Tests - Configuration
    // ------------------------------------------------------------------------

    #[test]
    fn test_config_creation() {
        let router_address = "0x89c1312Cedb0B0F67e4913D2076bd4a860652B69"
            .parse::<Address>()
            .unwrap();
        let staking_address = "0xDc64a140Aa3E981100a9becA4E685f962f0cF6C9"
            .parse::<Address>()
            .unwrap();
        let token_address = "0x5FC8d32690cc91D4c39d9d3abcBD16989F875707"
            .parse::<Address>()
            .unwrap();

        let config = ContractConfig::new(
            "http://localhost:8545".to_string(),
            router_address,
            staking_address,
            token_address,
        );

        assert_eq!(config.router_contract_address, router_address);
        assert_eq!(config.staking_contract_address, staking_address);
        assert_eq!(config.token_contract_address, token_address);
        assert_eq!(config.rpc_url, "http://localhost:8545");
    }

    #[test]
    fn test_contract_address_parsing() {
        let addr_str = "0x89c1312Cedb0B0F67e4913D2076bd4a860652B69";
        let addr = addr_str.parse::<Address>();
        assert!(addr.is_ok(), "Contract address should parse correctly");
    }

    // ------------------------------------------------------------------------
    // Integration Tests - Client Operations
    // ------------------------------------------------------------------------
    // Note: These tests require a running Ethereum node (Anvil, Hardhat, etc.)
    // Set TEST_RPC_URL and TEST_PRIVATE_KEY environment variables to run them.

    /// Helper to create a test WebSocket client for integration tests
    ///
    /// Reads configuration from environment variables:
    /// - `TEST_RPC_URL`: Ethereum RPC endpoint (default: http://localhost:8545)
    /// - `TEST_PRIVATE_KEY`: Private key for signing (default: Anvil account #0)
    ///
    /// Uses hardcoded contract addresses that should match your test deployment.
    async fn create_test_client() -> Result<NilAVClient, Box<dyn std::error::Error>> {
        // Read configuration from environment (with defaults for local Anvil)
        let rpc_url =
            env::var("TEST_RPC_URL").unwrap_or_else(|_| "http://localhost:8545".to_string());
        let private_key = env::var("TEST_PRIVATE_KEY").unwrap_or_else(|_| {
            // Anvil account #0 private key (publicly known, DO NOT use in production)
            "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string()
        });

        // Test contract addresses (update these to match your deployment)
        let router_address = "0x89c1312Cedb0B0F67e4913D2076bd4a860652B69"
            .parse::<Address>()
            .unwrap();
        let staking_address = "0xDc64a140Aa3E981100a9becA4E685f962f0cF6C9"
            .parse::<Address>()
            .unwrap();
        let token_address = "0x5FC8d32690cc91D4c39d9d3abcBD16989F875707"
            .parse::<Address>()
            .unwrap();

        // Create client with configuration
        let config = ContractConfig::new(rpc_url, router_address, staking_address, token_address);
        let client = NilAVClient::new(config, private_key).await?;

        Ok(client)
    }

    #[tokio::test]
    #[ignore] // Requires a running Ethereum node
    async fn test_node_count() -> Result<(), Box<dyn std::error::Error>> {
        let client = create_test_client().await?;
        let count = client.router.node_count().await?;
        println!("Node count: {}", count);
        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires a running Ethereum node
    async fn test_get_nodes() -> Result<(), Box<dyn std::error::Error>> {
        let client = create_test_client().await?;
        let nodes = client.router.get_nodes().await?;
        println!("Nodes: {:?}", nodes);
        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires a running Ethereum node
    async fn test_htx_submission() -> Result<(), Box<dyn std::error::Error>> {
        let client = create_test_client().await?;

        // Skip test if no nodes are registered
        let node_count = client.router.node_count().await?;
        if node_count.is_zero() {
            println!("No nodes registered, skipping HTX submission test");
            return Ok(());
        }

        // Create minimal test HTX
        let htx = crate::types::Htx {
            workload_id: WorkloadId {
                current: 1,
                previous: 0,
            },
            nilcc_operator: NilCcOperator {
                id: 1,
                name: "test".into(),
            },
            builder: Builder {
                id: 1,
                name: "test".into(),
            },
            nilcc_measurement: NilCcMeasurement {
                url: "https://test.com".into(),
                nilcc_version: "0.0.0".into(),
                cpu_count: 1,
                gpus: 0,
            },
            builder_measurement: BuilderMeasurement {
                url: "https://test.com".into(),
            },
        };

        // Submit and verify
        let (tx_hash, htx_id) = client.router.submit_htx(&htx).await?;
        println!("HTX submitted successfully:");
        println!("  Transaction: {:?}", tx_hash);
        println!("  HTX ID: {:?}", htx_id);

        Ok(())
    }
}
