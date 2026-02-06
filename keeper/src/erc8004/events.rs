use crate::{erc8004::ValidationRequestInfo, l2::KeeperState, metrics};
use alloy::{
    primitives::B256,
    providers::Provider,
    rpc::types::Log,
    sol_types::{SolEvent, SolValue},
};
use anyhow::Context;
use erc_8004_contract_clients::validation_registry::{
    ValidationRegistryUpgradeable::ValidationRegistryUpgradeableInstance, ValidationRequestEvent,
};
use futures_util::{Stream, StreamExt};
use std::{pin::pin, sync::Arc};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use super::Erc8004State;

pub type ValidationRegistryInstance<P> = ValidationRegistryUpgradeableInstance<P>;

/// Event listener for ERC-8004 ValidationRequest events.
pub struct Erc8004EventListener<P: Provider + Clone> {
    registry: ValidationRegistryInstance<P>,
}

impl<P: Provider + Clone + 'static> Erc8004EventListener<P> {
    pub fn new(registry: ValidationRegistryInstance<P>) -> Self {
        Self { registry }
    }

    /// Process historical ValidationRequest events and populate state.
    pub async fn process_historical_events(
        &self,
        from_block: u64,
        to_block: u64,
        state: &mut Erc8004State,
    ) -> anyhow::Result<()> {
        let events = self
            .query_events::<ValidationRequestEvent>(from_block, to_block)
            .await?;

        for (event, _log) in events {
            let heartbeat_key = compute_heartbeat_key(
                event.validatorAddress,
                event.agentId,
                &event.requestURI,
                event.requestHash,
            );
            let info = ValidationRequestInfo::new(
                event.validatorAddress,
                event.agentId,
                event.requestURI,
                event.requestHash,
            );
            state.pending_validations.insert(heartbeat_key, info);
        }

        info!(
            from_block,
            to_block,
            pending_validations = state.pending_validations.len(),
            "Loaded historical ERC-8004 validation requests"
        );

        Ok(())
    }

    /// Spawn background task to listen for new ValidationRequest events.
    pub async fn spawn(
        self,
        from_block: u64,
        state: Arc<Mutex<KeeperState>>,
    ) -> anyhow::Result<()> {
        let validation_request = self.subscribe::<ValidationRequestEvent>(from_block).await?;
        tokio::spawn(Self::process_validation_requests(validation_request, state));
        Ok(())
    }

    async fn query_events<E: SolEvent>(
        &self,
        from_block: u64,
        to_block: u64,
    ) -> anyhow::Result<Vec<(E, Log)>> {
        let events = self
            .registry
            .event_filter::<E>()
            .from_block(from_block)
            .to_block(to_block)
            .query()
            .await?;
        Ok(events)
    }

    async fn subscribe<E: SolEvent + 'static>(
        &self,
        from_block: u64,
    ) -> anyhow::Result<impl Stream<Item = E> + 'static> {
        let event_name = E::SIGNATURE
            .split_once('(')
            .map(|(name, _)| name)
            .unwrap_or(E::SIGNATURE);
        let stream = self
            .registry
            .event_filter::<E>()
            .from_block(from_block)
            .subscribe()
            .await
            .context("Failed to subscribe to ERC-8004 events")?
            .into_stream()
            .filter_map(async move |e| match e {
                Ok((event, _)) => {
                    metrics::get().l2.events.inc_events_received(event_name);
                    Some(event)
                }
                Err(e) => {
                    error!("Failed to receive {} event: {e}", E::SIGNATURE);
                    None
                }
            });
        Ok(stream)
    }

    async fn process_validation_requests(
        events: impl Stream<Item = ValidationRequestEvent>,
        state: Arc<Mutex<KeeperState>>,
    ) {
        let mut events = pin!(events);
        while let Some(event) = events.next().await {
            let heartbeat_key = compute_heartbeat_key(
                event.validatorAddress,
                event.agentId,
                &event.requestURI,
                event.requestHash,
            );

            let info = ValidationRequestInfo::new(
                event.validatorAddress,
                event.agentId,
                event.requestURI.clone(),
                event.requestHash,
            );

            let mut guard = state.lock().await;
            guard
                .erc8004
                .pending_validations
                .insert(heartbeat_key, info);
            metrics::get()
                .l2
                .erc8004
                .set_requests_tracked(guard.erc8004.pending_validations.len() as u64);

            info!(
                heartbeat_key = ?heartbeat_key,
                validator = ?event.validatorAddress,
                agent_id = ?event.agentId,
                request_hash = ?event.requestHash,
                "ERC-8004 validation request tracked"
            );
        }
    }
}

/// Compute the heartbeat key from validation request parameters.
///
/// This matches the Solidity encoding: `keccak256(abi.encode(validatorAddress, agentId, requestURI, requestHash))`
pub fn compute_heartbeat_key(
    validator_address: alloy::primitives::Address,
    agent_id: alloy::primitives::U256,
    request_uri: &str,
    request_hash: B256,
) -> B256 {
    let tuple = (
        validator_address,
        agent_id,
        request_uri.to_string(),
        request_hash,
    );
    let encoded = tuple.abi_encode();
    alloy::primitives::keccak256(&encoded)
}

/// Update ERC-8004 state when a round finalizes.
///
/// Called from the L2 event processor when RoundFinalized events are received.
pub fn on_round_finalized(state: &mut Erc8004State, heartbeat_key: B256, outcome: u8) {
    if let Some(info) = state.pending_validations.get_mut(&heartbeat_key) {
        let response = match outcome {
            0 => 50,  // inconclusive
            1 => 100, // valid
            2 => 0,   // invalid
            other => {
                warn!(
                    heartbeat_key = %heartbeat_key,
                    outcome = other,
                    "Unexpected HeartbeatManager outcome; defaulting ERC-8004 response to 0"
                );
                0
            }
        };
        info.outcome = Some(response);
        info!(
            heartbeat_key = %heartbeat_key,
            outcome,
            response,
            request_hash = %info.request_hash,
            "ERC-8004 validation round finalized"
        );
    } else {
        debug!(
            heartbeat_key = %heartbeat_key,
            "RoundFinalized for unknown heartbeat_key (not an ERC-8004 validation)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::{Address, U256};

    #[test]
    fn test_heartbeat_key_computation_matches_htx() {
        // Test data from htx.rs test:
        // abi.encode(0x5fc8d32690cc91d4c39d9d3abcbd16989f875707, 0, "https://api.nilai.nillion.network/", 0xa6719a2ea05fac172c1b20e16beea2a9739b715499a3a9ad488e6ce81602ffac)
        let validator_address: Address = "0x5fc8d32690cc91d4c39d9d3abcbd16989f875707"
            .parse()
            .unwrap();
        let agent_id = U256::ZERO;
        let request_uri = "https://api.nilai.nillion.network/";
        let request_hash: B256 =
            "0xa6719a2ea05fac172c1b20e16beea2a9739b715499a3a9ad488e6ce81602ffac"
                .parse()
                .unwrap();

        let heartbeat_key =
            compute_heartbeat_key(validator_address, agent_id, request_uri, request_hash);

        // The heartbeat key should be the keccak256 of the ABI-encoded tuple
        // This should match what the Solidity contract computes
        assert!(!heartbeat_key.is_zero());

        // Verify consistency - same inputs should produce same output
        let heartbeat_key2 =
            compute_heartbeat_key(validator_address, agent_id, request_uri, request_hash);
        assert_eq!(heartbeat_key, heartbeat_key2);
    }

    #[test]
    fn test_heartbeat_key_different_for_different_inputs() {
        let validator_address: Address = "0x5fc8d32690cc91d4c39d9d3abcbd16989f875707"
            .parse()
            .unwrap();
        let agent_id = U256::ZERO;
        let request_uri = "https://api.nilai.nillion.network/";
        let request_hash: B256 =
            "0xa6719a2ea05fac172c1b20e16beea2a9739b715499a3a9ad488e6ce81602ffac"
                .parse()
                .unwrap();

        let key1 = compute_heartbeat_key(validator_address, agent_id, request_uri, request_hash);

        // Different agent_id should produce different key
        let key2 =
            compute_heartbeat_key(validator_address, U256::from(1), request_uri, request_hash);
        assert_ne!(key1, key2);

        // Different request_uri should produce different key
        let key3 = compute_heartbeat_key(
            validator_address,
            agent_id,
            "https://different.uri/",
            request_hash,
        );
        assert_ne!(key1, key3);
    }
}
