use alloy::primitives::Address;

// ============================================================================
// Module Declarations
// ============================================================================
pub mod errors;
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
pub use nilav_router::{Assignment, NilAVRouter};

// Re-export NilAVRouter event types with Filter suffix for backwards compatibility
pub use nilav_router::NilAVRouter::HTXAssigned as HtxassignedFilter;
pub use nilav_router::NilAVRouter::HTXResponded as HtxrespondedFilter;
pub use nilav_router::NilAVRouter::HTXSubmitted as HtxsubmittedFilter;

// StakingOperators events
pub use staking_operators::StakingOperators;

// Re-export StakingOperators event types with Filter suffix for backwards compatibility
pub use staking_operators::StakingOperators::Jailed as JailedFilter;
pub use staking_operators::StakingOperators::OperatorDeactivated as OperatorDeactivatedFilter;
pub use staking_operators::StakingOperators::OperatorRegistered as OperatorRegisteredFilter;
pub use staking_operators::StakingOperators::Slashed as SlashedFilter;
pub use staking_operators::StakingOperators::StakedTo as StakedToFilter;
pub use staking_operators::StakingOperators::UnstakeDelayUpdated as UnstakeDelayUpdatedFilter;
pub use staking_operators::StakingOperators::UnstakeRequested as UnstakeRequestedFilter;
pub use staking_operators::StakingOperators::UnstakedWithdrawn as UnstakedWithdrawnFilter;

// TESTToken events
pub use test_token::TESTToken;

// Re-export TESTToken event types with Filter suffix for backwards compatibility
pub use test_token::TESTToken::Approval as ApprovalFilter;
pub use test_token::TESTToken::OwnershipTransferred as OwnershipTransferredFilter;
pub use test_token::TESTToken::Transfer as TransferFilter;

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
// Error Handling Re-exports
// ============================================================================

pub use errors::DecodedRevert;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Builder, BuilderMeasurement, NilCcMeasurement, NilCcOperator, WorkloadId};
    use std::env;


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
                current: "1".into(),
                previous: Some("0".into()),
            },
            nilcc_operator: Some(NilCcOperator {
                id: 1,
                name: "test".into(),
            }),
            builder: Some(Builder {
                id: 1,
                name: "test".into(),
            }),
            nilcc_measurement: NilCcMeasurement {
                url: "https://test.com".into(),
                nilcc_version: "0.0.0".into(),
                cpu_count: 1,
                gpus: 0,
                docker_compose_hash: [0; 32],
            },
            builder_measurement: BuilderMeasurement {
                url: "https://test.com".into(),
            },
        };

        // Submit and verify
        let tx_hash = client.router.submit_htx(&htx).await?;
        println!("HTX submitted successfully:");
        println!("  Transaction: {:?}", tx_hash);

        Ok(())
    }
}
