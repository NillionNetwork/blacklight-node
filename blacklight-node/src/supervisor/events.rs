use anyhow::Result;
use blacklight_contract_clients::BlacklightClient;
use std::sync::Arc;
use tracing::info;

use super::htx::{HtxEventSource, HtxProcessor};
/// Listen for HTX assignment events and process them
pub async fn run_event_listener(client: BlacklightClient, processor: HtxProcessor) -> Result<()> {
    let client_for_callback = client.clone();
    let processor_for_callback = processor.clone();

    let manager = Arc::new(client.manager.clone());
    let node_address = processor.node_address();
    let listen_future = manager.listen_htx_assigned_for_node(node_address, move |event| {
        let client = client_for_callback.clone();
        let processor = processor_for_callback.clone();
        async move {
            let vote_address = client.signer_address();
            processor.spawn_processing(event, vote_address, HtxEventSource::Realtime, true);

            Ok(())
        }
    });

    // Listen for either events or shutdown signal
    let shutdown_token = processor.shutdown_token();
    tokio::select! {
        result = listen_future => {
            result?;
            Ok(())
        },
        _ = shutdown_token.cancelled() => {
            info!("Shutdown signal received during event listening");
            Err(anyhow::anyhow!("Shutdown requested"))
        }
    }
}
