use crate::types::Htx;
use reqwest::Client;

#[derive(Debug)]
pub enum VerificationError {
    NilccUrl(String),
    NilccJson(String),
    MissingMeasurement,
    BuilderUrl(String),
    BuilderJson(String),
    NotInBuilderIndex,
}

impl VerificationError {
    pub fn message(&self) -> String {
        match self {
            VerificationError::NilccUrl(e) => format!("invalid nilcc_measurement URL: {}", e),
            VerificationError::NilccJson(e) => format!("invalid nilcc_measurement JSON: {}", e),
            VerificationError::MissingMeasurement => {
                "missing `measurement` field (looked at root and report.measurement)".to_string()
            }
            VerificationError::BuilderUrl(e) => format!("invalid builder_measurement URL: {}", e),
            VerificationError::BuilderJson(e) => {
                format!("invalid builder_measurement JSON: {}", e)
            }
            VerificationError::NotInBuilderIndex => {
                "measurement not found in builder index".to_string()
            }
        }
    }
}

impl std::fmt::Display for VerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for VerificationError {}

/// Verify an HTX by checking if the nilCC measurement exists in the builder index.
///
/// Steps:
/// 1. Fetch the nilCC measurement from the HTX's nilcc_measurement.url
/// 2. Extract the measurement value (looks at root.measurement or report.measurement)
/// 3. Fetch the builder measurement index from the HTX's builder_measurement.url
/// 4. Check if the measurement exists in the builder index (as object values or array elements)
///
/// Returns Ok(()) if verification succeeds, Err(VerificationError) otherwise.
pub async fn verify_htx(htx: &Htx) -> Result<(), VerificationError> {
    let client = Client::new();

    // Fetch nilcc measurement
    let meas_url = &htx.nilcc_measurement.url;
    let meas_resp = client.get(meas_url).send().await;
    let meas_json: serde_json::Value = match meas_resp.and_then(|r| r.error_for_status()) {
        Ok(resp) => match resp.json().await {
            Ok(v) => v,
            Err(e) => return Err(VerificationError::NilccJson(e.to_string())),
        },
        Err(e) => return Err(VerificationError::NilccUrl(e.to_string())),
    };

    // Extract measurement (try root.measurement first, then report.measurement)
    let measurement = meas_json
        .get("measurement")
        .and_then(|v| v.as_str())
        .or_else(|| {
            meas_json
                .get("report")
                .and_then(|r| r.get("measurement"))
                .and_then(|v| v.as_str())
        });
    let measurement = match measurement {
        Some(s) => s.to_string(),
        None => return Err(VerificationError::MissingMeasurement),
    };

    // Fetch builder measurement index
    let builder_resp = client.get(&htx.builder_measurement.url).send().await;
    let builder_json: serde_json::Value = match builder_resp.and_then(|r| r.error_for_status()) {
        Ok(resp) => match resp.json().await {
            Ok(v) => v,
            Err(e) => return Err(VerificationError::BuilderJson(e.to_string())),
        },
        Err(e) => return Err(VerificationError::BuilderUrl(e.to_string())),
    };

    // Check if measurement exists in builder index
    let mut matches_any = false;
    match builder_json {
        serde_json::Value::Object(map) => {
            for (_k, v) in map {
                if let Some(val) = v.as_str() {
                    if val == measurement {
                        matches_any = true;
                        break;
                    }
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                if let Some(val) = v.as_str() {
                    if val == measurement {
                        matches_any = true;
                        break;
                    }
                }
            }
        }
        _ => {}
    }

    if matches_any {
        Ok(())
    } else {
        Err(VerificationError::NotInBuilderIndex)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verification_error_messages() {
        let err = VerificationError::NilccUrl("connection failed".to_string());
        assert!(err.message().contains("invalid nilcc_measurement URL"));

        let err = VerificationError::MissingMeasurement;
        assert!(err.message().contains("missing `measurement` field"));

        let err = VerificationError::NotInBuilderIndex;
        assert!(err.message().contains("not found in builder index"));
    }
}
