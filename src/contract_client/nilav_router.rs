use crate::config::consts::DEFAULT_LOOKBACK_BLOCKS;
use crate::types::Htx;
use ethers::{
    contract::abigen,
    core::types::{Address, H256, U256},
};

use std::sync::Arc;
use tracing::error;

use ethers::providers::{Middleware, StreamExt};

use crate::contract_client::SignedWsProvider;

// Generate type-safe contract bindings from ABI
abigen!(
    NilAVRouter,
    "./contracts/out/NilAVRouter.sol/NilAVRouter.json",
    event_derives(serde::Deserialize, serde::Serialize)
);

/// Assignment struct for backwards compatibility with old NilAVRouter contract
/// The new contract uses a different structure with multiple nodes.
#[derive(Debug, Clone)]
pub struct Assignment {
    pub node: Address,
    pub responded: bool,
    pub result: bool,
}

/// WebSocket-based client for real-time event streaming and contract interaction with NilAVRouter
#[derive(Clone)]
pub struct NilAVRouterClient {
    provider: Arc<SignedWsProvider>,
    contract: NilAVRouter<SignedWsProvider>,
}

impl NilAVRouterClient {
    /// Create a new WebSocket client from ContractConfig
    pub fn new(provider: Arc<SignedWsProvider>, config: super::ContractConfig) -> Self {
        let contract_address = config.router_contract_address;
        let contract = NilAVRouter::new(contract_address, provider.clone());
        Self { provider, contract }
    }

    /// Get the contract address
    pub fn address(&self) -> Address {
        self.contract.address()
    }

    // ------------------------------------------------------------------------
    // Event Monitoring
    // ------------------------------------------------------------------------

    /// Get the current block number
    pub async fn get_block_number(&self) -> anyhow::Result<u64> {
        let block_number = self.provider.get_block_number().await?;
        Ok(block_number.as_u64())
    }

    /// Get the starting block for event queries based on lookback limit
    /// Returns max(0, current_block - lookback_blocks)
    async fn get_from_block(&self, lookback_blocks: u64) -> anyhow::Result<u64> {
        let current_block = self.get_block_number().await?;
        Ok(current_block.saturating_sub(lookback_blocks))
    }

    // ------------------------------------------------------------------------
    // Node Management (delegates to StakingOperators contract)
    // ------------------------------------------------------------------------

    /// Get the total number of active nodes
    pub async fn node_count(&self) -> anyhow::Result<U256> {
        Ok(self.contract.node_count().call().await?)
    }

    /// Get the list of all active node addresses
    pub async fn get_nodes(&self) -> anyhow::Result<Vec<Address>> {
        Ok(self.contract.get_nodes().call().await?)
    }

    // ------------------------------------------------------------------------
    // HTX Submission and Verification
    // ------------------------------------------------------------------------

    /// Submit an HTX for verification
    pub async fn submit_htx(&self, htx: &Htx) -> anyhow::Result<(H256, H256)> {
        let call = self.contract.submit_htx(htx.try_into()?);
        let tx = call.send().await?;
        let receipt = tx.await?;
        let receipt = receipt.ok_or_else(|| anyhow::anyhow!("No transaction receipt"))?;

        // Extract htxId from logs
        let htx_id = if let Some(log) = receipt.logs.first() {
            log.topics
                .get(1)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("No htxId in logs"))?
        } else {
            return Err(anyhow::anyhow!("No logs in receipt"));
        };

        Ok((receipt.transaction_hash, htx_id))
    }

    /// Respond to an HTX assignment (called by assigned node)
    pub async fn respond_htx(&self, htx_id: H256, result: bool) -> anyhow::Result<H256> {
        let htx_id_bytes: [u8; 32] = htx_id.into();
        let call = self.contract.respond_htx(htx_id_bytes, result);
        let tx = call.send().await.map_err(|e| {
            // Try to decode error message from revert data
            let error_msg = e.to_string();

            // Try different patterns for finding revert data
            let revert_data = error_msg
                .split("reverted with data: ")
                .nth(1)
                .or_else(|| error_msg.split("revert data: ").nth(1))
                .or_else(|| {
                    // Look for hex data starting with 0x08c379a0 (Error(string) selector)
                    error_msg.find("0x08c379a0").map(|start| {
                        // Find the end of the hex string (stop at first non-hex char after 0x)
                        let remaining = &error_msg[start..];
                        let end = remaining
                            .char_indices()
                            .skip(2) // Skip "0x"
                            .find(|(_, c)| !c.is_ascii_hexdigit())
                            .map(|(i, _)| i)
                            .unwrap_or(remaining.len());
                        &error_msg[start..start + end]
                    })
                });

            if let Some(data) = revert_data {
                if let Some(decoded) = super::decode_error_string(data.trim()) {
                    return anyhow::anyhow!("Contract call reverted: {}", decoded);
                }
            }

            e.into()
        })?;
        let receipt = tx.await?;
        let receipt = receipt.ok_or_else(|| anyhow::anyhow!("No transaction receipt"))?;
        Ok(receipt.transaction_hash)
    }

    /// Get assignment details for an HTX
    pub async fn get_assigned_nodes(&self, htx_id: H256) -> anyhow::Result<Vec<Address>> {
        let htx_id_bytes: [u8; 32] = htx_id.into();
        Ok(self
            .contract
            .get_assigned_nodes(htx_id_bytes)
            .call()
            .await?)
    }

    /// Get assignment info for an HTX
    pub async fn get_assignment_info(
        &self,
        htx_id: H256,
    ) -> anyhow::Result<(Vec<Address>, U256, U256, U256)> {
        let htx_id_bytes: [u8; 32] = htx_id.into();
        Ok(self
            .contract
            .get_assignment_info(htx_id_bytes)
            .call()
            .await?)
    }

    /// Check if a specific node has responded to an HTX
    pub async fn has_node_responded(
        &self,
        htx_id: H256,
        node: Address,
    ) -> anyhow::Result<(bool, bool)> {
        let htx_id_bytes: [u8; 32] = htx_id.into();
        Ok(self
            .contract
            .has_node_responded(htx_id_bytes, node)
            .call()
            .await?)
    }

    /// Check if all assigned nodes have responded
    pub async fn all_nodes_responded(&self, htx_id: H256) -> anyhow::Result<bool> {
        let htx_id_bytes: [u8; 32] = htx_id.into();
        Ok(self
            .contract
            .all_nodes_responded(htx_id_bytes)
            .call()
            .await?)
    }

    /// Get HTX bytes from the original submission transaction call data
    /// Default lookback: 1000 blocks. Use get_htx_with_lookback for custom lookback.
    pub async fn get_htx(&self, htx_id: H256) -> anyhow::Result<Vec<u8>> {
        self.get_htx_with_lookback(htx_id, DEFAULT_LOOKBACK_BLOCKS)
            .await
    }

    /// Get HTX bytes with custom block lookback limit
    /// Set lookback to u64::MAX to search entire history
    pub async fn get_htx_with_lookback(
        &self,
        htx_id: H256,
        lookback_blocks: u64,
    ) -> anyhow::Result<Vec<u8>> {
        let from_block = if lookback_blocks == u64::MAX {
            0
        } else {
            self.get_from_block(lookback_blocks).await?
        };

        // Find the transaction that submitted this HTX by querying the HTXSubmitted event
        let event_stream = self
            .contract
            .event::<HtxsubmittedFilter>()
            .topic1(htx_id) // Filter by htxId
            .from_block(from_block);

        let events = event_stream.query_with_meta().await?;

        let (_event, meta) = events
            .first()
            .ok_or_else(|| anyhow::anyhow!("No HTXSubmitted event found for htxId"))?;

        // Get the transaction hash from the event metadata
        let tx_hash = meta.transaction_hash;

        // Fetch the transaction
        let tx = self
            .provider
            .get_transaction(tx_hash)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Transaction not found"))?;

        // Decode the call data to extract the rawHTX parameter
        // The call data format is: 4-byte function selector + ABI-encoded parameters
        let input = tx.input;

        // Skip the function selector (first 4 bytes)
        if input.len() <= 4 {
            return Err(anyhow::anyhow!("Invalid call data"));
        }

        // Decode the bytes parameter (offset, length, data)
        use ethers::abi::{decode, ParamType};
        let decoded = decode(&[ParamType::Bytes], &input[4..])?;

        let htx_bytes = decoded[0]
            .clone()
            .into_bytes()
            .ok_or_else(|| anyhow::anyhow!("Failed to decode HTX bytes"))?;

        Ok(htx_bytes)
    }

    // ------------------------------------------------------------------------
    // Real-time Event Streaming
    // ------------------------------------------------------------------------

    /// Start listening for HTX assigned events and process them with a callback
    pub async fn listen_htx_assigned_events<F, Fut>(
        self: Arc<Self>,
        mut callback: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(HtxassignedFilter) -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        let event_stream = self.contract.event::<HtxassignedFilter>();

        let mut events = event_stream.subscribe().await?;

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok(event) => {
                    if let Err(e) = callback(event).await {
                        error!("Error processing HTX assigned event: {}", e);
                    }
                }
                Err(e) => {
                    error!("Error receiving HTX assigned event: {}", e);
                }
            }
        }
        Ok(())
    }

    /// Start listening for HTX assigned events for a specific node
    pub async fn listen_htx_assigned_for_node<F, Fut>(
        self: Arc<Self>,
        node_address: Address,
        mut callback: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(HtxassignedFilter) -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        let event_stream = self.contract.event::<HtxassignedFilter>();

        let mut events = event_stream.subscribe().await?;

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok(event) if event.node == node_address => {
                    if let Err(e) = callback(event).await {
                        error!("Error processing HTX assigned event: {}", e);
                    }
                }
                Ok(_) => {
                    // Event for different node, ignore
                }
                Err(e) => {
                    error!("Error receiving HTX assigned event: {}", e);
                }
            }
        }
        Ok(())
    }

    /// Start listening for HTX submitted events
    pub async fn listen_htx_submitted_events<F, Fut>(
        self: Arc<Self>,
        mut callback: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(HtxsubmittedFilter) -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        let event_stream = self.contract.event::<HtxsubmittedFilter>();
        let mut events = event_stream.subscribe().await?;

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok(event) => {
                    if let Err(e) = callback(event).await {
                        error!("Error processing HTX submitted event: {}", e);
                    }
                }
                Err(e) => {
                    error!("Error receiving HTX submitted event: {}", e);
                }
            }
        }
        Ok(())
    }

    /// Start listening for HTX responded events
    pub async fn listen_htx_responded_events<F, Fut>(
        self: Arc<Self>,
        mut callback: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut(HtxrespondedFilter) -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        let event_stream = self.contract.event::<HtxrespondedFilter>();

        let mut events = event_stream.subscribe().await?;

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok(event) => {
                    if let Err(e) = callback(event).await {
                        error!("Error processing HTX responded event: {}", e);
                    }
                }
                Err(e) => {
                    error!("Error receiving HTX responded event: {}", e);
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------------
    // Historical Event Query Methods
    // ------------------------------------------------------------------------

    /// Get HTX submitted events from recent history (default: last 1000 blocks)
    /// Use get_htx_submitted_events_with_lookback for custom lookback
    pub async fn get_htx_submitted_events(&self) -> anyhow::Result<Vec<HtxsubmittedFilter>> {
        self.get_htx_submitted_events_with_lookback(DEFAULT_LOOKBACK_BLOCKS)
            .await
    }

    /// Get HTX submitted events with custom block lookback limit
    /// Set lookback to u64::MAX to search entire history
    pub async fn get_htx_submitted_events_with_lookback(
        &self,
        lookback_blocks: u64,
    ) -> anyhow::Result<Vec<HtxsubmittedFilter>> {
        let from_block = if lookback_blocks == u64::MAX {
            0
        } else {
            self.get_from_block(lookback_blocks).await?
        };

        let events = self
            .contract
            .event::<HtxsubmittedFilter>()
            .from_block(from_block)
            .query()
            .await?;
        Ok(events)
    }

    /// Get HTX assigned events from recent history (default: last 1000 blocks)
    /// Use get_htx_assigned_events_with_lookback for custom lookback
    pub async fn get_htx_assigned_events(&self) -> anyhow::Result<Vec<HtxassignedFilter>> {
        self.get_htx_assigned_events_with_lookback(DEFAULT_LOOKBACK_BLOCKS)
            .await
    }

    /// Get HTX assigned events with custom block lookback limit
    /// Set lookback to u64::MAX to search entire history
    pub async fn get_htx_assigned_events_with_lookback(
        &self,
        lookback_blocks: u64,
    ) -> anyhow::Result<Vec<HtxassignedFilter>> {
        let from_block = if lookback_blocks == u64::MAX {
            0
        } else {
            self.get_from_block(lookback_blocks).await?
        };

        let events = self
            .contract
            .event::<HtxassignedFilter>()
            .from_block(from_block)
            .query()
            .await?;
        Ok(events)
    }

    /// Get HTX responded events from recent history (default: last 1000 blocks)
    /// Use get_htx_responded_events_with_lookback for custom lookback
    pub async fn get_htx_responded_events(&self) -> anyhow::Result<Vec<HtxrespondedFilter>> {
        self.get_htx_responded_events_with_lookback(DEFAULT_LOOKBACK_BLOCKS)
            .await
    }

    /// Get HTX responded events with custom block lookback limit
    /// Set lookback to u64::MAX to search entire history
    pub async fn get_htx_responded_events_with_lookback(
        &self,
        lookback_blocks: u64,
    ) -> anyhow::Result<Vec<HtxrespondedFilter>> {
        let from_block = if lookback_blocks == u64::MAX {
            0
        } else {
            self.get_from_block(lookback_blocks).await?
        };

        let events = self
            .contract
            .event::<HtxrespondedFilter>()
            .from_block(from_block)
            .query()
            .await?;
        Ok(events)
    }

    // ------------------------------------------------------------------------
    // Deprecated Event Query Methods (use StakingOperatorsClient)
    // ------------------------------------------------------------------------

    /// Get node registered events (DEPRECATED - use StakingOperatorsClient)
    /// Node registration events moved to StakingOperators contract.
    #[deprecated(note = "Use StakingOperatorsClient.get_operator_registered_events() instead")]
    pub async fn get_node_registered_events(&self) -> anyhow::Result<Vec<NodeRegisteredFilter>> {
        Err(anyhow::anyhow!(
            "NodeRegistered events are no longer emitted by NilAVRouter. Use StakingOperatorsClient instead."
        ))
    }

    /// Get node registered events with lookback (DEPRECATED - use StakingOperatorsClient)
    #[deprecated(
        note = "Use StakingOperatorsClient.get_operator_registered_events_with_lookback() instead"
    )]
    pub async fn get_node_registered_events_with_lookback(
        &self,
        _lookback_blocks: u64,
    ) -> anyhow::Result<Vec<NodeRegisteredFilter>> {
        Err(anyhow::anyhow!(
            "NodeRegistered events are no longer emitted by NilAVRouter. Use StakingOperatorsClient instead."
        ))
    }

    /// Get node deregistered events (DEPRECATED - use StakingOperatorsClient)
    /// Node deregistration events moved to StakingOperators contract.
    #[deprecated(note = "Use StakingOperatorsClient.get_operator_deactivated_events() instead")]
    pub async fn get_node_deregistered_events(
        &self,
    ) -> anyhow::Result<Vec<NodeDeregisteredFilter>> {
        Err(anyhow::anyhow!(
            "NodeDeregistered events are no longer emitted by NilAVRouter. Use StakingOperatorsClient instead."
        ))
    }

    /// Get node deregistered events with lookback (DEPRECATED - use StakingOperatorsClient)
    #[deprecated(
        note = "Use StakingOperatorsClient.get_operator_deactivated_events_with_lookback() instead"
    )]
    pub async fn get_node_deregistered_events_with_lookback(
        &self,
        _lookback_blocks: u64,
    ) -> anyhow::Result<Vec<NodeDeregisteredFilter>> {
        Err(anyhow::anyhow!(
            "NodeDeregistered events are no longer emitted by NilAVRouter. Use StakingOperatorsClient instead."
        ))
    }
}

// Define deprecated event filter types for backwards compatibility
#[derive(Debug, Clone)]
pub struct NodeRegisteredFilter {
    pub node: Address,
}

#[derive(Debug, Clone)]
pub struct NodeDeregisteredFilter {
    pub node: Address,
}
