use alloy::primitives::Address;

pub mod common;
pub mod heartbeat_manager;
pub mod nil_token;
pub mod blacklight_client;
pub mod staking_operators;

// ============================================================================
// Client Type Re-exports
// ============================================================================

pub use heartbeat_manager::HeartbeatManagerClient;
pub use nil_token::NilTokenClient;
pub use blacklight_client::blacklightClient;
pub use staking_operators::StakingOperatorsClient;

// ============================================================================
// Contract Event Type Re-exports
// ============================================================================

// Heartbeat manager events
pub use heartbeat_manager::HearbeatManager;

// StakingOperators events
pub use staking_operators::StakingOperators;

// NilToken events
pub use nil_token::NilToken;

// ============================================================================
// Type Aliases
// ============================================================================

/// Type alias for private key strings
pub type PrivateKey = String;

// ============================================================================
// Contract Configuration
// ============================================================================

/// Configuration for connecting to blacklight smart contracts
///
/// Contains addresses for all three contracts in the system:
/// - HeartbeatManager: Main routing and HTX verification logic
/// - StakingOperators: Operator registration and staking
/// - NilToken: Test token for staking (mainnet will use real token)
///
/// Also includes connection settings for WebSocket reliability.
#[derive(Clone, Debug)]
pub struct ContractConfig {
    pub manager_contract_address: Address,
    pub staking_contract_address: Address,
    pub token_contract_address: Address,
    pub rpc_url: String,
    /// Maximum number of WebSocket reconnection attempts (default: u32::MAX for infinite)
    pub max_ws_retries: u32,
}

impl Default for ContractConfig {
    fn default() -> Self {
        Self {
            manager_contract_address: Address::ZERO,
            staking_contract_address: Address::ZERO,
            token_contract_address: Address::ZERO,
            rpc_url: String::new(),
            max_ws_retries: u32::MAX,
        }
    }
}

impl ContractConfig {
    /// Create a new configuration for deployed contracts
    ///
    /// # Arguments
    /// * `rpc_url` - Ethereum RPC endpoint (HTTP or WebSocket)
    /// * `manager_contract_address` - Address of deployed HeartbeatManager contract
    /// * `staking_contract_address` - Address of deployed StakingOperators contract
    /// * `token_contract_address` - Address of deployed NilToken contract
    pub fn new(
        rpc_url: String,
        manager_contract_address: Address,
        staking_contract_address: Address,
        token_contract_address: Address,
    ) -> Self {
        Self {
            manager_contract_address,
            staking_contract_address,
            token_contract_address,
            rpc_url,
            max_ws_retries: u32::MAX,
        }
    }

    /// Set the maximum number of WebSocket reconnection attempts
    pub fn with_max_ws_retries(mut self, max_retries: u32) -> Self {
        self.max_ws_retries = max_retries;
        self
    }

    /// Create a configuration with Anvil local testnet defaults
    ///
    /// Uses deterministic Anvil deployment addresses based on standard nonce order:
    /// - Token deployed first (nonce 0)
    /// - Staking deployed second (nonce 1)
    /// - Heartbeat manager deployed third (nonce 2)
    pub fn anvil_config() -> Self {
        Self {
            // Anvil deterministic addresses for deployer 0xf39F...2266 (account #0)
            // These assume deployment order: Token -> Staking -> Manager
            token_contract_address: "0x5FbDB2315678afecb367f032d93F642f64180aa3"
                .parse::<Address>()
                .expect("Invalid token address"),
            staking_contract_address: "0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512"
                .parse::<Address>()
                .expect("Invalid staking address"),
            manager_contract_address: "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0"
                .parse::<Address>()
                .expect("Invalid manager address"),
            rpc_url: "http://127.0.0.1:8545".to_string(),
            max_ws_retries: u32::MAX,
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        Builder, BuilderMeasurement, NilCcOperator, NillionHtx, NillionHtxV1, WorkloadId,
        WorkloadMeasurement,
    };
    use std::env;

    // ------------------------------------------------------------------------
    // Unit Tests - Configuration
    // ------------------------------------------------------------------------

    #[test]
    fn test_config_creation() {
        let manager_address = "0x89c1312Cedb0B0F67e4913D2076bd4a860652B69"
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
            manager_address,
            staking_address,
            token_address,
        );

        assert_eq!(config.manager_contract_address, manager_address);
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
    async fn create_test_client() -> Result<blacklightClient, Box<dyn std::error::Error>> {
        // Read configuration from environment (with defaults for local Anvil)
        let rpc_url =
            env::var("TEST_RPC_URL").unwrap_or_else(|_| "http://localhost:8545".to_string());
        let private_key = env::var("TEST_PRIVATE_KEY").unwrap_or_else(|_| {
            // Anvil account #0 private key (publicly known, DO NOT use in production)
            "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string()
        });

        // Test contract addresses (update these to match your deployment)
        let manager_address = "0x89c1312Cedb0B0F67e4913D2076bd4a860652B69"
            .parse::<Address>()
            .unwrap();
        let staking_address = "0xDc64a140Aa3E981100a9becA4E685f962f0cF6C9"
            .parse::<Address>()
            .unwrap();
        let token_address = "0x5FC8d32690cc91D4c39d9d3abcBD16989F875707"
            .parse::<Address>()
            .unwrap();

        // Create client with configuration
        let config = ContractConfig::new(rpc_url, manager_address, staking_address, token_address);
        let client = blacklightClient::new(config, private_key).await?;

        Ok(client)
    }

    #[tokio::test]
    #[ignore] // Requires a running Ethereum node
    async fn test_node_count() -> Result<(), Box<dyn std::error::Error>> {
        let client = create_test_client().await?;
        let count = client.manager.node_count().await?;
        println!("Node count: {}", count);
        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires a running Ethereum node
    async fn test_get_nodes() -> Result<(), Box<dyn std::error::Error>> {
        let client = create_test_client().await?;
        let nodes = client.manager.get_nodes().await?;
        println!("Nodes: {:?}", nodes);
        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires a running Ethereum node
    async fn test_htx_submission() -> Result<(), Box<dyn std::error::Error>> {
        let client = create_test_client().await?;

        // Skip test if no nodes are registered
        let node_count = client.manager.node_count().await?;
        if node_count.is_zero() {
            println!("No nodes registered, skipping HTX submission test");
            return Ok(());
        }

        // Create minimal test HTX
        let htx = NillionHtx::V1(NillionHtxV1 {
            workload_id: WorkloadId {
                current: "1".into(),
                previous: Some("0".into()),
            },
            operator: Some(NilCcOperator {
                id: 1,
                name: "test".into(),
            }),
            builder: Some(Builder {
                id: 1,
                name: "test".into(),
            }),
            workload_measurement: WorkloadMeasurement {
                url: "https://test.com".into(),
                artifacts_version: "0.0.0".into(),
                cpus: 1,
                gpus: 0,
                docker_compose_hash: [0; 32],
            },
            builder_measurement: BuilderMeasurement {
                url: "https://test.com".into(),
            },
        })
        .into();

        // Submit and verify
        let tx_hash = client.manager.submit_htx(&htx).await?;
        println!("HTX submitted successfully:");
        println!("  Transaction: {:?}", tx_hash);

        Ok(())
    }
}
