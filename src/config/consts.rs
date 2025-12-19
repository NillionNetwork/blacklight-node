//!  Centralized constants with different values for debug and release builds
//!
//! This module provides all CLI and configuration constants used throughout the application.
//! Values are conditionally compiled based on the build profile (debug vs release).

use alloy::primitives::U256;

// =============================================================================
// State File Names
// =============================================================================

pub const STATE_FILE_NODE: &str = "niluv_node.env";
pub const STATE_FILE_SIMULATOR: &str = "nilcc_simulator.env";
pub const STATE_FILE_MONITOR: &str = "niluv_monitor.env";

// =============================================================================
// Default Configuration Values
// =============================================================================

/// Default RPC URL - points to localhost for local development
#[cfg(debug_assertions)]
pub const DEFAULT_RPC_URL: &str = "http://localhost:8545";
#[cfg(not(debug_assertions))]
pub const DEFAULT_RPC_URL: &str = "https://rpc-nilav-shzvox09l5.t.conduit.xyz";

/// Default contract address - Anvil's first deployed contract address
#[cfg(debug_assertions)]
pub const DEFAULT_MANAGER_CONTRACT_ADDRESS: &str = "0x3dbE95E20B370C5295E7436e2d887cFda8bcb02c";
#[cfg(not(debug_assertions))]
pub const DEFAULT_MANAGER_CONTRACT_ADDRESS: &str = "0x3dbE95E20B370C5295E7436e2d887cFda8bcb02c";

/// Default contract address - Anvil's first deployed contract address
#[cfg(debug_assertions)]
pub const DEFAULT_STAKING_CONTRACT_ADDRESS: &str = "0xe7f1725e7734ce288f8367e1bb143e90bb3f0512";
#[cfg(not(debug_assertions))]
pub const DEFAULT_STAKING_CONTRACT_ADDRESS: &str = "0x2913f0A4C1BE4e991CCf76F04C795E5646e02049";

/// Default contract address - Anvil's first deployed contract address
#[cfg(debug_assertions)]
pub const DEFAULT_TOKEN_CONTRACT_ADDRESS: &str = "0xf65b7cCF9f13ef932093bac19Eb5ea77ee70F4A4";
#[cfg(not(debug_assertions))]
pub const DEFAULT_TOKEN_CONTRACT_ADDRESS: &str = "0xf65b7cCF9f13ef932093bac19Eb5ea77ee70F4A4";

/// Default path to HTXs JSON file
pub const DEFAULT_HTXS_PATH: &str = "data/htxs.json";

// =============================================================================
// Simulator Configuration
// =============================================================================

/// Default slot interval in milliseconds - how often simulator submits HTXs
#[cfg(debug_assertions)]
pub const DEFAULT_SLOT_MS: u64 = 3000; // 3 seconds for debug (faster testing)

#[cfg(not(debug_assertions))]
pub const DEFAULT_SLOT_MS: u64 = 5000; // 5 seconds for release

// =============================================================================
// Node Reconnection Settings
// =============================================================================

/// Initial reconnection delay in seconds
pub const INITIAL_RECONNECT_DELAY_SECS: u64 = 1; // 1 second for debug
/// Maximum reconnection delay in seconds
pub const MAX_RECONNECT_DELAY_SECS: u64 = 60; // 60 seconds for release

// =============================================================================
// Contract Client Settings
// =============================================================================

/// Default number of blocks to look back when querying historical events
pub const DEFAULT_LOOKBACK_BLOCKS: u64 = 50; // Fewer blocks for release (performance)

/// Solidity Error(string) function selector
/// Used for decoding revert messages from contract calls
pub const ERROR_STRING_SELECTOR: &str = "08c379a0";

// =============================================================================
// Node Operation Settings
// =============================================================================

/// Convert ETH to wei at compile time
const fn eth_to_wei(eth: f64) -> U256 {
    let wei = (eth * 1_000_000_000_000_000_000.0) as u64;
    U256::from_limbs([wei, 0, 0, 0])
}

/// Minimum ETH balance required to continue operating
/// Node will initiate shutdown if balance falls below this threshold
pub const MIN_ETH_BALANCE: U256 = eth_to_wei(0.00001);
