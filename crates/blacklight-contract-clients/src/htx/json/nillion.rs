//! Nillion HTX - JSON-Encoded, Versioned
//!
//! TEE-based confidential compute workloads with hardware measurements.

use serde::{Deserialize, Serialize};
use serde_with::{hex::Hex, serde_as};

/// Nillion workload identifier with optional history tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadId {
    pub current: String,
    pub previous: Option<String>,
}

/// Nillion confidential compute operator information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NilCcOperator {
    pub id: u64,
    pub name: String,
}

/// Builder information for Nillion workloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Builder {
    pub id: u64,
    pub name: String,
}

/// Measurement data for a Nillion workload including hardware requirements.
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

/// Builder measurement data for Nillion workloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuilderMeasurement {
    pub url: String,
}

/// Nillion HTX Version 1 - Initial format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NillionHtxV1 {
    pub workload_id: WorkloadId,
    pub operator: Option<NilCcOperator>,
    pub builder: Option<Builder>,
    pub workload_measurement: WorkloadMeasurement,
    pub builder_measurement: BuilderMeasurement,
}

/// Versioned Nillion HTX format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "version", rename_all = "camelCase")]
pub enum NillionHtx {
    /// Version 1: Initial Nillion HTX format.
    V1(NillionHtxV1),
}

impl From<NillionHtxV1> for NillionHtx {
    fn from(htx: NillionHtxV1) -> Self {
        NillionHtx::V1(htx)
    }
}
