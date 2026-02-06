//! JSON-Encoded HTX Formats
//!
//! This module contains all JSON-encoded HTX types from various providers.

pub mod nillion;
pub mod phala;

use serde::{Deserialize, Serialize};

pub use nillion::*;
pub use phala::*;

/// JSON-serializable HTX wrapper for deserialization from JSON files.
///
/// This enum encompasses all JSON-encoded HTX formats. Each variant corresponds
/// to a different confidential compute provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "camelCase")]
pub enum JsonHtx {
    /// Nillion confidential compute HTX.
    Nillion(NillionHtx),
    /// Phala confidential compute HTX.
    Phala(PhalaHtx),
}
