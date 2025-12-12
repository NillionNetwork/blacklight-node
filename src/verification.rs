use std::path::PathBuf;
use std::sync::Arc;

use crate::types::Htx;
use anyhow::Context;
use attestation_verification::sev::firmware::guest::AttestationReport;
use attestation_verification::{
    DefaultCertificateFetcher, MeasurementGenerator, ReportBundle, ReportFetcher, ReportVerifier,
};
use reqwest::Client;

const ARTIFACTS_URL: &str = "https://nilcc.s3.eu-west-1.amazonaws.com";

#[derive(Debug)]
pub enum VerificationError {
    NilccUrl(String),
    NilccJson(String),
    FetchReport(String),
    VerifyReport(String),
    MeasurementHash(String),
    BuilderUrl(String),
    BuilderJson(String),
    NotInBuilderIndex,
}

impl VerificationError {
    pub fn message(&self) -> String {
        use VerificationError::*;
        match self {
            NilccUrl(e) => format!("invalid nilcc_measurement URL: {e}"),
            NilccJson(e) => format!("invalid nilcc_measurement JSON: {e}"),
            FetchReport(e) => format!("could not fetch attestation report: {e}"),
            VerifyReport(e) => format!("could not verify attestation report: {e}"),
            MeasurementHash(e) => format!("could not generate measurement hash: {e}"),
            BuilderUrl(e) => format!("invalid builder_measurement URL: {e}"),
            BuilderJson(e) => {
                format!("invalid builder_measurement JSON: {e}")
            }
            NotInBuilderIndex => "measurement not found in builder index".to_string(),
        }
    }
}

impl std::fmt::Display for VerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for VerificationError {}

#[derive(Clone)]
pub struct HtxVerifier {
    report_fetcher: Arc<ReportFetcher>,
    report_verifier: Arc<ReportVerifier>,
    artifact_cache: PathBuf,
}

impl HtxVerifier {
    pub fn new(artifact_cache: PathBuf, cert_cache: PathBuf) -> anyhow::Result<Self> {
        let report_fetcher = ReportFetcher::new(artifact_cache.clone(), ARTIFACTS_URL.to_string());
        let fetcher =
            DefaultCertificateFetcher::new(cert_cache).context("Creating certificate fetcher")?;
        let report_verifier = ReportVerifier::new(Arc::new(fetcher));
        Ok(Self {
            report_fetcher: Arc::new(report_fetcher),
            report_verifier: Arc::new(report_verifier),
            artifact_cache,
        })
    }

    /// Verify an HTX by checking if the nilCC measurement exists in the builder index.
    ///
    /// Steps:
    /// 1. Fetch the nilCC measurement from the HTX's nilcc_measurement.url
    /// 2. Extract the measurement value (looks at root.measurement or report.measurement)
    /// 3. Fetch the builder measurement index from the HTX's builder_measurement.url
    /// 4. Check if the measurement exists in the builder index (as object values or array elements)
    ///
    /// Returns Ok(()) if verification succeeds, Err(VerificationError) otherwise.
    pub async fn verify_htx(&self, htx: &Htx) -> Result<(), VerificationError> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("Failed to build HTTP client");

        let report = self
            .verify_report(
                &htx.nilcc_measurement.url,
                htx.nilcc_measurement.docker_compose_hash,
            )
            .await?;

        // Fetch builder measurement index
        let builder_resp = client.get(&htx.builder_measurement.url).send().await;
        let builder_json: serde_json::Value = match builder_resp.and_then(|r| r.error_for_status())
        {
            Ok(resp) => match resp.json().await {
                Ok(v) => v,
                Err(e) => return Err(VerificationError::BuilderJson(e.to_string())),
            },
            Err(e) => return Err(VerificationError::BuilderUrl(e.to_string())),
        };

        // Check if measurement exists in builder index
        let mut matches_any = false;
        let measurement_hex = hex::encode(report.measurement);
        match builder_json {
            serde_json::Value::Object(map) => {
                for (_k, v) in map {
                    if let Some(val) = v.as_str() {
                        if val == measurement_hex {
                            matches_any = true;
                            break;
                        }
                    }
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    if let Some(val) = v.as_str() {
                        if val == measurement_hex {
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

    async fn verify_report(
        &self,
        report_url: &str,
        docker_compose_hash: [u8; 32],
    ) -> Result<AttestationReport, VerificationError> {
        let bundle = self
            .report_fetcher
            .fetch_report(report_url)
            .await
            .map_err(|e| VerificationError::FetchReport(e.to_string()))?;
        let ReportBundle {
            cpu_count,
            nilcc_version,
            metadata,
            vm_type,
            ..
        } = bundle;

        let artifacts_path = self.artifact_cache.join(&nilcc_version);
        let measurement = MeasurementGenerator::new(
            docker_compose_hash,
            cpu_count,
            vm_type.into(),
            &metadata,
            &artifacts_path,
        )
        .generate()
        .map_err(|e| VerificationError::MeasurementHash(e.to_string()))?;
        self.report_verifier
            .verify_report(&bundle.report, &measurement)
            .await
            .map_err(|e| VerificationError::VerifyReport(e.to_string()))?;
        Ok(bundle.report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verification_error_messages() {
        let err = VerificationError::NilccUrl("connection failed".to_string());
        assert!(err.message().contains("invalid nilcc_measurement URL"));

        let err = VerificationError::NotInBuilderIndex;
        assert!(err.message().contains("not found in builder index"));
    }
}