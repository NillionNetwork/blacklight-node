use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{fs, path::Path};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NilCcMeasurement {
    pub url: String,
    #[serde(rename = "nilcc_version")]
    pub nilcc_version: String,
    #[serde(rename = "cpu_count")]
    pub cpu_count: u64,
    #[serde(rename = "GPUs")]
    pub gpus: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuilderMeasurement {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Htx {
    pub workload_id: WorkloadId,
    #[serde(rename = "nilCC_operator")]
    pub nil_cc_operator: NilCcOperator,
    pub builder: Builder,
    #[serde(rename = "nilCC_measurement")]
    pub nil_cc_measurement: NilCcMeasurement,
    pub builder_measurement: BuilderMeasurement,
}

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

pub fn canonicalize_json(value: &Value) -> Value {
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

pub fn stable_stringify<T: Serialize>(value: &T) -> anyhow::Result<String> {
    let v = serde_json::to_value(value)?;
    let c = canonicalize_json(&v);
    Ok(serde_json::to_string(&c)?)
}

pub fn choose_k<'a, T: Clone>(items: &'a [T], k: usize) -> Vec<T> {
    let mut rng = rand::rng();
    let mut shuffled = items.to_vec();
    shuffled.shuffle(&mut rng);
    shuffled.into_iter().take(k).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElectionConfig {
    pub validators_per_htx: usize,
    pub approve_threshold: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub election: ElectionConfig,
    #[serde(default = "default_slot_ms")]
    pub slot_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            election: ElectionConfig {
                validators_per_htx: 3,
                approve_threshold: 2,
            },
            slot_ms: default_slot_ms(),
        }
    }
}

pub fn load_config_from_path<P: AsRef<Path>>(path: P) -> anyhow::Result<Config> {
    let s = fs::read_to_string(path)?;
    let cfg: Config = toml::from_str(&s)?;
    Ok(cfg)
}

fn default_slot_ms() -> u64 {
    5000
}
