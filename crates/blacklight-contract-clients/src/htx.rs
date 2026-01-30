use alloy::dyn_abi::{DynSolType, DynSolValue};
use alloy::primitives::{Address, B256, Bytes, U256};
use alloy::sol_types::SolValue;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use serde_with::{hex::Hex, serde_as};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadId {
    pub current: String,
    pub previous: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NilCcOperator {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Builder {
    pub id: u64,
    pub name: String,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadMeasurement {
    pub url: String,
    pub artifacts_version: String,
    pub cpus: u64,
    pub gpus: u64,
    #[serde_as(as = "Hex")]
    pub docker_compose_hash: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuilderMeasurement {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NillionHtxV1 {
    pub workload_id: WorkloadId,
    pub operator: Option<NilCcOperator>,
    pub builder: Option<Builder>,
    pub workload_measurement: WorkloadMeasurement,
    pub builder_measurement: BuilderMeasurement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "version", rename_all = "camelCase")]
pub enum NillionHtx {
    /// The first HTX format version.
    V1(NillionHtxV1),
}

impl From<NillionHtxV1> for NillionHtx {
    fn from(htx: NillionHtxV1) -> Self {
        NillionHtx::V1(htx)
    }
}

// Phala HTX types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhalaAttestData {
    pub quote: String,
    pub event_log: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhalaHtxV1 {
    pub app_compose: String,
    pub attest_data: PhalaAttestData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "version", rename_all = "camelCase")]
pub enum PhalaHtx {
    /// The first HTX format version.
    V1(PhalaHtxV1),
}

// ERC-8004 Validation HTX - ABI encoded from ValidationRegistry
// Solidity: abi.encode(validatorAddress, agentId, requestURI, requestHash)

/// ERC-8004 Validation HTX data parsed from ABI-encoded bytes
#[derive(Debug, Clone)]
pub struct Erc8004Htx {
    pub validator_address: Address,
    pub agent_id: U256,
    pub request_uri: String,
    pub request_hash: B256,
}

impl Erc8004Htx {
    /// Try to decode ABI-encoded ERC-8004 validation data
    /// Format: abi.encode(validatorAddress, agentId, requestURI, requestHash)
    pub fn try_decode(data: &[u8]) -> Result<Self, Erc8004DecodeError> {
        // Use DynSolType::Tuple for proper ABI decoding of abi.encode() output
        // abi.encode() produces parameter encoding, so we use abi_decode_params on a tuple
        let tuple_type = DynSolType::Tuple(vec![
            DynSolType::Address,
            DynSolType::Uint(256),
            DynSolType::String,
            DynSolType::FixedBytes(32),
        ]);

        let decoded = tuple_type
            .abi_decode_params(data)
            .map_err(|e| Erc8004DecodeError(e.to_string()))?;

        // Extract values from the decoded tuple
        let values = match decoded {
            DynSolValue::Tuple(values) => values,
            _ => return Err(Erc8004DecodeError("Expected tuple".to_string())),
        };

        if values.len() != 4 {
            return Err(Erc8004DecodeError(format!(
                "Expected 4 values, got {}",
                values.len()
            )));
        }

        let validator_address = match &values[0] {
            DynSolValue::Address(addr) => *addr,
            _ => return Err(Erc8004DecodeError("Expected address".to_string())),
        };

        let agent_id = match &values[1] {
            DynSolValue::Uint(val, _) => *val,
            _ => return Err(Erc8004DecodeError("Expected uint256".to_string())),
        };

        let request_uri = match &values[2] {
            DynSolValue::String(s) => s.clone(),
            _ => return Err(Erc8004DecodeError("Expected string".to_string())),
        };

        let request_hash = match &values[3] {
            DynSolValue::FixedBytes(word, 32) => B256::from_slice(word.as_slice()),
            _ => return Err(Erc8004DecodeError("Expected bytes32".to_string())),
        };

        Ok(Self {
            validator_address,
            agent_id,
            request_uri,
            request_hash,
        })
    }
}

#[derive(Debug)]
pub struct Erc8004DecodeError(pub String);

impl std::fmt::Display for Erc8004DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ERC-8004 decode error: {}", self.0)
    }
}

impl std::error::Error for Erc8004DecodeError {}

// Unified HTX type that can represent nilCC, Phala, and ERC-8004 HTXs
#[derive(Debug, Clone)]
pub enum Htx {
    Nillion(NillionHtx),
    Phala(PhalaHtx),
    Erc8004(Erc8004Htx),
}

/// JSON-serializable HTX types (Nillion and Phala only, not ERC-8004)
/// Use this for loading HTXs from JSON files.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "camelCase")]
pub enum JsonHtx {
    Nillion(NillionHtx),
    Phala(PhalaHtx),
}

impl From<JsonHtx> for Htx {
    fn from(htx: JsonHtx) -> Self {
        match htx {
            JsonHtx::Nillion(htx) => Htx::Nillion(htx),
            JsonHtx::Phala(htx) => Htx::Phala(htx),
        }
    }
}

impl Htx {
    /// Parse HTX from raw bytes, trying JSON first then ABI decoding
    pub fn try_parse(data: &[u8]) -> Result<Self, HtxParseError> {
        // First try JSON parsing (nilCC and Phala)
        match serde_json::from_slice::<JsonHtx>(data) {
            Ok(json_htx) => {
                return Ok(match json_htx {
                    JsonHtx::Nillion(htx) => Htx::Nillion(htx),
                    JsonHtx::Phala(htx) => Htx::Phala(htx),
                });
            }
            Err(json_err) => {
                tracing::debug!(error = %json_err, "JSON parsing failed, trying ABI decode");
            }
        }

        // Then try ABI decoding (ERC-8004)
        match Erc8004Htx::try_decode(data) {
            Ok(erc8004_htx) => {
                return Ok(Htx::Erc8004(erc8004_htx));
            }
            Err(abi_err) => {
                tracing::debug!(error = %abi_err, data_len = data.len(), "ABI decoding failed");
            }
        }

        Err(HtxParseError::UnknownFormat)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HtxParseError {
    #[error("Unknown HTX format: not valid JSON or ABI-encoded ERC-8004")]
    UnknownFormat,
}

impl From<NillionHtx> for Htx {
    fn from(htx: NillionHtx) -> Self {
        Htx::Nillion(htx)
    }
}

impl From<PhalaHtx> for Htx {
    fn from(htx: PhalaHtx) -> Self {
        Htx::Phala(htx)
    }
}

impl From<Erc8004Htx> for Htx {
    fn from(htx: Erc8004Htx) -> Self {
        Htx::Erc8004(htx)
    }
}

impl TryFrom<&Htx> for Bytes {
    type Error = anyhow::Error;

    fn try_from(htx: &Htx) -> Result<Self, Self::Error> {
        match htx {
            Htx::Nillion(htx) => {
                let json_htx = JsonHtx::Nillion(htx.clone());
                let json = canonicalize_json(&serde_json::to_value(json_htx)?);
                let json = serde_json::to_string(&json)?;
                Ok(Bytes::from(json.into_bytes()))
            }
            Htx::Phala(htx) => {
                let json_htx = JsonHtx::Phala(htx.clone());
                let json = canonicalize_json(&serde_json::to_value(json_htx)?);
                let json = serde_json::to_string(&json)?;
                Ok(Bytes::from(json.into_bytes()))
            }
            Htx::Erc8004(htx) => {
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
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_htx_deterministic_serialization() {
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
    fn test_htx_phala_serialization() {
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
    fn test_deserialize_phala() {
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

    #[test]
    fn test_deserialize_nillion() {
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
    fn test_erc8004_decode() {
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
        assert_eq!(htx.agent_id, U256::ZERO);
        assert_eq!(htx.request_uri, "https://api.nilai.nillion.network/");
    }
}
