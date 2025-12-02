use crate::config::consts::ERROR_STRING_SELECTOR;
use ethers::core::types::Address;

// Module declarations
pub mod nilav_client;
pub mod nilav_router;
pub mod staking_operators;
pub mod test_token;

// Re-export client types
pub use nilav_client::NilAVClient;
pub use nilav_router::{NilAVRouterClient, SignedWsProvider as NilAVRouterSignedWsProvider};
pub use staking_operators::{
    SignedWsProvider as StakingOperatorsSignedWsProvider, StakingOperatorsClient,
};
pub use test_token::{SignedWsProvider as TESTTokenSignedWsProvider, TESTTokenClient};

// Re-export contract event types for convenience
pub use nilav_router::{
    Assignment, HtxassignedFilter, HtxrespondedFilter, HtxsubmittedFilter, NilAVRouter,
    NodeDeregisteredFilter, NodeRegisteredFilter,
};
pub use staking_operators::{
    JailedFilter, OperatorDeactivatedFilter, OperatorRegisteredFilter, SlashedFilter,
    StakedToFilter, StakingOperators, UnstakeDelayUpdatedFilter, UnstakeRequestedFilter,
    UnstakedWithdrawnFilter,
};
pub use test_token::{ApprovalFilter, OwnershipTransferredFilter, TESTToken, TransferFilter};

// Backwards compatibility alias
pub type NilAVWsClient = NilAVRouterClient;
pub type SignedWsProvider = NilAVRouterSignedWsProvider;

/// Configuration for connecting to smart contracts
#[derive(Clone)]
pub struct ContractConfig {
    pub router_contract_address: Address,
    pub staking_contract_address: Address,
    pub token_contract_address: Address,
    pub rpc_url: String,
}

pub type PrivateKey = String;

impl ContractConfig {
    /// Create a new config for a deployed contract
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

    /// Create a config with anvil defaults for NilAVRouter
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

/// Decode a Solidity Error(string) revert message from hex data
/// Returns the decoded error message if it's a standard Error(string), otherwise None
pub fn decode_error_string(revert_data: &str) -> Option<String> {
    // Remove 0x prefix if present
    let data = revert_data.strip_prefix("0x").unwrap_or(revert_data);

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
            config.router_contract_address,
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
        let contract_address = "0x89c1312Cedb0B0F67e4913D2076bd4a860652B69"
            .parse::<Address>()
            .unwrap();

        let client = NilAVWsClient::new(rpc_url, contract_address, private_key.unwrap()).await?;
        Ok(client)
    }

    #[tokio::test]
    #[ignore] // Requires a running Ethereum node
    async fn test_node_count() -> Result<(), Box<dyn std::error::Error>> {
        let client = create_test_client().await?;
        let count = client.node_count().await?;
        println!("Node count: {}", count);
        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires a running Ethereum node
    async fn test_get_nodes() -> Result<(), Box<dyn std::error::Error>> {
        let client = create_test_client().await?;
        let nodes = client.get_nodes().await?;
        println!("Nodes: {:?}", nodes);
        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires a running Ethereum node
    async fn test_htx_submission() -> Result<(), Box<dyn std::error::Error>> {
        let client = create_test_client().await?;

        // Ensure at least one node is registered
        let node_count = client.node_count().await?;
        if node_count.is_zero() {
            println!("No nodes registered, skipping HTX submission test");
            return Ok(());
        }

        // Create test HTX data
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

        // Submit HTX
        let (tx_hash, htx_id) = client.submit_htx(&htx).await?;
        println!("HTX submitted, tx: {:?}, htx_id: {:?}", tx_hash, htx_id);

        Ok(())
    }
}
