//! Phala HTX - JSON-Encoded, Versioned
//!
//! TEE-based confidential compute with attestation data.

use serde::{Deserialize, Serialize};

/// Phala attestation data containing quote and event logs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhalaAttestData {
    pub quote: String,
    pub event_log: String,
}

/// Phala HTX Version 1 - Initial format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhalaHtxV1 {
    pub app_compose: String,
    pub attest_data: PhalaAttestData,
}

/// Versioned Phala HTX format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "version", rename_all = "camelCase")]
pub enum PhalaHtx {
    /// Version 1: Initial Phala HTX format.
    V1(PhalaHtxV1),
}
