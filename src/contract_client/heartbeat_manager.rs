use crate::{
    config::consts::DEFAULT_LOOKBACK_BLOCKS,
    contract_client::{
        common::tx_submitter::TransactionSubmitter,
        heartbeat_manager::HearbeatManager::HearbeatManagerInstance,
    },
    types::Htx,
};
use alloy::{
    contract::{CallBuilder, CallDecoder},
    primitives::{keccak256, Address, B256, U256},
    providers::Provider,
    sol,
    sol_types::SolValue,
};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::contract_client::common::errors::decode_any_error;
use crate::contract_client::common::event_helper::{
    listen_events, listen_events_filtered, BlockRange,
};
use anyhow::{anyhow, bail, Context, Result};

sol! {
    interface ISlashingPolicy {
        enum Outcome { Inconclusive, ValidThreshold, InvalidThreshold }
    }

    #[sol(rpc)]
    #[derive(Debug)]
    contract HearbeatManager {
        error ZeroAddress();
        error NotPending();
        error RoundClosed();
        error RoundAlreadyFinalized();
        error NotInCommittee();
        error ZeroStake();
        error BeforeDeadline();
        error AlreadyResponded();
        error InvalidVerdict();
        error CommitteeNotStarted();
        error InvalidRound();
        error EmptyCommittee();
        error InvalidSignature();
        error InvalidBatchSize();
        error RoundNotFinalized();
        error SnapshotBlockUnavailable(uint64 snapshotId);
        error RewardsAlreadyDone();
        error InvalidOutcome();
        error UnsortedVoters();
        error InvalidVoterInList();
        error InvalidVoterCount(uint256 got, uint256 expected);
        error InvalidVoterWeightSum(uint256 got, uint256 expected);
        error RawHTXHashMismatch();
        error InvalidCommitteeMember(address member);

        event HeartbeatEnqueued(bytes32 indexed heartbeatKey, bytes rawHTX, address indexed submitter);
        event RoundStarted(bytes32 indexed heartbeatKey, uint8 round, bytes32 committeeRoot, uint64 snapshotId, uint64 startedAt, uint64 deadline, address[] members, bytes rawHTX);
        event OperatorVoted(bytes32 indexed heartbeatKey, uint8 round, address indexed operator, uint8 verdict, uint256 weight);

        function submitHeartbeat(bytes calldata rawHTX, uint64 snapshotId) external whenNotPaused nonReentrant returns (bytes32 heartbeatKey);
        function submitVerdict(bytes32 heartbeatKey, uint8 verdict, bytes32[] calldata memberProof);
        function getVotePacked(bytes32 heartbeatKey, uint8 round, address operator) external view returns (uint256);
        function nodeCount() external view returns (uint256);
        function getNodes() external view returns (address[] memory);
    }
}

pub type RoundStartedEvent = HearbeatManager::RoundStarted;
pub type OperatorVotedEvent = HearbeatManager::OperatorVoted;
pub type HeartbeatEnqueuedEvent = HearbeatManager::HeartbeatEnqueued;

/// WebSocket-based client for real-time event streaming and contract interaction with
/// HeartbeatManager
#[derive(Clone)]
pub struct HeartbeatManagerClient<P: Provider + Clone> {
    provider: P,
    contract: HearbeatManagerInstance<P>,
    submitter: TransactionSubmitter<HearbeatManager::HearbeatManagerErrors>,
}

impl<P: Provider + Clone> HeartbeatManagerClient<P> {
    /// Create a new WebSocket client from ContractConfig
    pub fn new(provider: P, config: super::ContractConfig, tx_lock: Arc<Mutex<()>>) -> Self {
        let contract =
            HearbeatManagerInstance::new(config.manager_contract_address, provider.clone());
        let submitter = TransactionSubmitter::new(tx_lock);
        Self {
            provider,
            contract,
            submitter,
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
    pub async fn submit_htx(&self, htx: &Htx) -> Result<B256> {
        let snapshot_id = self.contract.provider().get_block_number().await?;
        let snapshot_id = snapshot_id.saturating_sub(1);
        let raw_htx = alloy::primitives::Bytes::try_from(htx)?;
        let call = self.contract.submitHeartbeat(raw_htx, snapshot_id);
        let gas_with_buffer = Self::overestimate_gas(&call).await?;
        self.submitter
            .with_gas_limit(gas_with_buffer)
            .invoke("submitHeartbeat", call)
            .await
    }

    /// Respond to an HTX assignment (called by assigned node)
    pub async fn respond_htx(
        &self,
        event: RoundStartedEvent,
        verdict: Verdict,
        submitter_address: Address,
    ) -> Result<B256> {
        let proofs =
            Self::compute_merkle_proof(*self.contract.address(), &event, submitter_address)?;
        let verdict = match verdict {
            Verdict::Success => 1,
            Verdict::Failure => 2,
            Verdict::Inconclusive => 3,
        };
        let call = self
            .contract
            .submitVerdict(event.heartbeatKey, verdict, proofs);
        let gas_with_buffer = Self::overestimate_gas(&call).await?;
        self.submitter
            .with_gas_limit(gas_with_buffer)
            .invoke("submitVerdict", call)
            .await
    }

    /// Check if a specific node has responded to an HTX
    pub async fn get_node_vote(
        &self,
        workload_key: B256,
        node: Address,
    ) -> Result<Option<Verdict>> {
        let vote = self
            .contract
            .getVotePacked(workload_key, 0, node)
            .call()
            .await?;
        match u8::try_from(vote).context("invalid vote")? {
            0 => Ok(None),
            1 => Ok(Some(Verdict::Success)),
            2 => Ok(Some(Verdict::Failure)),
            3 => Ok(Some(Verdict::Inconclusive)),
            other => bail!("invalid vote: {other}"),
        }
    }

    // ------------------------------------------------------------------------
    // Real-time Event Streaming
    // ------------------------------------------------------------------------

    /// Start listening for HTX assigned events and process them with a callback
    pub async fn listen_htx_assigned_events<F, Fut>(self: Arc<Self>, callback: F) -> Result<()>
    where
        F: FnMut(RoundStartedEvent) -> Fut + Send,
        Fut: std::future::Future<Output = Result<()>> + Send,
    {
        let subscription = self
            .contract
            .event_filter::<RoundStartedEvent>()
            .subscribe()
            .await?;
        listen_events(subscription.into_stream(), "RoundStarted", callback).await
    }

    /// Start listening for HTX assigned events for a specific node
    pub async fn listen_htx_assigned_for_node<F, Fut>(
        self: Arc<Self>,
        node_address: Address,
        callback: F,
    ) -> Result<()>
    where
        F: FnMut(RoundStartedEvent) -> Fut + Send,
        Fut: std::future::Future<Output = Result<()>> + Send,
    {
        let subscription = self
            .contract
            .event_filter::<RoundStartedEvent>()
            .subscribe()
            .await?;
        listen_events_filtered(
            subscription.into_stream(),
            "RoundStarted",
            move |event: &RoundStartedEvent| event.members.contains(&node_address),
            callback,
        )
        .await
    }

    /// Start listening for HTX submitted events
    pub async fn listen_htx_submitted_events<F, Fut>(self: Arc<Self>, callback: F) -> Result<()>
    where
        F: FnMut(HeartbeatEnqueuedEvent) -> Fut + Send,
        Fut: std::future::Future<Output = Result<()>> + Send,
    {
        let subscription = self
            .contract
            .event_filter::<HeartbeatEnqueuedEvent>()
            .subscribe()
            .await?;
        listen_events(subscription.into_stream(), "WorkloadEnqueued", callback).await
    }

    /// Start listening for HTX responded events
    pub async fn listen_htx_responded_events<F, Fut>(self: Arc<Self>, callback: F) -> Result<()>
    where
        F: FnMut(OperatorVotedEvent) -> Fut + Send,
        Fut: std::future::Future<Output = Result<()>> + Send,
    {
        let subscription = self
            .contract
            .event_filter::<OperatorVotedEvent>()
            .subscribe()
            .await?;
        listen_events(subscription.into_stream(), "OperatorVoted", callback).await
    }

    // ------------------------------------------------------------------------
    // Historical Event Query Methods
    // ------------------------------------------------------------------------

    /// Get HTX submitted events from recent history (default: last 1000 blocks)
    /// Use get_htx_submitted_events_with_lookback for custom lookback
    pub async fn get_htx_submitted_events(&self) -> Result<Vec<HeartbeatEnqueuedEvent>> {
        self.get_htx_submitted_events_with_lookback(DEFAULT_LOOKBACK_BLOCKS)
            .await
    }

    /// Get HTX submitted events with custom block lookback limit
    /// Set lookback to u64::MAX to search entire history
    pub async fn get_htx_submitted_events_with_lookback(
        &self,
        lookback_blocks: u64,
    ) -> Result<Vec<HeartbeatEnqueuedEvent>> {
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
    ) -> Result<Vec<HeartbeatEnqueuedEvent>> {
        let mut filter = self
            .contract
            .event_filter::<HeartbeatEnqueuedEvent>()
            .from_block(range.from_block);

        if let Some(to_block) = range.to_block {
            filter = filter.to_block(to_block);
        }

        let events = filter.query().await?;
        Ok(events.into_iter().map(|(event, _log)| event).collect())
    }

    /// Get HTX assigned events from recent history (default: last 1000 blocks)
    /// Use get_htx_assigned_events_with_lookback for custom lookback
    pub async fn get_htx_assigned_events(&self) -> Result<Vec<RoundStartedEvent>> {
        self.get_htx_assigned_events_with_lookback(DEFAULT_LOOKBACK_BLOCKS)
            .await
    }

    /// Get HTX assigned events with custom block lookback limit
    /// Set lookback to u64::MAX to search entire history
    pub async fn get_htx_assigned_events_with_lookback(
        &self,
        lookback_blocks: u64,
    ) -> Result<Vec<RoundStartedEvent>> {
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
    ) -> Result<Vec<RoundStartedEvent>> {
        let mut filter = self
            .contract
            .event_filter::<RoundStartedEvent>()
            .from_block(range.from_block);

        if let Some(to_block) = range.to_block {
            filter = filter.to_block(to_block);
        }

        let events = filter.query().await?;
        Ok(events.into_iter().map(|(event, _log)| event).collect())
    }

    /// Get HTX responded events from recent history (default: last 1000 blocks)
    /// Use get_htx_responded_events_with_lookback for custom lookback
    pub async fn get_htx_responded_events(&self) -> Result<Vec<OperatorVotedEvent>> {
        self.get_htx_responded_events_with_lookback(DEFAULT_LOOKBACK_BLOCKS)
            .await
    }

    /// Get HTX responded events with custom block lookback limit
    /// Set lookback to u64::MAX to search entire history
    pub async fn get_htx_responded_events_with_lookback(
        &self,
        lookback_blocks: u64,
    ) -> Result<Vec<OperatorVotedEvent>> {
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
    ) -> Result<Vec<OperatorVotedEvent>> {
        let mut filter = self
            .contract
            .event_filter::<OperatorVotedEvent>()
            .from_block(range.from_block);

        if let Some(to_block) = range.to_block {
            filter = filter.to_block(to_block);
        }

        let events = filter.query().await?;
        Ok(events.into_iter().map(|(event, _log)| event).collect())
    }

    fn compute_leaf(
        contract_address: Address,
        heartbeat_key: B256,
        round: u8,
        address: Address,
    ) -> B256 {
        let encoded =
            ([0xa1_u8], contract_address, heartbeat_key, [round], address).abi_encode_packed();
        keccak256(encoded)
    }

    fn compute_merkle_proof(
        contract_address: Address,
        event: &RoundStartedEvent,
        my_address: Address,
    ) -> anyhow::Result<Vec<B256>> {
        let sorted_members = event.members.clone();

        // Generate leaves
        let leaves: Vec<_> = sorted_members
            .iter()
            .map(|&member| {
                Self::compute_leaf(contract_address, event.heartbeatKey, event.round, member)
            })
            .collect();

        let mut target_index = sorted_members
            .iter()
            .position(|&member| member == my_address)
            .ok_or_else(|| anyhow!("Could not find our address in members"))?;

        // 3. Build Merkle proof (bottom-up, siblings from leaf to root)
        let mut proof: Vec<B256> = Vec::new();
        let mut current_layer = leaves;

        while current_layer.len() > 1 {
            let mut next_layer = Vec::new();

            for i in (0..current_layer.len()).step_by(2) {
                let left = current_layer[i];
                let right = if i + 1 < current_layer.len() {
                    current_layer[i + 1]
                } else {
                    // Duplicate last node when odd length (matches Solidity)
                    left
                };

                // Check if our target is in this pair
                let is_left = i == target_index;
                let is_right = i + 1 < current_layer.len() && i + 1 == target_index;

                if is_left || is_right {
                    let sibling = if is_left { right } else { left };
                    proof.push(sibling);

                    // Move target up to parent index
                    target_index = i / 2;
                }

                // Commutative hash: sort pair before hashing
                let parent = hash_pair(left, right);
                next_layer.push(parent);
            }

            current_layer = next_layer;
        }

        Ok(proof)
    }

    async fn overestimate_gas<D: CallDecoder>(call: &CallBuilder<&P, D>) -> anyhow::Result<u64> {
        // Estimate gas and add a 50% buffer
        let estimated_gas = call.estimate_gas().await.map_err(|e| {
            let decoded = decode_any_error(&e);
            anyhow!("failed to estimate gas: {decoded}")
        })?;
        let gas_with_buffer = estimated_gas.saturating_add(estimated_gas / 2);
        Ok(gas_with_buffer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Success,
    Failure,
    Inconclusive,
}

fn hash_pair(a: B256, b: B256) -> B256 {
    let (first, second) = if a < b { (a, b) } else { (b, a) };

    let encoded = (first, second).abi_encode_packed();
    keccak256(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::providers::DynProvider;

    #[test]
    fn leaf() {
        let contract_address = "0x3dbe95e20b370c5295e7436e2d887cfda8bcb02c"
            .parse()
            .unwrap();
        let member = "0xF3A6D9F493b30E0560f555f27adB143bE6b16309"
            .parse()
            .unwrap();
        let heartbeat_key = "0xbb93579fba8c311f05bc9accbc18f421d0b0c4912f7992534bf1e1a9fed70801"
            .parse()
            .unwrap();
        let leaf = HeartbeatManagerClient::<DynProvider>::compute_leaf(
            contract_address,
            heartbeat_key,
            1,
            member,
        );
        let expected: B256 = "0xcab48c8675419b700c85d1998d622e0d3c6eb61c2a92eaa7898ccbac25302c46"
            .parse()
            .unwrap();
        assert_eq!(leaf, expected);
    }
}
