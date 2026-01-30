use alloy::primitives::Address;

pub mod common;
pub mod erc_8004_client;
pub mod identity_registry;
pub mod validation_registry;

// ============================================================================
// Client Type Re-exports
// ============================================================================

pub use erc_8004_client::Erc8004Client;
pub use identity_registry::IdentityRegistryClient;
pub use validation_registry::ValidationRegistryClient;

// ============================================================================
// Type Aliases
// ============================================================================

/// Type alias for private key strings
pub type PrivateKey = String;

// ============================================================================
// Contract Configuration
// ============================================================================

/// Configuration for connecting to ERC-8004 smart contracts
///
/// Contains addresses for the ERC-8004 registry contracts.
#[derive(Clone, Debug)]
pub struct ContractConfig {
    pub identity_registry_contract_address: Address,
    pub validation_registry_contract_address: Address,
    pub rpc_url: String,
}

impl Default for ContractConfig {
    fn default() -> Self {
        Self {
            identity_registry_contract_address: Address::ZERO,
            validation_registry_contract_address: Address::ZERO,
            rpc_url: String::new(),
        }
    }
}

impl ContractConfig {
    /// Create a new configuration for deployed contracts
    ///
    /// # Arguments
    /// * `rpc_url` - Ethereum RPC endpoint (HTTP or WebSocket)
    /// * `identity_registry_contract_address` - Address of deployed IdentityRegistry contract
    /// * `validation_registry_contract_address` - Address of deployed ValidationRegistry contract
    pub fn new(
        rpc_url: String,
        identity_registry_contract_address: Address,
        validation_registry_contract_address: Address,
    ) -> Self {
        Self {
            identity_registry_contract_address,
            validation_registry_contract_address,
            rpc_url,
        }
    }

    /// Create a configuration with Anvil local testnet defaults
    ///
    /// Uses deterministic Anvil deployment addresses based on standard nonce order:
    /// - IdentityRegistry deployed first (nonce 0)
    /// - ValidationRegistry deployed second (nonce 1)
    pub fn anvil_config() -> Self {
        Self {
            // Anvil deterministic addresses for deployer 0xf39F...2266 (account #0)
            // These assume deployment order: IdentityRegistry -> ValidationRegistry
            identity_registry_contract_address: "0x5FbDB2315678afecb367f032d93F642f64180aa3"
                .parse::<Address>()
                .expect("Invalid token address"),
            validation_registry_contract_address: "0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512"
                .parse::<Address>()
                .expect("Invalid validation registry address"),
            rpc_url: "http://127.0.0.1:8545".to_string(),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_config_creation() {
        let identity_registry_address = "0x89c1312Cedb0B0F67e4913D2076bd4a860652B69"
            .parse::<Address>()
            .unwrap();
        let validation_registry_address = "0xDc64a140Aa3E981100a9becA4E685f962f0cF6C9"
            .parse::<Address>()
            .unwrap();

        let config = ContractConfig::new(
            "http://localhost:8545".to_string(),
            identity_registry_address,
            validation_registry_address,
        );

        assert_eq!(
            config.identity_registry_contract_address,
            identity_registry_address
        );
        assert_eq!(
            config.validation_registry_contract_address,
            validation_registry_address
        );
        assert_eq!(config.rpc_url, "http://localhost:8545");
    }

    #[test]
    fn test_contract_address_parsing() {
        let addr_str = "0x89c1312Cedb0B0F67e4913D2076bd4a860652B69";
        let addr = addr_str.parse::<Address>();
        assert!(addr.is_ok(), "Contract address should parse correctly");
    }
}
