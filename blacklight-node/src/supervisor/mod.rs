use alloy::primitives::Address;
use anyhow::{Result, bail};
use blacklight_contract_clients::{BlacklightClient, ContractConfig, StreamWatchdog};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::args::{EVENT_IDLE_TIMEOUT_JITTER_SECS, NodeConfig, validate_node_requirements};
use crate::verification::HtxVerifier;

use crate::supervisor::htx::HtxProcessor;
use crate::supervisor::version::validate_node_version;

mod events;
mod htx;
mod status;
mod version;

/// Initial reconnection delay
const INITIAL_RECONNECT_DELAY: Duration = Duration::from_secs(1);
/// Maximum reconnection delay
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);

/// How many times alloy's transport retries the socket before giving up.
///
/// Bounded rather than infinite because this supervisor does its own
/// reconnection. While alloy retries — on a flat 3s interval, with no backoff or
/// jitter — the failure is invisible here: the subscription stream stays open, so
/// nothing below can react. Letting the transport give up (~30s) closes the
/// stream, which hands control to the backoff and full client rebuild in `run`.
const MAX_WS_RETRIES: u32 = 10;

/// Node supervisor - manages WebSocket connection, reconnection, and event processing
pub struct Supervisor<'a> {
    config: &'a NodeConfig,
    verifier: &'a HtxVerifier,
    shutdown_token: CancellationToken,
    verified_counter: Arc<AtomicU64>,
    node_address: Address,
    reconnect_delay: Duration,
    client: BlacklightClient,
    /// Events delivered by the current subscription, used to tell a working
    /// stream from one that never delivered anything.
    events_seen: Arc<AtomicU64>,
}

impl<'a> Supervisor<'a> {
    /// Create a new supervisor, establishing the initial connection and validating requirements
    pub async fn new(
        config: &'a NodeConfig,
        verifier: &'a HtxVerifier,
        shutdown_token: CancellationToken,
    ) -> Result<Self> {
        let client = Self::create_client_with_retry(config, &shutdown_token).await?;
        let node_address = client.signer_address();

        // Validate node version against protocol requirement
        validate_node_version(&client).await?;

        // Validate node has sufficient ETH and staked NIL tokens
        validate_node_requirements(&client, &config.rpc_url, config.was_wallet_created).await?;

        info!(node_address = %node_address, "Node initialized");

        Ok(Self {
            config,
            verifier,
            shutdown_token,
            verified_counter: Arc::new(AtomicU64::new(0)),
            node_address,
            reconnect_delay: INITIAL_RECONNECT_DELAY,
            client,
            events_seen: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Build the liveness watchdog for the event subscription, if enabled.
    ///
    /// The timeout is jittered per node so that a network-wide lull does not
    /// make the whole fleet reconnect simultaneously.
    fn watchdog(&self) -> Option<StreamWatchdog> {
        let base_secs = self.config.event_idle_timeout_secs;
        if base_secs == 0 {
            warn!("Event stream watchdog disabled (EVENT_IDLE_TIMEOUT_SECS=0)");
            return None;
        }
        let timeout = Duration::from_secs(
            base_secs + jitter_secs(self.node_address, EVENT_IDLE_TIMEOUT_JITTER_SECS),
        );
        info!(
            idle_timeout_secs = timeout.as_secs(),
            "Event stream watchdog armed"
        );
        Some(StreamWatchdog::new(timeout, self.events_seen.clone()))
    }

    /// Run the supervisor loop until shutdown is requested
    pub async fn run(mut self) -> Result<()> {
        loop {
            info!("Starting WebSocket event listener with auto-reconnection");
            info!("Press Ctrl+C to gracefully shutdown and deactivate");

            // Use existing client or create a new one
            let client = self.client.clone();

            // Register node if needed
            if let Err(e) = self.register_node_if_needed(&client).await {
                error!(error = %e, "Failed to register node");
                std::process::exit(1);
            }

            // Process any backlog of assignments
            if let Err(e) = self.process_backlog(client.clone()).await {
                error!(error = %e, "Failed to query historical assignments");
            }

            // Start listening for events
            let events_before = self.events_seen.load(Ordering::Relaxed);
            let outcome = self.listen_for_events(client).await;
            let stream_was_healthy = self.events_seen.load(Ordering::Relaxed) > events_before;

            match outcome {
                Ok(_) => {
                    warn!("WebSocket listener exited normally. Reconnecting...");
                }
                Err(e) if e.to_string().contains("Shutdown") => {
                    break;
                }
                Err(e) => {
                    error!(error = %e, "WebSocket listener error. Reconnecting...");
                }
            }

            // Only a stream that actually delivered events counts as healthy.
            // Resetting on connection success instead would keep the backoff
            // pinned at its minimum whenever the RPC accepts the socket but
            // fails the subscription, turning recovery into a hot loop across
            // the whole fleet.
            if stream_was_healthy {
                self.reconnect_delay = INITIAL_RECONNECT_DELAY;
            }

            if self.reconnect_client().await? {
                break;
            }
        }

        Ok(())
    }

    /// Create a new WebSocket client
    pub(crate) async fn create_client(config: &NodeConfig) -> Result<BlacklightClient> {
        let contract_config = ContractConfig::new(
            config.rpc_url.clone(),
            config.manager_contract_address,
            config.staking_contract_address,
            config.token_contract_address,
        )
        .with_max_ws_retries(MAX_WS_RETRIES);
        BlacklightClient::new(contract_config, config.private_key.clone()).await
    }

    /// Create a client with retry/backoff. Returns Shutdown error if cancelled.
    async fn create_client_with_retry(
        config: &NodeConfig,
        shutdown_token: &CancellationToken,
    ) -> Result<BlacklightClient> {
        let mut reconnect_delay = INITIAL_RECONNECT_DELAY;
        loop {
            match Self::create_client(config).await {
                Ok(client) => return Ok(client),
                Err(e) => {
                    error!(error = %e, "Failed to create client. Retrying...");
                    let sleep = tokio::time::sleep(reconnect_delay);
                    tokio::select! {
                        _ = sleep => {
                            reconnect_delay = std::cmp::min(
                                reconnect_delay * 2,
                                MAX_RECONNECT_DELAY
                            );
                        }
                        _ = shutdown_token.cancelled() => {
                            bail!("Shutdown requested during initial connect");
                        }
                    }
                }
            }
        }
    }

    /// Register node with the contract if not already registered
    async fn register_node_if_needed(&self, client: &BlacklightClient) -> Result<()> {
        info!(node_address = %self.node_address, "Checking node registration");

        let is_registered = client.staking.is_active_operator(self.node_address).await?;

        if is_registered {
            info!("Node already registered");
            return Ok(());
        }

        info!("Registering node with contract");
        let tx_hash = client.staking.register_operator("".to_string()).await?;
        info!(tx_hash = ?tx_hash, "Node registered successfully");

        Ok(())
    }

    /// Process backlog of historical assignments
    async fn process_backlog(&self, client: BlacklightClient) -> Result<()> {
        self.build_htx_processor(client.clone())
            .process_assignment_backlog(client)
            .await
    }

    /// Listen for HTX assignment events
    async fn listen_for_events(&self, client: BlacklightClient) -> Result<()> {
        events::run_event_listener(
            client.clone(),
            self.build_htx_processor(client),
            self.watchdog(),
        )
        .await
    }

    fn build_htx_processor(&self, client: BlacklightClient) -> HtxProcessor {
        HtxProcessor::new(
            client,
            self.verifier.clone(),
            self.verified_counter.clone(),
            self.node_address,
            self.shutdown_token.clone(),
        )
    }

    /// Reconnect the client with retry/backoff. Returns true if shutdown was requested.
    ///
    /// Always waits before reconnecting. Creating the client can succeed while
    /// the subscription that follows still fails, so returning immediately on a
    /// successful connect would let the caller spin with no delay at all.
    async fn reconnect_client(&mut self) -> Result<bool> {
        loop {
            if self.wait_before_reconnect().await {
                return Ok(true);
            }
            match Self::create_client(self.config).await {
                Ok(client) => {
                    self.client = client;
                    return Ok(false);
                }
                Err(e) => {
                    error!(error = %e, "Failed to create client. Retrying...");
                }
            }
        }
    }

    /// Wait before reconnecting, returns true if shutdown was requested
    async fn wait_before_reconnect(&mut self) -> bool {
        tokio::select! {
            _ = tokio::time::sleep(self.reconnect_delay) => {
                self.reconnect_delay = std::cmp::min(
                    self.reconnect_delay * 2,
                    MAX_RECONNECT_DELAY
                );
                false
            }
            _ = self.shutdown_token.cancelled() => {
                true
            }
        }
    }
}

/// Deterministic per-node offset in `0..spread` seconds, derived from the node
/// address so it is stable across restarts but differs between operators.
fn jitter_secs(node_address: Address, spread: u64) -> u64 {
    if spread == 0 {
        return 0;
    }
    let bytes = node_address.into_array();
    let seed = u64::from_be_bytes(bytes[..8].try_into().expect("address is 20 bytes"));
    seed % spread
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(hex: &str) -> Address {
        hex.parse().expect("valid address")
    }

    #[test]
    fn jitter_is_within_spread() {
        for a in [
            "0x8413388033aC4F79c34285ad6f7b3684231A5c45",
            "0xe0d1e31C3c7cC3a2554ead6BCB8035eD6f69f6Ab",
            "0xc831594C6748D900B4FD9068705ee828d3e2DBAe",
        ] {
            assert!(jitter_secs(addr(a), 180) < 180);
        }
    }

    #[test]
    fn jitter_is_stable_for_the_same_address() {
        let a = addr("0x8413388033aC4F79c34285ad6f7b3684231A5c45");
        assert_eq!(jitter_secs(a, 180), jitter_secs(a, 180));
    }

    #[test]
    fn jitter_differs_between_nodes() {
        // Not a guarantee for arbitrary inputs, but it must hold for real
        // operator addresses or the fleet would still reconnect in lockstep.
        let a = jitter_secs(addr("0x8413388033aC4F79c34285ad6f7b3684231A5c45"), 180);
        let b = jitter_secs(addr("0xe0d1e31C3c7cC3a2554ead6BCB8035eD6f69f6Ab"), 180);
        assert_ne!(a, b);
    }

    #[test]
    fn zero_spread_disables_jitter() {
        assert_eq!(
            jitter_secs(addr("0x8413388033aC4F79c34285ad6f7b3684231A5c45"), 0),
            0
        );
    }
}
