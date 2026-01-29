use alloy::primitives::Address;
use anyhow::{Result, anyhow};
use blacklight_contract_clients::{BlacklightClient, ContractConfig};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::args::{NodeConfig, validate_node_requirements};
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

/// Node supervisor - manages WebSocket connection, reconnection, and event processing
pub struct Supervisor<'a> {
    config: &'a NodeConfig,
    verifier: &'a HtxVerifier,
    shutdown_token: CancellationToken,
    verified_counter: Arc<AtomicU64>,
    node_address: Address,
    reconnect_delay: Duration,
    client: BlacklightClient,
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
        })
    }

    /// Run the supervisor loop, returns the client for use in shutdown
    pub async fn run(mut self) -> Result<BlacklightClient> {
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
            match self.listen_for_events(client).await {
                Ok(_) => {
                    warn!("WebSocket listener exited normally. Reconnecting...");
                    if self.reconnect_client().await? {
                        break;
                    }
                }
                Err(e) if e.to_string().contains("Shutdown") => {
                    break;
                }
                Err(e) => {
                    error!(error = %e, "WebSocket listener error. Reconnecting...");
                    if self.reconnect_client().await? {
                        break;
                    }
                }
            }
        }

        Ok(self.client)
    }

    /// Create a new WebSocket client
    async fn create_client(config: &NodeConfig) -> Result<BlacklightClient> {
        let contract_config = ContractConfig::new(
            config.rpc_url.clone(),
            config.manager_contract_address,
            config.staking_contract_address,
            config.token_contract_address,
        );
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
                            return Err(anyhow!("Shutdown requested during initial connect"));
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
        events::run_event_listener(client.clone(), self.build_htx_processor(client)).await
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
    async fn reconnect_client(&mut self) -> Result<bool> {
        loop {
            match Self::create_client(self.config).await {
                Ok(client) => {
                    self.client = client;
                    self.reconnect_delay = INITIAL_RECONNECT_DELAY;
                    return Ok(false);
                }
                Err(e) => {
                    error!(error = %e, "Failed to create client. Retrying...");
                    if self.wait_before_reconnect().await {
                        return Ok(true);
                    }
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
