use crate::config::consts::DEFAULT_LOOKBACK_BLOCKS;
use crate::types::Htx;
use alloy::{
    consensus::Transaction,
    dyn_abi::DynSolType,
    primitives::{Address, B256, U256},
    providers::Provider,
    sol,
};
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::error;

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
pub struct NilAVRouterClient<P: Provider + Clone> {
    provider: P,
    contract: NilAVRouter::NilAVRouterInstance<P>,
    tx_lock: Arc<Mutex<()>>,
}

impl<P: Provider + Clone> NilAVRouterClient<P> {
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
    pub async fn get_block_number(&self) -> anyhow::Result<u64> {
        Ok(self.provider.get_block_number().await?)
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
        Ok(self.contract.nodeCount().call().await?)
    }

    /// Get the list of all active node addresses
    pub async fn get_nodes(&self) -> anyhow::Result<Vec<Address>> {
        Ok(self.contract.getNodes().call().await?)
    }

    // ------------------------------------------------------------------------
    // HTX Submission and Verification
    // ------------------------------------------------------------------------

    /// Submit an HTX for verification
    pub async fn submit_htx(&self, htx: &Htx) -> anyhow::Result<B256> {
        let raw_htx: alloy::primitives::Bytes = htx.try_into()?;
        let call = self.contract.submitHTX(raw_htx.clone());

        // First simulate the call to catch reverts with proper error messages
        if let Err(e) = call.call().await {
            let decoded = super::errors::decode_any_error(&e);
            return Err(anyhow::anyhow!("submitHTX reverted: {}", decoded));
        }

        // Estimate gas and add buffer for safety
        let estimated_gas = call.estimate_gas().await.map_err(|e| {
            let decoded = super::errors::decode_any_error(&e);
            anyhow::anyhow!("submitHTX would revert: {}", decoded)
        })?;
        let gas_with_buffer = estimated_gas.saturating_add(estimated_gas / 5); // +20% buffer for variable node selection
        let call_with_gas = call.gas(gas_with_buffer);
        let _guard = self.tx_lock.lock().await;
        let pending = call_with_gas.send().await.map_err(|e| {
            let decoded = super::errors::decode_any_error(&e);
            anyhow::anyhow!("submitHTX failed to send: {}", decoded)
        })?;

        let receipt = pending.get_receipt().await?;
        if !receipt.status() {
            // Check if it was an OutOfGas error by comparing gas used to gas limit
            let gas_used = receipt.gas_used;
            if gas_used >= gas_with_buffer {
                return Err(anyhow::anyhow!(
                    "submitHTX ran out of gas (used {} of {} limit). \
                    This can happen when many nodes are selected. Tx: {:?}",
                    gas_used,
                    gas_with_buffer,
                    receipt.transaction_hash
                ));
            }

            // Transaction was included but reverted - re-simulate at the SAME block to get the error
            // This is important because htxId = keccak256(rawHTXHash, sender, block.number)
            // so simulating at a different block would give a different htxId
            if let Some(block_number) = receipt.block_number {
                let retry_call = self.contract.submitHTX(raw_htx).block(block_number.into());
                if let Err(e) = retry_call.call().await {
                    let decoded = super::errors::decode_any_error(&e);
                    return Err(anyhow::anyhow!(
                        "submitHTX reverted: {}. Tx: {:?}",
                        decoded,
                        receipt.transaction_hash
                    ));
                }
                // Re-simulation succeeded - likely ran out of gas but gas check above didn't catch it
                return Err(anyhow::anyhow!(
                    "submitHTX failed on-chain (possible OutOfGas - re-simulation succeeded). Tx: {:?}",
                    receipt.transaction_hash
                ));
            }
            // No block number in receipt - shouldn't happen
            return Err(anyhow::anyhow!(
                "submitHTX reverted on-chain (no block number in receipt). Tx: {:?}",
                receipt.transaction_hash
            ));
        }

        Ok(receipt.transaction_hash)
    }

    /// Respond to an HTX assignment (called by assigned node)
    pub async fn respond_htx(&self, htx_id: B256, result: bool) -> anyhow::Result<B256> {
        let call = self.contract.respondHTX(htx_id, result);

        // First simulate the call to catch reverts with proper error messages
        if let Err(e) = call.call().await {
            let decoded = super::errors::decode_any_error(&e);
            return Err(anyhow::anyhow!("respondHTX reverted: {}", decoded));
        }

        // Estimate gas and add buffer for safety
        let estimated_gas = call.estimate_gas().await.map_err(|e| {
            let decoded = super::errors::decode_any_error(&e);
            anyhow::anyhow!("respondHTX would revert: {}", decoded)
        })?;
        let gas_with_buffer = estimated_gas.saturating_add(estimated_gas / 5); // +20%

        let call_with_gas = call.gas(gas_with_buffer);

        let _guard = self.tx_lock.lock().await;
        let pending = call_with_gas.send().await.map_err(|e| {
            let decoded = super::errors::decode_any_error(&e);
            anyhow::anyhow!("respondHTX failed to send: {}", decoded)
        })?;

        let receipt = pending.get_receipt().await?;
        if !receipt.status() {
            // Transaction was included but reverted - re-simulate at the SAME block to get the error
            if let Some(block_number) = receipt.block_number {
                let retry_call = self
                    .contract
                    .respondHTX(htx_id, result)
                    .block(block_number.into());
                if let Err(e) = retry_call.call().await {
                    let decoded = super::errors::decode_any_error(&e);
                    return Err(anyhow::anyhow!(
                        "respondHTX reverted: {}. Tx: {:?}",
                        decoded,
                        receipt.transaction_hash
                    ));
                }
                // Re-simulation succeeded
                return Err(anyhow::anyhow!(
                    "respondHTX reverted (re-simulation at block {} succeeded, state changed). Tx: {:?}",
                    block_number, receipt.transaction_hash
                ));
            }
            return Err(anyhow::anyhow!(
                "respondHTX reverted on-chain (no block number in receipt). Tx: {:?}",
                receipt.transaction_hash
            ));
        }

        Ok(receipt.transaction_hash)
    }

    /// Get assignment details for an HTX
    pub async fn get_assigned_nodes(&self, htx_id: B256) -> anyhow::Result<Vec<Address>> {
        Ok(self.contract.getAssignedNodes(htx_id).call().await?)
    }

    /// Get assignment info for an HTX
    pub async fn get_assignment_info(
        &self,
        htx_id: B256,
    ) -> anyhow::Result<(Vec<Address>, U256, U256, U256)> {
        Ok(self.contract.getAssignmentInfo(htx_id).call().await?.into())
    }

    /// Check if a specific node has responded to an HTX
    pub async fn has_node_responded(
        &self,
        htx_id: B256,
        node: Address,
    ) -> anyhow::Result<(bool, bool)> {
        Ok(self
            .contract
            .hasNodeResponded(htx_id, node)
            .call()
            .await?
            .into())
    }

    /// Check if all assigned nodes have responded
    pub async fn all_nodes_responded(&self, htx_id: B256) -> anyhow::Result<bool> {
        Ok(self.contract.allNodesResponded(htx_id).call().await?)
    }

    /// Get HTX bytes from the original submission transaction call data
    /// Default lookback: 1000 blocks. Use get_htx_with_lookback for custom lookback.
    pub async fn get_htx(&self, htx_id: B256) -> anyhow::Result<Vec<u8>> {
        self.get_htx_with_lookback(htx_id, DEFAULT_LOOKBACK_BLOCKS)
            .await
    }

    /// Get HTX bytes with custom block lookback limit
    /// Set lookback to u64::MAX to search entire history
    pub async fn get_htx_with_lookback(
        &self,
        htx_id: B256,
        lookback_blocks: u64,
    ) -> anyhow::Result<Vec<u8>> {
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
            .ok_or_else(|| anyhow::anyhow!("No HTXSubmitted event found for htxId"))?;

        // Get the transaction hash from the event log
        let tx_hash = log
            .transaction_hash
            .ok_or_else(|| anyhow::anyhow!("No transaction hash in log"))?;

        // Fetch the transaction
        let tx = self
            .provider
            .get_transaction_by_hash(tx_hash)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Transaction not found"))?;

        // Decode the call data to extract the rawHTX parameter
        // The call data format is: 4-byte function selector + ABI-encoded parameters
        let input = tx.inner.input();

        // Skip the function selector (first 4 bytes)
        if input.len() <= 4 {
            return Err(anyhow::anyhow!("Invalid call data"));
        }

        // Decode the bytes parameter using DynSolType
        let decoded = DynSolType::Bytes.abi_decode(&input[4..])?;

        let htx_bytes = decoded
            .as_bytes()
            .ok_or_else(|| anyhow::anyhow!("Failed to decode HTX bytes as bytes"))?
            .to_vec();

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
        F: FnMut(HTXAssignedFilter) -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        let event_stream = self.contract.event_filter::<HTXAssignedFilter>();
        let subscription = event_stream.subscribe().await?;
        let mut events = subscription.into_stream();

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok((event, _log)) => {
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
        F: FnMut(HTXAssignedFilter) -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        let event_stream = self.contract.event_filter::<HTXAssignedFilter>();
        let subscription = event_stream.subscribe().await?;
        let mut events = subscription.into_stream();

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok((event, _log)) if event.node == node_address => {
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
        F: FnMut(HTXSubmittedFilter) -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        let event_stream = self.contract.event_filter::<HTXSubmittedFilter>();
        let subscription = event_stream.subscribe().await?;
        let mut events = subscription.into_stream();

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok((event, _log)) => {
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
        F: FnMut(HTXRespondedFilter) -> Fut + Send,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send,
    {
        let event_stream = self.contract.event_filter::<HTXRespondedFilter>();
        let subscription = event_stream.subscribe().await?;
        let mut events = subscription.into_stream();

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok((event, _log)) => {
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
    pub async fn get_htx_submitted_events(&self) -> anyhow::Result<Vec<HTXSubmittedFilter>> {
        self.get_htx_submitted_events_with_lookback(DEFAULT_LOOKBACK_BLOCKS)
            .await
    }

    /// Get HTX submitted events with custom block lookback limit
    /// Set lookback to u64::MAX to search entire history
    pub async fn get_htx_submitted_events_with_lookback(
        &self,
        lookback_blocks: u64,
    ) -> anyhow::Result<Vec<HTXSubmittedFilter>> {
        let from_block = if lookback_blocks == u64::MAX {
            0
        } else {
            self.get_from_block(lookback_blocks).await?
        };

        let event_filter = self
            .contract
            .event_filter::<HTXSubmittedFilter>()
            .from_block(from_block);
        let events = event_filter.query().await?;
        Ok(events.into_iter().map(|(event, _log)| event).collect())
    }

    /// Get HTX assigned events from recent history (default: last 1000 blocks)
    /// Use get_htx_assigned_events_with_lookback for custom lookback
    pub async fn get_htx_assigned_events(&self) -> anyhow::Result<Vec<HTXAssignedFilter>> {
        self.get_htx_assigned_events_with_lookback(DEFAULT_LOOKBACK_BLOCKS)
            .await
    }

    /// Get HTX assigned events with custom block lookback limit
    /// Set lookback to u64::MAX to search entire history
    pub async fn get_htx_assigned_events_with_lookback(
        &self,
        lookback_blocks: u64,
    ) -> anyhow::Result<Vec<HTXAssignedFilter>> {
        let from_block = if lookback_blocks == u64::MAX {
            0
        } else {
            self.get_from_block(lookback_blocks).await?
        };

        let event_filter = self
            .contract
            .event_filter::<HTXAssignedFilter>()
            .from_block(from_block);
        let events = event_filter.query().await?;
        Ok(events.into_iter().map(|(event, _log)| event).collect())
    }

    /// Get HTX responded events from recent history (default: last 1000 blocks)
    /// Use get_htx_responded_events_with_lookback for custom lookback
    pub async fn get_htx_responded_events(&self) -> anyhow::Result<Vec<HTXRespondedFilter>> {
        self.get_htx_responded_events_with_lookback(DEFAULT_LOOKBACK_BLOCKS)
            .await
    }

    /// Get HTX responded events with custom block lookback limit
    /// Set lookback to u64::MAX to search entire history
    pub async fn get_htx_responded_events_with_lookback(
        &self,
        lookback_blocks: u64,
    ) -> anyhow::Result<Vec<HTXRespondedFilter>> {
        let from_block = if lookback_blocks == u64::MAX {
            0
        } else {
            self.get_from_block(lookback_blocks).await?
        };

        let event_filter = self
            .contract
            .event_filter::<HTXRespondedFilter>()
            .from_block(from_block);
        let events = event_filter.query().await?;
        Ok(events.into_iter().map(|(event, _log)| event).collect())
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
