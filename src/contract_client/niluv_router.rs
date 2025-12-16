use crate::{config::consts::DEFAULT_LOOKBACK_BLOCKS, types::VersionedHtx};
use alloy::{
    consensus::Transaction,
    dyn_abi::DynSolType,
    primitives::{Address, B256, U256},
    providers::Provider,
    sol,
};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::contract_client::common::errors::decode_any_error;
use crate::contract_client::common::event_helper::{
    listen_events, listen_events_filtered, BlockRange,
};
use crate::contract_client::common::tx_helper::{send_and_confirm, send_with_gas_and_confirm};
use anyhow::{anyhow, Result};

sol!(
    #[sol(rpc)]
    #[derive(Debug)]
    NilAVRouter,
    "./contracts/out/NilAVRouter.sol/NilAVRouter.json"
);

// Type aliases for event types to match old ethers naming convention
pub type HTXAssignedFilter = NilAVRouter::HTXAssigned;
pub type HTXRespondedFilter = NilAVRouter::HTXResponded;
pub type HTXSubmittedFilter = NilAVRouter::HTXSubmitted;

/// WebSocket-based client for real-time event streaming and contract interaction with NilUVRouter
#[derive(Clone)]
pub struct NilUVRouterClient<P: Provider + Clone> {
    provider: P,
    contract: NilAVRouter::NilAVRouterInstance<P>,
    tx_lock: Arc<Mutex<()>>,
}

impl<P: Provider + Clone> NilUVRouterClient<P> {
    /// Create a new WebSocket client from ContractConfig
    pub fn new(provider: P, config: super::ContractConfig, tx_lock: Arc<Mutex<()>>) -> Self {
        let contract =
            NilAVRouter::NilAVRouterInstance::new(config.router_contract_address, provider.clone());
        Self {
            provider,
            contract,
            tx_lock,
        }
    }

    /// Get the contract address
    pub fn address(&self) -> Address {
        *self.contract.address()
    }

    // ------------------------------------------------------------------------
    // Event Monitoring
    // ------------------------------------------------------------------------

    /// Get the current block number
    pub async fn get_block_number(&self) -> Result<u64> {
        Ok(self.provider.get_block_number().await?)
    }

    /// Get the starting block for event queries based on lookback limit
    /// Returns max(0, current_block - lookback_blocks)
    async fn get_from_block(&self, lookback_blocks: u64) -> Result<u64> {
        let current_block = self.get_block_number().await?;
        Ok(current_block.saturating_sub(lookback_blocks))
    }

    // ------------------------------------------------------------------------
    // Node Management (delegates to StakingOperators contract)
    // ------------------------------------------------------------------------

    /// Get the total number of active nodes
    pub async fn node_count(&self) -> Result<U256> {
        Ok(self.contract.nodeCount().call().await?)
    }

    /// Get the list of all active node addresses
    pub async fn get_nodes(&self) -> Result<Vec<Address>> {
        Ok(self.contract.getNodes().call().await?)
    }

    // ------------------------------------------------------------------------
    // HTX Submission and Verification
    // ------------------------------------------------------------------------

    /// Submit an HTX for verification
    pub async fn submit_htx(&self, htx: &VersionedHtx) -> Result<B256> {
        let raw_htx: alloy::primitives::Bytes = htx.try_into()?;
        let call = self.contract.submitHTX(raw_htx);

        // Estimate gas and add 50% buffer for variable node selection
        let estimated_gas = call.estimate_gas().await.map_err(|e| {
            let decoded = decode_any_error(&e);
            anyhow!("submitHTX would revert: {}", decoded)
        })?;
        let gas_with_buffer = estimated_gas.saturating_add(estimated_gas / 2);

        send_with_gas_and_confirm(call, &self.tx_lock, "submitHTX", gas_with_buffer).await
    }

    /// Respond to an HTX assignment (called by assigned node)
    pub async fn respond_htx(&self, htx_id: B256, result: bool) -> Result<B256> {
        let call = self.contract.respondHTX(htx_id, result);
        send_and_confirm(call, &self.tx_lock, "respondHTX").await
    }

    /// Check if a specific node has responded to an HTX
    pub async fn has_node_responded(&self, htx_id: B256, node: Address) -> Result<(bool, bool)> {
        Ok(self
            .contract
            .hasNodeResponded(htx_id, node)
            .call()
            .await?
            .into())
    }

    /// Get HTX bytes from the original submission transaction call data
    /// Default lookback: 1000 blocks. Use get_htx_with_lookback for custom lookback.
    pub async fn get_htx(&self, htx_id: B256) -> Result<Vec<u8>> {
        self.get_htx_with_lookback(htx_id, DEFAULT_LOOKBACK_BLOCKS)
            .await
    }

    /// Get HTX bytes with custom block lookback limit
    /// Set lookback to u64::MAX to search entire history
    pub async fn get_htx_with_lookback(
        &self,
        htx_id: B256,
        lookback_blocks: u64,
    ) -> Result<Vec<u8>> {
        let from_block = if lookback_blocks == u64::MAX {
            0
        } else {
            self.get_from_block(lookback_blocks).await?
        };

        // Find the transaction that submitted this HTX by querying the HTXSubmitted event
        let event_filter = self
            .contract
            .event_filter::<HTXSubmittedFilter>()
            .topic1(htx_id)
            .from_block(from_block);
        let events = event_filter.query().await?;

        let (_event, log) = events
            .first()
            .ok_or_else(|| anyhow!("No HTXSubmitted event found for htxId"))?;

        // Get the transaction hash from the event log
        let tx_hash = log
            .transaction_hash
            .ok_or_else(|| anyhow!("No transaction hash in log"))?;

        // Fetch the transaction
        let tx = self
            .provider
            .get_transaction_by_hash(tx_hash)
            .await?
            .ok_or_else(|| anyhow!("Transaction not found"))?;

        // Decode the call data to extract the rawHTX parameter
        // The call data format is: 4-byte function selector + ABI-encoded parameters
        let input = tx.inner.input();

        // Skip the function selector (first 4 bytes)
        if input.len() <= 4 {
            return Err(anyhow!("Invalid call data"));
        }

        // Decode the bytes parameter using DynSolType
        let decoded = DynSolType::Bytes.abi_decode(&input[4..])?;

        let htx_bytes = decoded
            .as_bytes()
            .ok_or_else(|| anyhow!("Failed to decode HTX bytes as bytes"))?
            .to_vec();

        Ok(htx_bytes)
    }

    // ------------------------------------------------------------------------
    // Real-time Event Streaming
    // ------------------------------------------------------------------------

    /// Start listening for HTX assigned events and process them with a callback
    pub async fn listen_htx_assigned_events<F, Fut>(self: Arc<Self>, callback: F) -> Result<()>
    where
        F: FnMut(HTXAssignedFilter) -> Fut + Send,
        Fut: std::future::Future<Output = Result<()>> + Send,
    {
        let subscription = self
            .contract
            .event_filter::<HTXAssignedFilter>()
            .subscribe()
            .await?;
        listen_events(subscription.into_stream(), "HTXAssigned", callback).await
    }

    /// Start listening for HTX assigned events for a specific node
    pub async fn listen_htx_assigned_for_node<F, Fut>(
        self: Arc<Self>,
        node_address: Address,
        callback: F,
    ) -> Result<()>
    where
        F: FnMut(HTXAssignedFilter) -> Fut + Send,
        Fut: std::future::Future<Output = Result<()>> + Send,
    {
        let subscription = self
            .contract
            .event_filter::<HTXAssignedFilter>()
            .subscribe()
            .await?;
        listen_events_filtered(
            subscription.into_stream(),
            "HTXAssigned",
            move |event: &HTXAssignedFilter| event.node == node_address,
            callback,
        )
        .await
    }

    /// Start listening for HTX submitted events
    pub async fn listen_htx_submitted_events<F, Fut>(self: Arc<Self>, callback: F) -> Result<()>
    where
        F: FnMut(HTXSubmittedFilter) -> Fut + Send,
        Fut: std::future::Future<Output = Result<()>> + Send,
    {
        let subscription = self
            .contract
            .event_filter::<HTXSubmittedFilter>()
            .subscribe()
            .await?;
        listen_events(subscription.into_stream(), "HTXSubmitted", callback).await
    }

    /// Start listening for HTX responded events
    pub async fn listen_htx_responded_events<F, Fut>(self: Arc<Self>, callback: F) -> Result<()>
    where
        F: FnMut(HTXRespondedFilter) -> Fut + Send,
        Fut: std::future::Future<Output = Result<()>> + Send,
    {
        let subscription = self
            .contract
            .event_filter::<HTXRespondedFilter>()
            .subscribe()
            .await?;
        listen_events(subscription.into_stream(), "HTXResponded", callback).await
    }

    // ------------------------------------------------------------------------
    // Historical Event Query Methods
    // ------------------------------------------------------------------------

    /// Get HTX submitted events from recent history (default: last 1000 blocks)
    /// Use get_htx_submitted_events_with_lookback for custom lookback
    pub async fn get_htx_submitted_events(&self) -> Result<Vec<HTXSubmittedFilter>> {
        self.get_htx_submitted_events_with_lookback(DEFAULT_LOOKBACK_BLOCKS)
            .await
    }

    /// Get HTX submitted events with custom block lookback limit
    /// Set lookback to u64::MAX to search entire history
    pub async fn get_htx_submitted_events_with_lookback(
        &self,
        lookback_blocks: u64,
    ) -> Result<Vec<HTXSubmittedFilter>> {
        let current_block = self.get_block_number().await?;
        let range = if lookback_blocks == u64::MAX {
            BlockRange::all()
        } else {
            BlockRange::from_lookback(current_block, lookback_blocks)
        };
        self.get_htx_submitted_events_in_range(range).await
    }

    /// Get HTX submitted events within a specific block range
    pub async fn get_htx_submitted_events_in_range(
        &self,
        range: BlockRange,
    ) -> Result<Vec<HTXSubmittedFilter>> {
        let mut filter = self
            .contract
            .event_filter::<HTXSubmittedFilter>()
            .from_block(range.from_block);

        if let Some(to_block) = range.to_block {
            filter = filter.to_block(to_block);
        }

        let events = filter.query().await?;
        Ok(events.into_iter().map(|(event, _log)| event).collect())
    }

    /// Get HTX assigned events from recent history (default: last 1000 blocks)
    /// Use get_htx_assigned_events_with_lookback for custom lookback
    pub async fn get_htx_assigned_events(&self) -> Result<Vec<HTXAssignedFilter>> {
        self.get_htx_assigned_events_with_lookback(DEFAULT_LOOKBACK_BLOCKS)
            .await
    }

    /// Get HTX assigned events with custom block lookback limit
    /// Set lookback to u64::MAX to search entire history
    pub async fn get_htx_assigned_events_with_lookback(
        &self,
        lookback_blocks: u64,
    ) -> Result<Vec<HTXAssignedFilter>> {
        let current_block = self.get_block_number().await?;
        let range = if lookback_blocks == u64::MAX {
            BlockRange::all()
        } else {
            BlockRange::from_lookback(current_block, lookback_blocks)
        };
        self.get_htx_assigned_events_in_range(range).await
    }

    /// Get HTX assigned events within a specific block range
    pub async fn get_htx_assigned_events_in_range(
        &self,
        range: BlockRange,
    ) -> Result<Vec<HTXAssignedFilter>> {
        let mut filter = self
            .contract
            .event_filter::<HTXAssignedFilter>()
            .from_block(range.from_block);

        if let Some(to_block) = range.to_block {
            filter = filter.to_block(to_block);
        }

        let events = filter.query().await?;
        Ok(events.into_iter().map(|(event, _log)| event).collect())
    }

    /// Get HTX responded events from recent history (default: last 1000 blocks)
    /// Use get_htx_responded_events_with_lookback for custom lookback
    pub async fn get_htx_responded_events(&self) -> Result<Vec<HTXRespondedFilter>> {
        self.get_htx_responded_events_with_lookback(DEFAULT_LOOKBACK_BLOCKS)
            .await
    }

    /// Get HTX responded events with custom block lookback limit
    /// Set lookback to u64::MAX to search entire history
    pub async fn get_htx_responded_events_with_lookback(
        &self,
        lookback_blocks: u64,
    ) -> Result<Vec<HTXRespondedFilter>> {
        let current_block = self.get_block_number().await?;
        let range = if lookback_blocks == u64::MAX {
            BlockRange::all()
        } else {
            BlockRange::from_lookback(current_block, lookback_blocks)
        };
        self.get_htx_responded_events_in_range(range).await
    }

    /// Get HTX responded events within a specific block range
    pub async fn get_htx_responded_events_in_range(
        &self,
        range: BlockRange,
    ) -> Result<Vec<HTXRespondedFilter>> {
        let mut filter = self
            .contract
            .event_filter::<HTXRespondedFilter>()
            .from_block(range.from_block);

        if let Some(to_block) = range.to_block {
            filter = filter.to_block(to_block);
        }

        let events = filter.query().await?;
        Ok(events.into_iter().map(|(event, _log)| event).collect())
    }
}
