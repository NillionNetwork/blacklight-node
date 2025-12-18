use alloy::primitives::Bytes;
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

// Unified HTX type that can represent both nilCC and Phala HTXs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "camelCase")]
pub enum Htx {
    Nillion(NillionHtx),
    Phala(PhalaHtx),
}

impl From<NillionHtx> for Htx {
    fn from(htx: NillionHtx) -> Self {
        Htx::Nillion(htx)
    }
}

impl TryFrom<&Htx> for Bytes {
    type Error = anyhow::Error;

    fn try_from(htx: &Htx) -> Result<Self, Self::Error> {
        let json = canonicalize_json(&serde_json::to_value(htx)?);
        let json = serde_json::to_string(&json)?;
        Ok(Bytes::from(json.into_bytes()))
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

        let htx: Htx = serde_json::from_str(phala_json).unwrap();
        let Htx::Phala(PhalaHtx::V1(htx)) = htx else {
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

        let htx: Htx = serde_json::from_str(nilcc_json).unwrap();
        assert!(matches!(htx, Htx::Nillion(_)), "not a nillion HTX");
    }
}
