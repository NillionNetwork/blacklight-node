use crate::json::stable_stringify;
use ethers::types::Bytes;
use serde::{Deserialize, Serialize};
use serde_with::{hex::Hex, serde_as};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadId {
    pub current: u64,
    pub previous: u64,
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
pub struct NilCcMeasurement {
    pub url: String,
    #[serde(rename = "nilcc_version")]
    pub nilcc_version: String,
    #[serde(rename = "cpu_count")]
    pub cpu_count: u64,
    #[serde(rename = "GPUs")]
    pub gpus: u64,
    #[serde_as(as = "Hex")]
    pub docker_compose_hash: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuilderMeasurement {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Htx {
    pub workload_id: WorkloadId,
    #[serde(rename = "nilCC_operator")]
    pub nilcc_operator: NilCcOperator,
    pub builder: Builder,
    #[serde(rename = "nilCC_measurement")]
    pub nilcc_measurement: NilCcMeasurement,
    pub builder_measurement: BuilderMeasurement,
}

/// Convert HTX to bytes for on-chain submission.
/// Uses stable_stringify to ensure deterministic JSON serialization with sorted keys.
/// This is critical because:
/// 1. The HTX ID is derived from keccak256(abi.encode(rawHTXHash, sender, blockNumber))
/// 2. Different JSON key orderings would produce different hashes
/// 3. Non-deterministic serialization would break verification and assignment matching
impl TryInto<Bytes> for Htx {
    type Error = anyhow::Error;
    fn try_into(self) -> Result<Bytes, Self::Error> {
        let json = stable_stringify(&self)?;
        Ok(Bytes::from(json.into_bytes()))
    }
}

/// Convert HTX reference to bytes for on-chain submission.
/// See the impl for `Htx` for details on why we use stable_stringify.
impl TryInto<Bytes> for &Htx {
    type Error = anyhow::Error;
    fn try_into(self) -> Result<Bytes, Self::Error> {
        let json = stable_stringify(self)?;
        Ok(Bytes::from(json.into_bytes()))
    }
}

// Legacy types (from WebSocket-based architecture, kept for compatibility)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignmentMsg {
    #[serde(rename = "type")]
    pub msg_type: String, // "assignment"
    pub slot: u64,
    #[serde(rename = "nodeId")]
    pub node_id: String,
    pub htx: Htx,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredMsg {
    #[serde(rename = "type")]
    pub msg_type: String, // "registered"
    #[serde(rename = "nodeId")]
    pub node_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionEnvelope {
    pub htx: Htx,
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationPayload {
    pub transaction: TransactionEnvelope,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResultMsg {
    #[serde(rename = "type")]
    pub msg_type: String, // "verification_result"
    #[serde(rename = "nodeId")]
    pub node_id: String,
    pub slot: u64,
    pub payload: VerificationPayload,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_htx_deterministic_serialization() {
        // Create an HTX
        let htx = Htx {
            workload_id: WorkloadId {
                current: 1,
                previous: 0,
            },
            nilcc_operator: NilCcOperator {
                id: 123,
                name: "test-operator".to_string(),
            },
            builder: Builder {
                id: 456,
                name: "test-builder".to_string(),
            },
            nilcc_measurement: NilCcMeasurement {
                url: "https://example.com/measurement".to_string(),
                nilcc_version: "1.0.0".to_string(),
                cpu_count: 8,
                gpus: 2,
                docker_compose_hash: [0; 32],
            },
            builder_measurement: BuilderMeasurement {
                url: "https://example.com/builder".to_string(),
            },
        };

        // Serialize the same HTX multiple times
        let bytes1: Result<Bytes, _> = htx.clone().try_into();
        let bytes2: Result<Bytes, _> = htx.clone().try_into();
        let bytes3: Result<Bytes, _> = htx.try_into();

        // All should be identical (deterministic)
        assert!(bytes1.is_ok());
        assert!(bytes2.is_ok());
        assert!(bytes3.is_ok());

        let b1 = bytes1.unwrap();
        let b2 = bytes2.unwrap();
        let b3 = bytes3.unwrap();

        assert_eq!(b1, b2);
        assert_eq!(b2, b3);

        // Verify keys are sorted in the JSON
        let json_str = String::from_utf8(b1.to_vec()).unwrap();

        // The JSON should have keys sorted alphabetically
        // For the top-level object: builder, builder_measurement, nilCC_measurement, nilCC_operator, workload_id
        assert!(json_str.contains("\"builder\""));
        assert!(json_str.contains("\"builder_measurement\""));
        assert!(json_str.contains("\"nilCC_measurement\""));
        assert!(json_str.contains("\"nilCC_operator\""));
        assert!(json_str.contains("\"workload_id\""));

        // Verify consistent ordering by checking position of keys
        let builder_pos = json_str.find("\"builder\"").unwrap();
        let builder_meas_pos = json_str.find("\"builder_measurement\"").unwrap();
        let nilcc_meas_pos = json_str.find("\"nilCC_measurement\"").unwrap();
        let nilcc_op_pos = json_str.find("\"nilCC_operator\"").unwrap();
        let workload_pos = json_str.find("\"workload_id\"").unwrap();

        // Keys should appear in alphabetical order
        assert!(builder_pos < builder_meas_pos);
        assert!(builder_meas_pos < nilcc_meas_pos);
        assert!(nilcc_meas_pos < nilcc_op_pos);
        assert!(nilcc_op_pos < workload_pos);
    }
}
