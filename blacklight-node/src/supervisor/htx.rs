use alloy::primitives::Address;
use alloy::primitives::utils::format_ether;
use anyhow::Result;
use blacklight_contract_clients::{
    BlacklightClient,
    heartbeat_manager::{RoundStartedEvent, Verdict},
    htx::Htx,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::args::MIN_ETH_BALANCE;
use crate::verification::HtxVerifier;

use super::status::print_status;

/// Process a single HTX assignment - verifies and submits result
pub async fn process_htx_assignment(
    client: BlacklightClient,
    event: RoundStartedEvent,
    verifier: &HtxVerifier,
    verified_counter: Arc<AtomicU64>,
    shutdown_token: CancellationToken,
    node_address: Address,
) -> Result<()> {
    let htx_id = event.heartbeatKey;
    // Parse the HTX data - UnifiedHtx automatically detects provider field
    let verification_result = match serde_json::from_slice::<Htx>(&event.rawHTX) {
        Ok(htx) => match htx {
            Htx::Nillion(htx) => {
                info!(htx_id = ?htx_id, "Detected nilCC HTX");
                verifier.verify_nillion_htx(&htx).await
            }
            Htx::Phala(htx) => {
                info!(htx_id = ?htx_id, "Detected Phala HTX");
                verifier.verify_phala_htx(&htx).await
            }
        },
        Err(e) => {
            error!(htx_id = ?htx_id, error = %e, "Failed to parse HTX data");
            // If we parse invalid data, it could be a malicious node, so Failure and it doesn't get rewarded
            client
                .manager
                .respond_htx(event, Verdict::Failure, node_address)
                .await?;
            info!(htx_id = ?htx_id, "✅ HTX verification submitted");
            return Ok(());
        }
    };
    let verdict = match verification_result {
        Ok(_) => Verdict::Success,
        Err(ref e) => e.verdict(),
    };

    // Submit the verification result
    match client
        .manager
        .respond_htx(event, verdict, node_address)
        .await
    {
        Ok(tx_hash) => {
            let count = verified_counter.fetch_add(1, Ordering::SeqCst) + 1;

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

            if let Err(e) = print_status(&client, count).await {
                warn!(error = %e, "Failed to fetch status information");
            }

            // Check if balance is below minimum threshold
            match client.get_balance().await {
                Ok(balance) => {
                    if balance < MIN_ETH_BALANCE {
                        error!(
                            balance = %format_ether(balance),
                            min_required = %format_ether(MIN_ETH_BALANCE),
                            "⚠️ ETH balance below minimum threshold. Initiating shutdown..."
                        );
                        shutdown_token.cancel();
                        return Err(anyhow::anyhow!("Insufficient ETH balance"));
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Failed to check balance after transaction");
                }
            }

            Ok(())
        }
        Err(e) => {
            error!(htx_id = ?htx_id, error = %e, "Failed to respond to HTX");
            Err(e)
        }
    }
}

/// Process backlog of historical assignments
pub async fn process_assignment_backlog(
    client: BlacklightClient,
    node_address: Address,
    verifier: &HtxVerifier,
    verified_counter: Arc<AtomicU64>,
    shutdown_token: CancellationToken,
) -> Result<()> {
    info!("Checking for pending assignments from before connection");

    let assigned_events = client.manager.get_htx_assigned_events().await?;
    let pending: Vec<_> = assigned_events
        .into_iter()
        .filter(|e| e.members.contains(&node_address))
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
        let htx_id = event.heartbeatKey;

        // Check if already responded
        match client.manager.get_node_vote(htx_id, node_address).await {
            Ok(Some(_)) => {
                debug!(htx_id = ?htx_id, "Already responded HTX, skipping");
            }
            Ok(None) => {
                info!(htx_id = ?htx_id, "📥 HTX received (backlog)");
                let client_clone = client.clone();
                let verifier = verifier.clone();
                let counter = verified_counter.clone();
                let shutdown_clone = shutdown_token.clone();
                tokio::spawn(async move {
                    if let Err(e) = process_htx_assignment(
                        client_clone,
                        event,
                        &verifier,
                        counter,
                        shutdown_clone,
                        node_address,
                    )
                    .await
                    {
                        error!(htx_id = ?htx_id, error = %e, "Failed to process pending HTX");
                    }
                });
            }
            Err(e) => {
                error!(htx_id = ?htx_id, error = %e, "Failed to check assignment status");
            }
        }
    }

    info!("Backlog processing complete");
    Ok(())
}
