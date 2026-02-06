use alloy::primitives::{Address, B256, U256};
use std::collections::HashMap;

pub mod events;
pub mod responder;

/// State tracking for ERC-8004 validations.
///
/// Tracks validation requests by their heartbeat key and stores the outcome
/// when rounds finalize, enabling the keeper to submit validation responses.
#[derive(Default)]
pub struct Erc8004State {
    /// Maps heartbeat_key -> ValidationRequestInfo
    pub pending_validations: HashMap<B256, ValidationRequestInfo>,
}

/// Information about a pending validation request.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields kept for debugging and potential future use
pub struct ValidationRequestInfo {
    /// The validator address that submitted the request
    pub validator_address: Address,
    /// The agent ID being validated
    pub agent_id: U256,
    /// The request URI
    pub request_uri: String,
    /// The request hash (unique identifier for the validation)
    pub request_hash: B256,
    /// The ERC-8004 validation response value (0-100), mapped from HeartbeatManager outcome.
    pub outcome: Option<u8>,
    /// Whether the validation response has been submitted
    pub response_submitted: bool,
}

impl ValidationRequestInfo {
    pub fn new(
        validator_address: Address,
        agent_id: U256,
        request_uri: String,
        request_hash: B256,
    ) -> Self {
        Self {
            validator_address,
            agent_id,
            request_uri,
            request_hash,
            outcome: None,
            response_submitted: false,
        }
    }
}
