use std::path::PathBuf;
use std::sync::Arc;

use crate::contract_client::heartbeat_manager::Verdict;
use crate::retry::{retry_with_classifier, RetryConfig};
use crate::types::{NillionHtx, PhalaHtx};
use anyhow::Context;
use attestation_verification::sev::firmware::guest::AttestationReport;
use attestation_verification::VerificationError as ExtVerificationError;
use attestation_verification::{
    DefaultCertificateFetcher, MeasurementGenerator, ReportBundle, ReportFetcher, ReportVerifier,
};
use dcap_qvl::collateral::get_collateral_and_verify;
use reqwest::Client;
use sha2::{Digest, Sha256};

const ARTIFACTS_URL: &str = "https://nilcc.s3.eu-west-1.amazonaws.com";

#[derive(Debug)]
pub enum VerificationError {
    // Inconclusive errors - operational/infrastructure failures
    NilccUrl(String),
    NilccJson(String),
    FetchReport(String),
    BuilderUrl(String),
    BuilderJson(String),
    PhalaEventLogParse(String),
    FetchCerts(String),
    DetectProcessor(String),

    // Malicious errors - cryptographic verification failures
    VerifyReport(String),
    MeasurementHash(String),
    NotInBuilderIndex,
    PhalaComposeHashMismatch,
    PhalaQuoteVerify(String),
    PhalaPlatformVerify(String),
    CertVerification(String),
    InvalidMeasurement(String),
    InvalidCertificate(String),
}

impl VerificationError {
    /// Returns the verdict for this error.
    ///
    /// - `Verdict::Failure`: Cryptographic verification failed, indicating potential tampering.
    /// - `Verdict::Inconclusive`: Operational failure (network, parsing, etc.) - cannot determine validity.
    ///
    /// Note: Never returns `Verdict::Success` since this is an error type.
    pub fn verdict(&self) -> Verdict {
        use VerificationError::*;
        match self {
            // Inconclusive - operational/infrastructure failures
            NilccUrl(_)
            | NilccJson(_)
            | FetchReport(_)
            | BuilderUrl(_)
            | BuilderJson(_)
            | PhalaEventLogParse(_)
            | FetchCerts(_)
            | InvalidCertificate(_)
            | DetectProcessor(_) => Verdict::Inconclusive,

            // Failure - cryptographic verification failures (indicates potential tampering)
            VerifyReport(_)
            | MeasurementHash(_)
            | NotInBuilderIndex
            | PhalaComposeHashMismatch
            | PhalaQuoteVerify(_)
            | PhalaPlatformVerify(_)
            | CertVerification(_)
            | InvalidMeasurement(_) => Verdict::Failure,
        }
    }

    /// Returns whether this error indicates a definitive verification failure.
    pub fn is_failure(&self) -> bool {
        self.verdict() == Verdict::Failure
    }

    /// Returns whether this error is inconclusive (operational failure).
    pub fn is_inconclusive(&self) -> bool {
        self.verdict() == Verdict::Inconclusive
    }

    pub fn message(&self) -> String {
        use VerificationError::*;
        match self {
            // Inconclusive errors
            NilccUrl(e) => format!("invalid nilcc_measurement URL: {e}"),
            NilccJson(e) => format!("invalid nilcc_measurement JSON: {e}"),
            FetchReport(e) => format!("could not fetch attestation report: {e}"),
            BuilderUrl(e) => format!("invalid builder_measurement URL: {e}"),
            BuilderJson(e) => format!("invalid builder_measurement JSON: {e}"),
            PhalaEventLogParse(e) => format!("failed to parse event_log: {e}"),
            FetchCerts(e) => format!("could not fetch AMD certificates: {e}"),
            DetectProcessor(e) => format!("could not detect processor type: {e}"),
            InvalidCertificate(e) => format!("invalid certificate obtained from AMD: {e}"),

            // Malicious errors
            VerifyReport(e) => format!("attestation report verification failed: {e}"),
            MeasurementHash(e) => format!("measurement hash verification failed: {e}"),
            NotInBuilderIndex => "measurement not found in builder index".to_string(),
            PhalaComposeHashMismatch => "compose-hash mismatch".to_string(),
            PhalaQuoteVerify(e) => format!("quote verification failed: {e}"),
            PhalaPlatformVerify(e) => format!("platform verification failed: {e}"),
            CertVerification(e) => format!("certificate chain verification failed: {e}"),
            InvalidMeasurement(e) => format!("measurement mismatch: {e}"),
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

    /// Verify a nillion HTX by checking if the nilCC measurement exists in the builder index.
    ///
    /// Steps:
    /// 1. Fetch the nilCC measurement from the HTX's nilcc_measurement.url
    /// 2. Extract the measurement value (looks at root.measurement or report.measurement)
    /// 3. Fetch the builder measurement index from the HTX's builder_measurement.url
    /// 4. Check if the measurement exists in the builder index (as object values or array elements)
    ///
    /// Returns Ok(()) if verification succeeds, Err(VerificationError) otherwise.
    pub async fn verify_nillion_htx(&self, htx: &NillionHtx) -> Result<(), VerificationError> {
        let NillionHtx::V1(htx) = htx;
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("Failed to build HTTP client");

        let report = self
            .verify_report(
                &htx.workload_measurement.url,
                htx.workload_measurement.docker_compose_hash,
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
            .map_err(|e: attestation_verification::VerificationError| {
                match e {
                    // Inconclusive errors - infrastructure/operational failures (outside of host control)
                    ExtVerificationError::FetchCerts(ref inner) => {
                        VerificationError::FetchCerts(inner.to_string())
                    }
                    ExtVerificationError::DetectProcessor(ref inner) => {
                        VerificationError::DetectProcessor(inner.to_string())
                    }
                    ExtVerificationError::InvalidCertificate(ref inner) => {
                        VerificationError::InvalidCertificate(inner.to_string())
                    }
                    // Any other verification failures treated as malicious
                    _ => VerificationError::VerifyReport(e.to_string()),
                }
            })?;
        Ok(bundle.report)
    }

    /// Verify a Phala HTX by checking compose hash and quote.
    ///
    /// Steps:
    /// 1. Calculate SHA-256 hash of app_compose
    /// 2. Extract attested hash from event_log (compose-hash event)
    /// 3. Verify hashes match
    /// 4. Verify quote locally using dcap-qvl (get_collateral_and_verify)
    ///
    /// Returns Ok(()) if verification succeeds, Err(VerificationError) otherwise.
    pub async fn verify_phala_htx(&self, htx: &PhalaHtx) -> Result<(), VerificationError> {
        let PhalaHtx::V1(htx) = htx;
        // 1. Calculate SHA-256 hash of app_compose
        let mut hasher = Sha256::new();
        hasher.update(htx.app_compose.as_bytes());
        let calculated_hash = hex::encode(hasher.finalize());

        // 2. Extract attested hash from event_log
        let events: Vec<serde_json::Value> = serde_json::from_str(&htx.attest_data.event_log)
            .map_err(|e| VerificationError::PhalaEventLogParse(e.to_string()))?;

        let compose_event = events
            .iter()
            .find(|e| e.get("event").and_then(|v| v.as_str()) == Some("compose-hash"))
            .ok_or_else(|| {
                VerificationError::PhalaEventLogParse("compose-hash event not found".to_string())
            })?;

        let attested_hash = compose_event
            .get("event_payload")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                VerificationError::PhalaEventLogParse("event_payload not found".to_string())
            })?;

        // 3. Verify hashes match
        if calculated_hash != attested_hash {
            return Err(VerificationError::PhalaComposeHashMismatch);
        }

        // 4. Verify quote locally using dcap-qvl
        let quote_bytes = hex::decode(&htx.attest_data.quote)
            .map_err(|e| VerificationError::PhalaQuoteVerify(format!("invalid quote hex: {e}")))?;

        get_collateral_and_verify(&quote_bytes, None)
            .await
            .map_err(|e| {
                VerificationError::PhalaQuoteVerify(format!("quote verification failed: {e}"))
            })?;

        Ok(())
    }

    /// Verify a Nillion HTX with automatic retry for operational failures.
    ///
    /// Only retries errors classified as "inconclusive" (network/operational failures).
    /// Cryptographic verification failures are not retried.
    ///
    /// # Arguments
    /// * `htx` - The Nillion HTX to verify
    /// * `config` - Retry configuration
    pub async fn verify_nillion_htx_with_retry(
        &self,
        htx: &NillionHtx,
        config: RetryConfig,
    ) -> Result<(), VerificationError> {
        retry_with_classifier(
            config,
            "verify_nillion_htx",
            || async { self.verify_nillion_htx(htx).await },
            |err| err.is_inconclusive(),
        )
        .await
    }

    /// Verify a Phala HTX with automatic retry for operational failures.
    ///
    /// Only retries errors classified as "inconclusive" (network/operational failures).
    /// Cryptographic verification failures are not retried.
    ///
    /// # Arguments
    /// * `htx` - The Phala HTX to verify
    /// * `config` - Retry configuration
    pub async fn verify_phala_htx_with_retry(
        &self,
        htx: &PhalaHtx,
        config: RetryConfig,
    ) -> Result<(), VerificationError> {
        retry_with_classifier(
            config,
            "verify_phala_htx",
            || async { self.verify_phala_htx(htx).await },
            |err| err.is_inconclusive(),
        )
        .await
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

        let err = VerificationError::PhalaComposeHashMismatch;
        assert!(err.message().contains("compose-hash mismatch"));

        let err = VerificationError::PhalaEventLogParse("parse error".to_string());
        assert!(err.message().contains("failed to parse event_log"));

        let err = VerificationError::PhalaQuoteVerify("quote error".to_string());
        assert!(err.message().contains("quote verification failed"));

        let err = VerificationError::PhalaPlatformVerify("platform error".to_string());
        assert!(err.message().contains("platform verification failed"));
    }

    #[test]
    fn test_inconclusive_errors() {
        // These are operational failures - don't indicate maliciousness
        let inconclusive_errors = vec![
            VerificationError::NilccUrl("network error".to_string()),
            VerificationError::NilccJson("parse error".to_string()),
            VerificationError::FetchReport("timeout".to_string()),
            VerificationError::BuilderUrl("connection refused".to_string()),
            VerificationError::BuilderJson("invalid json".to_string()),
            VerificationError::PhalaEventLogParse("missing field".to_string()),
            VerificationError::FetchCerts("AMD server unreachable".to_string()),
            VerificationError::DetectProcessor("unknown CPU".to_string()),
        ];

        for err in inconclusive_errors {
            assert_eq!(
                err.verdict(),
                Verdict::Inconclusive,
                "Expected {:?} to be Inconclusive",
                err
            );
            assert!(err.is_inconclusive());
            assert!(!err.is_failure());
        }
    }

    #[test]
    fn test_failure_errors() {
        // These are cryptographic failures - indicate potential tampering
        let failure_errors = vec![
            VerificationError::VerifyReport("signature invalid".to_string()),
            VerificationError::MeasurementHash("hash mismatch".to_string()),
            VerificationError::NotInBuilderIndex,
            VerificationError::PhalaComposeHashMismatch,
            VerificationError::PhalaQuoteVerify("quote failed".to_string()),
            VerificationError::PhalaPlatformVerify("platform invalid".to_string()),
            VerificationError::CertVerification("cert chain invalid".to_string()),
            VerificationError::InvalidMeasurement("expected X got Y".to_string()),
        ];

        for err in failure_errors {
            assert_eq!(
                err.verdict(),
                Verdict::Failure,
                "Expected {:?} to be Failure",
                err
            );
            assert!(err.is_failure());
            assert!(!err.is_inconclusive());
        }
    }
}
