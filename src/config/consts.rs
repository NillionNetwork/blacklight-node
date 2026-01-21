//!  Centralized constants with different values for debug and release builds
//!
//! This module provides all CLI and configuration constants used throughout the application.
//! Values are conditionally compiled based on the build profile (debug vs release).

use alloy::primitives::U256;

// =============================================================================
// State File Names
// =============================================================================

pub const STATE_FILE_NODE: &str = "blacklight_node.env";
pub const STATE_FILE_SIMULATOR: &str = "nilcc_simulator.env";
pub const STATE_FILE_MONITOR: &str = "blacklight_monitor.env";
pub const STATE_FILE_KEEPER: &str = "niluv_keeper.env";

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
