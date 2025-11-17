use ethers::types::Bytes;
use serde::{Deserialize, Serialize};

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

impl TryInto<Bytes> for Htx {
    type Error = serde_json::Error;
    fn try_into(self) -> Result<Bytes, Self::Error> {
        let json = serde_json::to_string(&self).expect("Failed to serialize Htx");
        Ok(Bytes::from(json.into_bytes()))
    }
}

impl TryInto<Bytes> for &Htx {
    type Error = serde_json::Error;
    fn try_into(self) -> Result<Bytes, Self::Error> {
        let json = serde_json::to_string(self)?;
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
