use alloy::primitives::{Address, B256};
use anyhow::Result;
use clap::Parser;
use nilav::{
    config::{
        consts::{INITIAL_RECONNECT_DELAY_SECS, MAX_RECONNECT_DELAY_SECS},
        validate_node_requirements, NodeCliArgs, NodeConfig,
    },
    contract_client::{ContractConfig, NilAVClient},
    types::Htx,
    verification::HtxVerifier,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tracing::{debug, error, info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

// ============================================================================
// Signal Handling
// ============================================================================

/// Setup shutdown signal handler (Ctrl+C / SIGTERM)
async fn setup_shutdown_handler(shutdown_notify: Arc<Notify>) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigterm =
            signal(SignalKind::terminate()).expect("Failed to register SIGTERM handler");
        let mut sigint =
            signal(SignalKind::interrupt()).expect("Failed to register SIGINT handler");

        tokio::select! {
            _ = sigterm.recv() => {
                info!("Shutdown signal received (SIGTERM)");
            }
            _ = sigint.recv() => {
                info!("Shutdown signal received (SIGINT/Ctrl+C)");
            }
        }

        shutdown_notify.notify_waiters();
    }

    #[cfg(not(unix))]
    {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                info!("Shutdown signal received (Ctrl+C)");
                shutdown_notify.notify_waiters();
            }
            Err(err) => {
                error!(error = %err, "Failed to listen for shutdown signal");
            }
        }
    }
}

// ============================================================================
// HTX Processing
// ============================================================================

/// Process a single HTX assignment - verifies and submits result
async fn process_htx_assignment(
    client: Arc<NilAVClient>,
    htx_id: B256,
    verifier: &HtxVerifier,
) -> Result<()> {
    // Retrieve the HTX data from the contract
    let htx_bytes = client.router.get_htx(htx_id).await.map_err(|e| {
        error!(htx_id = ?htx_id, error = %e, "Failed to get HTX data");
        e
    })?;

    // Parse the HTX data
    let htx: Htx = match serde_json::from_slice(&htx_bytes) {
        Ok(h) => h,
        Err(e) => {
            error!(htx_id = ?htx_id, error = %e, "Failed to parse HTX data");
            // Respond with false if we can't parse the data
            client.router.respond_htx(htx_id, false).await?;
            warn!(htx_id = ?htx_id, "HTX not verified (parse error) | tx: submitted");
            return Ok(());
        }
    };

    // Verify the HTX
    let verification_result = verifier.verify_htx(&htx).await;
    let result = verification_result.is_ok();

    if let Err(ref e) = verification_result {
        warn!(htx_id = ?htx_id, error = %e, "HTX verification failed");
    }

    // Submit the verification result
    match client.router.respond_htx(htx_id, result).await {
        Ok(tx_hash) => {
            let verdict = if result { "VALID" } else { "INVALID" };
            info!(htx_id = ?htx_id, tx_hash = ?tx_hash, verdict = %verdict, "HTX verified");
            Ok(())
        }
        Err(e) => {
            error!(htx_id = ?htx_id, error = %e, "Failed to respond to HTX");
            Err(e)
        }
    }
}

/// Process backlog of historical assignments
async fn process_assignment_backlog(
    client: Arc<NilAVClient>,
    node_address: Address,
    verifier: &HtxVerifier,
) -> Result<()> {
    info!("Checking for pending assignments from before connection");

    let assigned_events = client.router.get_htx_assigned_events().await?;
    let pending: Vec<_> = assigned_events
        .iter()
        .filter(|e| e.node == node_address)
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
        let htx_id = event.htxId;

        // Check if already responded
        match client.router.has_node_responded(htx_id, node_address).await {
            Ok((responded, _result)) if responded => {
                debug!(htx_id = ?htx_id, "Already responded HTX, skipping");
            }
            Ok(_) => {
                info!(htx_id = ?htx_id, "Processing pending HTX");
                let client_clone = client.clone();
                let verifier = verifier.clone();
                tokio::spawn(async move {
                    if let Err(e) = process_htx_assignment(client_clone, htx_id, &verifier).await {
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

// ============================================================================
// Node Registration
// ============================================================================

/// Register node with the contract if not already registered
async fn register_node_if_needed(client: &NilAVClient, node_address: Address) -> Result<()> {
    info!(node_address = %node_address, "Checking node registration");

    let is_registered = client.staking.is_active_operator(node_address).await?;

    if is_registered {
        info!("Node already registered");
        return Ok(());
    }

    info!("Registering node with contract");
    let tx_hash = client.staking.register_operator("".to_string()).await?;
    info!(tx_hash = ?tx_hash, "Node registered successfully");

    Ok(())
}

// ============================================================================
// Client Creation
// ============================================================================

/// Create a WebSocket client with exponential backoff retry logic
async fn create_client_with_retry(
    config: &NodeConfig,
    shutdown_notify: &Arc<Notify>,
) -> Result<NilAVClient> {
    let mut reconnect_delay = Duration::from_secs(INITIAL_RECONNECT_DELAY_SECS);
    let max_reconnect_delay = Duration::from_secs(MAX_RECONNECT_DELAY_SECS);

    let contract_config = ContractConfig::new(
        config.rpc_url.clone(),
        config.router_contract_address,
        config.staking_contract_address,
        config.token_contract_address,
    );

    loop {
        let client_result =
            NilAVClient::new(contract_config.clone(), config.private_key.clone()).await;

        match client_result {
            Ok(client) => {
                let balance = client.get_balance().await?;
                info!(balance = ?balance, "WebSocket connection established");
                return Ok(client);
            }
            Err(e) => {
                error!(error = %e, reconnect_delay = ?reconnect_delay, "Failed to connect WebSocket. Retrying...");

                // Sleep with ability to be interrupted by shutdown
                tokio::select! {
                    _ = tokio::time::sleep(reconnect_delay) => {
                        reconnect_delay = std::cmp::min(reconnect_delay * 2, max_reconnect_delay);
                    }
                    _ = shutdown_notify.notified() => {
                        return Err(anyhow::anyhow!("Shutdown signal received during connection retry"));
                    }
                }
            }
        }
    }
}

// ============================================================================
// Event Listening
// ============================================================================

/// Listen for HTX assignment events and process them
async fn run_event_listener(
    client: Arc<NilAVClient>,
    node_address: Address,
    shutdown_notify: Arc<Notify>,
    verifier: &HtxVerifier,
) -> Result<()> {
    let client_for_callback = client.clone();

    let router_arc = Arc::new(client.router.clone());
    let listen_future = router_arc.listen_htx_assigned_for_node(node_address, move |event| {
        let client = client_for_callback.clone();

        async move {
            let htx_id = event.htxId;
            let node_addr = client.signer_address();
            let verifier = verifier.clone();
            tokio::spawn(async move {
                // Check if already responded
                match client.router.has_node_responded(htx_id, node_addr).await {
                    Ok((responded, _result)) if responded => (),
                    Ok(_) => {
                        info!(htx_id = ?htx_id, "Processing HTX");
                        if let Err(e) = process_htx_assignment(client, htx_id, &verifier).await {
                            error!(htx_id = ?htx_id, error = %e, "Failed to process real-time HTX");
                        }
                    }
                    Err(e) => {
                        error!(htx_id = ?htx_id, error = %e, "Failed to get assignment for HTX");
                    }
                }
            });

            Ok(())
        }
    });

    // Listen for either events or shutdown signal
    tokio::select! {
        result = listen_future => {
            result?;
            Ok(())
        },
        _ = shutdown_notify.notified() => {
            info!("Shutdown signal received during event listening");
            Err(anyhow::anyhow!("Shutdown requested"))
        }
    }
}

// ============================================================================
// Shutdown
// ============================================================================

/// Deactivate node from contract on shutdown
async fn deactivate_node_on_shutdown(
    config: &NodeConfig,
    node_address: Option<Address>,
) -> Result<()> {
    info!("Initiating graceful shutdown");

    let Some(addr) = node_address else {
        warn!("Node was never registered, skipping deactivation");
        return Ok(());
    };

    info!(node_address = %addr, "Deactivating node from contract");

    let contract_config = ContractConfig::new(
        config.rpc_url.clone(),
        config.router_contract_address,
        config.staking_contract_address,
        config.token_contract_address,
    );

    let client = NilAVClient::new(contract_config, config.private_key.clone()).await?;
    let tx_hash = client.staking.deactivate_operator().await?;
    info!(tx_hash = ?tx_hash, "Node deactivated successfully");

    Ok(())
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(fmt::layer().with_ansi(true))
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    // Load configuration
    let cli_args = NodeCliArgs::parse();
    let verifier = HtxVerifier::new(cli_args.artifact_cache.clone(), cli_args.cert_cache.clone())?;
    let config = NodeConfig::load(cli_args).await?;

    // Create initial client to validate requirements
    let contract_config = ContractConfig::new(
        config.rpc_url.clone(),
        config.router_contract_address,
        config.staking_contract_address,
        config.token_contract_address,
    );
    let validation_client = NilAVClient::new(contract_config, config.private_key.clone()).await?;

    // Validate node has sufficient ETH and staked TEST tokens
    validate_node_requirements(
        &validation_client,
        &config.rpc_url,
        config.was_wallet_created,
    )
    .await?;

    info!("Node initialized");
    info!("Press Ctrl+C to gracefully shutdown and deactivate");

    // Setup graceful shutdown handler
    let shutdown_notify = Arc::new(Notify::new());
    let shutdown_notify_clone = shutdown_notify.clone();
    tokio::spawn(async move {
        setup_shutdown_handler(shutdown_notify_clone).await;
    });

    // Main reconnection loop
    let mut node_address: Option<Address> = None;
    let mut reconnect_delay = Duration::from_secs(INITIAL_RECONNECT_DELAY_SECS);
    let max_reconnect_delay = Duration::from_secs(MAX_RECONNECT_DELAY_SECS);

    loop {
        info!("Starting WebSocket event listener with auto-reconnection");

        // Create client with retry logic
        let client = match create_client_with_retry(&config, &shutdown_notify).await {
            Ok(client) => client,
            Err(_) => break, // Shutdown requested or unrecoverable error
        };

        let current_address = client.signer_address();
        node_address = Some(current_address);

        // Register node if needed
        if let Err(e) = register_node_if_needed(&client, current_address).await {
            error!(error = %e, reconnect_delay = ?reconnect_delay, "Failed to register node. Retrying...");

            // Exit the loop
            std::process::exit(1);
        }

        let client_arc = Arc::new(client);

        // Process any backlog of assignments
        if let Err(e) =
            process_assignment_backlog(client_arc.clone(), current_address, &verifier).await
        {
            error!(error = %e, "Failed to query historical assignments");
        }

        // Start listening for events
        match run_event_listener(
            client_arc,
            current_address,
            shutdown_notify.clone(),
            &verifier,
        )
        .await
        {
            Ok(_) => {
                warn!(reconnect_delay = ?reconnect_delay, "WebSocket listener exited normally. Reconnecting...");
            }
            Err(e) if e.to_string().contains("Shutdown") => {
                break; // Graceful shutdown
            }
            Err(e) => {
                error!(error = %e, reconnect_delay = ?reconnect_delay, "WebSocket listener error. Reconnecting...");
            }
        }

        // Sleep before reconnecting, with ability to be interrupted by shutdown
        tokio::select! {
            _ = tokio::time::sleep(reconnect_delay) => {
                reconnect_delay = std::cmp::min(reconnect_delay * 2, max_reconnect_delay);
            }
            _ = shutdown_notify.notified() => {
                break; // Shutdown requested
            }
        }
    }

    // Graceful shutdown - deactivate node from contract
    if let Err(e) = deactivate_node_on_shutdown(&config, node_address).await {
        error!(error = %e, "Failed to deactivate node gracefully");
    }

    info!("Shutdown complete");
    Ok(())
}
