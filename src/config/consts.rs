/// Centralized constants with different values for debug and release builds
///
/// This module provides all CLI and configuration constants used throughout the application.
/// Values are conditionally compiled based on the build profile (debug vs release).

// =============================================================================
// State File Names
// =============================================================================

pub const STATE_FILE_NODE: &str = "nilav_node.env";
pub const STATE_FILE_SIMULATOR: &str = "nilcc_simulator.env";
pub const STATE_FILE_MONITOR: &str = "nilav_monitor.env";

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
pub const DEFAULT_CONTRACT_ADDRESS: &str = "0x5FbDB2315678afecb367f032d93F642f64180aa3";
#[cfg(not(debug_assertions))]
pub const DEFAULT_CONTRACT_ADDRESS: &str = "0x4f071c297EF53565A86c634C9AAf5faCa89f6209";

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
