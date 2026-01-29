use alloy::primitives::Address;
use anyhow::Result;
use blacklight_contract_clients::{
    BlacklightClient,
    heartbeat_manager::{RoundStartedEvent, Verdict},
    htx::Htx,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::supervisor::status::{check_minimum_balance, print_status};
use crate::supervisor::version::validate_node_version;
use crate::verification::HtxVerifier;

#[derive(Clone)]
pub struct HtxProcessor {
    client: BlacklightClient,
    verifier: HtxVerifier,
    verified_counter: Arc<AtomicU64>,
    node_address: Address,
    shutdown_token: CancellationToken,
}

impl HtxProcessor {
    pub fn new(
        client: BlacklightClient,
        verifier: HtxVerifier,
        verified_counter: Arc<AtomicU64>,
        node_address: Address,
        shutdown_token: CancellationToken,
    ) -> Self {
        Self {
            client,
            verifier,
            verified_counter,
            node_address,
            shutdown_token,
        }
    }

    pub fn node_address(&self) -> Address {
        self.node_address
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown_token.clone()
    }

    pub fn spawn_processing(
        &self,
        event: RoundStartedEvent,
        vote_address: Address,
        source: HtxEventSource,
        run_post_process: bool,
    ) {
        let processor = self.clone();
        tokio::spawn(async move {
            let htx_id = event.heartbeatKey;

            let client = processor.client.clone();
            // Check if already responded
            match client.manager.get_node_vote(htx_id, vote_address).await {
                Ok(Some(_)) => {
                    if let Some(message) = source.already_responded_message() {
                        debug!(htx_id = ?htx_id, "{}", message);
                    }
                }
                Ok(None) => {
                    info!(htx_id = ?htx_id, "{}", source.received_message());
                    match processor.process_htx_assignment(client, event).await {
                        Ok(Some(count)) => {
                            if run_post_process
                                && let Err(e) = processor.post_process_checks(count).await
                            {
                                warn!(error = %e, "Failed to post-process events");
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            error!(htx_id = ?htx_id, error = %e, "{}", source.process_error_message());
                        }
                    }
                }
                Err(e) => {
                    error!(htx_id = ?htx_id, error = %e, "{}", source.vote_error_message());
                }
            }
        });
    }

    /// Process a single HTX assignment - verifies and submits result
    pub async fn process_htx_assignment(
        &self,
        client: BlacklightClient,
        event: RoundStartedEvent,
    ) -> Result<Option<u64>> {
        let htx_id = event.heartbeatKey;
        // Parse the HTX data - UnifiedHtx automatically detects provider field
        let verification_result = match serde_json::from_slice::<Htx>(&event.rawHTX) {
            Ok(htx) => match htx {
                Htx::Nillion(htx) => {
                    info!(htx_id = ?htx_id, "Detected nilCC HTX");
                    self.verifier.verify_nillion_htx(&htx).await
                }
                Htx::Phala(htx) => {
                    info!(htx_id = ?htx_id, "Detected Phala HTX");
                    self.verifier.verify_phala_htx(&htx).await
                }
            },
            Err(e) => {
                error!(htx_id = ?htx_id, error = %e, "Failed to parse HTX data");
                // If we parse invalid data, it could be a malicious node, so Failure and it doesn't get rewarded
                client
                    .manager
                    .respond_htx(event, Verdict::Failure, self.node_address)
                    .await?;
                info!(htx_id = ?htx_id, "✅ HTX verification submitted");
                return Ok(None);
            }
        };
        let verdict = match verification_result {
            Ok(_) => Verdict::Success,
            Err(ref e) => e.verdict(),
        };

        // Submit the verification result
        match client
            .manager
            .respond_htx(event, verdict, self.node_address)
            .await
        {
            Ok(tx_hash) => {
                let count = self.verified_counter.fetch_add(1, Ordering::SeqCst) + 1;

                match (verdict, verification_result) {
                    (Verdict::Success, Ok(_)) => {
                        info!(tx_hash=?tx_hash, "✅ VALID HTX verification submitted");
                    }
                    (Verdict::Failure, Err(e)) => {
                        info!(tx_hash=?tx_hash, error=?e, verdict="failure", "❌ INVALID HTX verification submitted");
                    }
                    (Verdict::Inconclusive, Err(e)) => {
                        info!(tx_hash=?tx_hash, error=?e, verdict="inconclusive", "⚠️ INCONCLUSIVE HTX verification submitted");
                    }
                    (_, _) => {
                        error!(tx_hash=?tx_hash, verdict=?verdict, "Unexpected verification state");
                    }
                }

                Ok(Some(count))
            }
            Err(e) => {
                error!(htx_id = ?htx_id, error = %e, "Failed to respond to HTX");
                Err(e)
            }
        }
    }

    /// Process backlog of historical assignments
    pub async fn process_assignment_backlog(&self, client: BlacklightClient) -> Result<()> {
        info!("Checking for pending assignments from before connection");

        let assigned_events = client.manager.get_htx_assigned_events().await?;
        let pending: Vec<_> = assigned_events
            .into_iter()
            .filter(|e| e.members.contains(&self.node_address))
            .collect();

        if pending.is_empty() {
            info!("No pending assignments found");
            return Ok(());
        }

        info!(
            count = pending.len(),
            "Found historical assignments, processing backlog"
        );

        for event in pending {
            self.spawn_processing(event, self.node_address, HtxEventSource::Backlog, false);
        }

        info!("Backlog processing complete");
        Ok(())
    }

    pub async fn post_process_checks(&self, verified_count: u64) -> Result<()> {
        let client = self.client.clone();
        let shutdown_token = self.shutdown_token.clone();
        if let Err(e) = print_status(&client, verified_count).await {
            warn!(error = %e, "Failed to fetch status information");
        }

        if let Err(e) = check_minimum_balance(&client, &shutdown_token).await {
            warn!(error = %e, "Failed to check minimum balance is above minimum threshold");
        }

        if let Err(e) = validate_node_version(&client).await {
            warn!(error = %e, "Failed to validate node version against protocol requirement");
        }

        Ok(())
    }
}

#[derive(Copy, Clone)]
pub enum HtxEventSource {
    Realtime,
    Backlog,
}

impl HtxEventSource {
    fn received_message(self) -> &'static str {
        match self {
            Self::Realtime => "📥 HTX received",
            Self::Backlog => "📥 HTX received (backlog)",
        }
    }

    fn already_responded_message(self) -> Option<&'static str> {
        match self {
            Self::Realtime => None,
            Self::Backlog => Some("Already responded HTX, skipping"),
        }
    }

    fn process_error_message(self) -> &'static str {
        match self {
            Self::Realtime => "Failed to process real-time HTX",
            Self::Backlog => "Failed to process pending HTX",
        }
    }

    fn vote_error_message(self) -> &'static str {
        match self {
            Self::Realtime => "Failed to get assignment for HTX",
            Self::Backlog => "Failed to check assignment status",
        }
    }
}
