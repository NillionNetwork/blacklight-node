use alloy::primitives::Bytes;
use serde::{Deserialize, Serialize};
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
    pub nilcc_operator: Option<NilCcOperator>,
    pub builder: Option<Builder>,
    #[serde(rename = "nilCC_measurement")]
    pub nilcc_measurement: NilCcMeasurement,
    pub builder_measurement: BuilderMeasurement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "version", rename_all = "camelCase")]
pub enum VersionedHtx {
    /// The first HTX format version.
    V1(Htx),
}

impl From<Htx> for VersionedHtx {
    fn from(htx: Htx) -> Self {
        VersionedHtx::V1(htx)
    }
}

impl TryFrom<&VersionedHtx> for Bytes {
    type Error = anyhow::Error;

    fn try_from(htx: &VersionedHtx) -> Result<Self, Self::Error> {
        // Convert into a json::Value first to ensure keys are sorted
        let json = serde_json::to_value(htx)?;
        let json = serde_json::to_string(&json)?;
        Ok(Bytes::from(json.into_bytes()))
    }
}

// Phala HTX types

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestData {
    pub quote: String,
    #[serde(rename = "event_log")]
    pub event_log: String,
    #[serde(rename = "vm_config")]
    pub vm_config: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HtxPhala {
    #[serde(rename = "app_compose")]
    pub app_compose: String,
    #[serde(rename = "attest_data")]
    pub attest_data: AttestData,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_htx_deterministic_serialization() {
        // Create an HTX
        let htx = Htx {
            workload_id: WorkloadId {
                current: "1".into(),
                previous: Some("0".into()),
            },
            nilcc_operator: Some(NilCcOperator {
                id: 123,
                name: "test-operator".to_string(),
            }),
            builder: Some(Builder {
                id: 456,
                name: "test-builder".to_string(),
            }),
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
        let htx = VersionedHtx::V1(htx);

        // Serialize the same HTX multiple times
        let b1 = Bytes::try_from(&htx).unwrap();
        let b2 = Bytes::try_from(&htx).unwrap();
        let b3 = Bytes::try_from(&htx).unwrap();

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

    #[test]
    fn test_htx_phala_serialization() {
        let htx_phala = HtxPhala {
            app_compose: "test-compose-config".to_string(),
            attest_data: AttestData {
                quote: "test-quote-hex".to_string(),
                event_log: r#"[{"event":"compose-hash","event_payload":"abc123"}]"#.to_string(),
                vm_config: r#"{}"#.to_string(),
            },
        };

        let json = serde_json::to_string(&htx_phala).unwrap();
        assert!(json.contains("\"app_compose\""));
        assert!(json.contains("\"attest_data\""));
        assert!(json.contains("test-compose-config"));

        let deserialized: HtxPhala = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.app_compose, "test-compose-config");
        assert_eq!(deserialized.attest_data.quote, "test-quote-hex");
    }
}
