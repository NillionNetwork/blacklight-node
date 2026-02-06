//! # HTX Data Types
//!
//! This module provides a taxonomy of HTX formats organized by encoding type and version.
//!
//! ## Taxonomy
//!
//! ### JSON-Encoded HTXs ([`json`] module)
//! - **Nillion** ([`json::nillion`]): TEE-based confidential compute workloads
//!   - V1: Initial version with workload measurements, operators, and builders
//! - **Phala** ([`json::phala`]): TEE-based confidential compute with attestation
//!   - V1: Initial version with app composition and attestation data
//!
//! ### ABI-Encoded HTXs ([`abi`] module)
//! - **ERC-8004** ([`abi::erc8004`]): On-chain validation standard for agent validations
//!   - Format: `abi.encode(validatorAddress, agentId, requestURI, requestHash)`
//!
//! ## Main Types
//!
//! - [`Htx`]: Unified enum for all HTX types to be used externally
//! - [`JsonHtx`]: JSON-serializable HTX formats (Nillion, Phala)
//! - [`AbiHtx`]: ABI-encoded HTX formats (ERC-8004)

pub mod abi;
pub mod json;

use alloy::primitives::Bytes;
use alloy::sol_types::SolValue;
use serde_json::{Map, Value};

pub use abi::*;
pub use json::*;

/// Unified HTX type encompassing all supported formats.
///
/// This enum provides a common interface for working with different HTX types.
/// The variants are organized by provider/standard for convenient pattern matching.
///
/// The module structure organizes types by encoding (json/ and abi/ modules),
/// but the enum is flat for ease of use.
#[derive(Debug, Clone)]
pub enum Htx {
    /// Nillion confidential compute HTX (JSON-encoded, versioned).
    Nillion(json::NillionHtx),
    /// Phala confidential compute HTX (JSON-encoded, versioned).
    Phala(json::PhalaHtx),
    /// ERC-8004 validation HTX (ABI-encoded).
    Erc8004(abi::Erc8004Htx),
}

impl From<json::NillionHtx> for Htx {
    fn from(htx: json::NillionHtx) -> Self {
        Htx::Nillion(htx)
    }
}

impl From<json::PhalaHtx> for Htx {
    fn from(htx: json::PhalaHtx) -> Self {
        Htx::Phala(htx)
    }
}

impl From<abi::Erc8004Htx> for Htx {
    fn from(htx: abi::Erc8004Htx) -> Self {
        Htx::Erc8004(htx)
    }
}

// Internal conversions for parsing
impl From<JsonHtx> for Htx {
    fn from(htx: JsonHtx) -> Self {
        match htx {
            JsonHtx::Nillion(htx) => Htx::Nillion(htx),
            JsonHtx::Phala(htx) => Htx::Phala(htx),
        }
    }
}

impl From<AbiHtx> for Htx {
    fn from(htx: AbiHtx) -> Self {
        match htx {
            AbiHtx::Erc8004(htx) => Htx::Erc8004(htx),
        }
    }
}

impl Htx {
    /// Parse HTX from raw bytes using auto-detection.
    ///
    /// This method attempts to parse the data in the following order:
    /// 1. JSON-encoded HTX (Nillion or Phala)
    /// 2. ABI-encoded HTX (ERC-8004)
    ///
    /// # Errors
    ///
    /// Returns `HtxParseError::UnknownFormat` if the data doesn't match any
    /// supported HTX format.
    pub fn try_parse(data: &[u8]) -> Result<Self, HtxParseError> {
        if let Ok(json_htx) = serde_json::from_slice::<JsonHtx>(data) {
            return Ok(json_htx.into());
        }

        if let Ok(abi_htx) = AbiHtx::try_decode(data) {
            return Ok(abi_htx.into());
        }

        Err(HtxParseError::UnknownFormat)
    }
}

/// Error type for HTX parsing failures.
#[derive(Debug, thiserror::Error)]
pub enum HtxParseError {
    #[error("Unknown HTX format: not valid JSON or ABI-encoded")]
    UnknownFormat,
}

// ============================================================================
// Serialization & Encoding
// ============================================================================

impl TryFrom<&Htx> for Bytes {
    type Error = anyhow::Error;

    /// Convert an HTX to its wire format (bytes).
    ///
    /// - JSON-encoded HTXs (Nillion, Phala) are serialized as canonical JSON
    /// - ABI-encoded HTXs (ERC-8004) are encoded according to their ABI specification
    fn try_from(htx: &Htx) -> Result<Self, Self::Error> {
        match htx {
            Htx::Nillion(htx) => json_htx_to_bytes(JsonHtx::Nillion(htx.clone())),
            Htx::Phala(htx) => json_htx_to_bytes(JsonHtx::Phala(htx.clone())),
            Htx::Erc8004(htx) => abi_htx_to_bytes(&AbiHtx::Erc8004(htx.clone())),
        }
    }
}

/// Serialize a JSON HTX to bytes with canonical JSON formatting.
///
/// Canonical formatting ensures deterministic serialization by sorting
/// all object keys alphabetically.
fn json_htx_to_bytes(htx: JsonHtx) -> Result<Bytes, anyhow::Error> {
    let json = canonicalize_json(&serde_json::to_value(htx)?);
    let json = serde_json::to_string(&json)?;
    Ok(Bytes::from(json.into_bytes()))
}

/// Encode an ABI HTX to bytes according to its ABI specification.
fn abi_htx_to_bytes(htx: &AbiHtx) -> Result<Bytes, anyhow::Error> {
    match htx {
        AbiHtx::Erc8004(htx) => {
            let tuple = (
                htx.validator_address,
                htx.agent_id,
                htx.request_uri.clone(),
                htx.request_hash,
            );
            Ok(Bytes::from(tuple.abi_encode()))
        }
    }
}

/// Canonicalize JSON by recursively sorting all object keys.
///
/// This ensures deterministic serialization regardless of insertion order.
fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted = Map::new();
            let mut keys: Vec<_> = map.keys().cloned().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), canonicalize_json(&map[&k]));
            }
            Value::Object(sorted)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(canonicalize_json).collect()),
        _ => value.clone(),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------
    // JSON-Encoded HTX Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_nillion_deterministic_serialization() {
        use json::*;

        // Create an HTX
        let htx = NillionHtxV1 {
            workload_id: WorkloadId {
                current: "1".into(),
                previous: Some("0".into()),
            },
            operator: Some(NilCcOperator {
                id: 123,
                name: "test-operator".to_string(),
            }),
            builder: Some(Builder {
                id: 456,
                name: "test-builder".to_string(),
            }),
            workload_measurement: WorkloadMeasurement {
                url: "https://example.com/measurement".to_string(),
                artifacts_version: "1.0.0".to_string(),
                cpus: 8,
                gpus: 2,
                docker_compose_hash: [0; 32],
            },
            builder_measurement: BuilderMeasurement {
                url: "https://example.com/builder".to_string(),
            },
        };
        let htx = Htx::Nillion(NillionHtx::V1(htx));

        // Serialize the same HTX multiple times
        let b1 = Bytes::try_from(&htx).unwrap();
        let b2 = Bytes::try_from(&htx).unwrap();
        let b3 = Bytes::try_from(&htx).unwrap();

        assert_eq!(b1, b2);
        assert_eq!(b2, b3);

        // Ensure all top level keys show up in sorted order
        let json_str = String::from_utf8(b1.to_vec()).unwrap();
        let mut keys = [
            "builder",
            "builder_measurement",
            "operator",
            "workload_id",
            "workload_measurement",
        ];
        keys.sort();
        let mut last_index = 0;
        for key in keys {
            let index = json_str
                .find(&format!("\"{key}\""))
                .expect(&format!("key '{key}' not found"));
            assert!(index > last_index);
            last_index = index;
        }
    }

    #[test]
    fn test_nillion_deserialization() {
        let nilcc_json = r#"{
            "provider": "nillion",
            "version": "v1",
            "workload_id": {
                "current": "1",
                "previous": null
            },
            "workload_measurement": {
                "url": "https://example.com/measurement",
                "artifacts_version": "1.0.0",
                "cpus": 8,
                "gpus": 0,
                "docker_compose_hash": "0000000000000000000000000000000000000000000000000000000000000000"
            },
            "builder_measurement": {
                "url": "https://example.com/builder"
            }
        }"#;

        let htx: JsonHtx = serde_json::from_str(nilcc_json).unwrap();
        assert!(matches!(htx, JsonHtx::Nillion(_)), "not a nillion HTX");
    }

    #[test]
    fn test_phala_serialization() {
        use json::*;

        let htx_phala = PhalaHtxV1 {
            app_compose: "test-compose-config".to_string(),
            attest_data: PhalaAttestData {
                quote: "test-quote-hex".to_string(),
                event_log: r#"[{"event":"compose-hash","event_payload":"abc123"}]"#.to_string(),
            },
        };

        let json = serde_json::to_string(&htx_phala).unwrap();
        assert!(json.contains("\"app_compose\""));
        assert!(json.contains("\"attest_data\""));
        assert!(json.contains("test-compose-config"));

        let deserialized: PhalaHtxV1 = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.app_compose, "test-compose-config");
        assert_eq!(deserialized.attest_data.quote, "test-quote-hex");
    }

    #[test]
    fn test_phala_deserialization() {
        let phala_json = r#"{
            "provider": "phala",
            "version": "v1",
            "app_compose": "test-compose",
            "attest_data": {
                "quote": "test-quote",
                "event_log": "[]"
            }
        }"#;

        let htx: JsonHtx = serde_json::from_str(phala_json).unwrap();
        let JsonHtx::Phala(PhalaHtx::V1(htx)) = htx else {
            panic!("not a phala HTX");
        };
        assert_eq!(htx.app_compose, "test-compose");
    }

    // ------------------------------------------------------------------------
    // ABI-Encoded HTX Tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_erc8004_decode() {
        use alloy::primitives::Address;

        // Test data: abi.encode(0x5fc8d32690cc91d4c39d9d3abcbd16989f875707, 0, "https://api.nilai.nillion.network/", 0xa6719a2ea05fac172c1b20e16beea2a9739b715499a3a9ad488e6ce81602ffac)
        let raw_hex = "0000000000000000000000005fc8d32690cc91d4c39d9d3abcbd16989f87570700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000080a6719a2ea05fac172c1b20e16beea2a9739b715499a3a9ad488e6ce81602ffac000000000000000000000000000000000000000000000000000000000000002268747470733a2f2f6170692e6e696c61692e6e696c6c696f6e2e6e6574776f726b2f000000000000000000000000000000000000000000000000000000000000";
        let data = alloy::hex::decode(raw_hex).unwrap();

        let htx = Erc8004Htx::try_decode(&data).expect("should decode ERC-8004 HTX");
        assert_eq!(
            htx.validator_address,
            "0x5fc8d32690cc91d4c39d9d3abcbd16989f875707"
                .parse::<Address>()
                .unwrap()
        );
        assert_eq!(htx.agent_id, alloy::primitives::U256::ZERO);
        assert_eq!(htx.request_uri, "https://api.nilai.nillion.network/");
    }

    #[test]
    fn test_htx_parse_json() {
        let json_data = r#"{
            "provider": "nillion",
            "version": "v1",
            "workload_id": {
                "current": "1",
                "previous": null
            },
            "workload_measurement": {
                "url": "https://example.com/measurement",
                "artifacts_version": "1.0.0",
                "cpus": 8,
                "gpus": 0,
                "docker_compose_hash": "0000000000000000000000000000000000000000000000000000000000000000"
            },
            "builder_measurement": {
                "url": "https://example.com/builder"
            }
        }"#;

        let htx = Htx::try_parse(json_data.as_bytes()).unwrap();
        assert!(matches!(htx, Htx::Nillion(_)));
    }

    #[test]
    fn test_htx_parse_abi() {
        let raw_hex = "0000000000000000000000005fc8d32690cc91d4c39d9d3abcbd16989f87570700000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000080a6719a2ea05fac172c1b20e16beea2a9739b715499a3a9ad488e6ce81602ffac000000000000000000000000000000000000000000000000000000000000002268747470733a2f2f6170692e6e696c61692e6e696c6c696f6e2e6e6574776f726b2f000000000000000000000000000000000000000000000000000000000000";
        let data = alloy::hex::decode(raw_hex).unwrap();

        let htx = Htx::try_parse(&data).unwrap();
        assert!(matches!(htx, Htx::Erc8004(_)));
    }
}
