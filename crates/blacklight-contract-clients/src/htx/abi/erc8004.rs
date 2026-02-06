//! ERC-8004 HTX - ABI-Encoded
//!
//! On-chain validation standard for agent validations.

use alloy::primitives::{Address, B256, U256};
use alloy::sol_types::{SolType, sol_data};

/// ERC-8004 Validation HTX data parsed from ABI-encoded bytes.
///
/// This format follows the ERC-8004 standard for on-chain agent validations.
///
/// **ABI Format**: `abi.encode(validatorAddress, agentId, requestURI, requestHash)`
#[derive(Debug, Clone)]
pub struct Erc8004Htx {
    /// Address of the validator performing the validation.
    pub validator_address: Address,
    /// Unique identifier for the agent being validated.
    pub agent_id: U256,
    /// URI pointing to the validation request data.
    pub request_uri: String,
    /// Hash of the validation request for integrity verification.
    pub request_hash: B256,
}

impl Erc8004Htx {
    /// Decode ABI-encoded ERC-8004 validation data from raw bytes.
    ///
    /// # Errors
    ///
    /// Returns `Erc8004DecodeError` if the data cannot be decoded according to
    /// the ERC-8004 ABI specification.
    pub fn try_decode(data: &[u8]) -> Result<Self, Erc8004DecodeError> {
        type Erc8004Tuple = (
            sol_data::Address,
            sol_data::Uint<256>,
            sol_data::String,
            sol_data::FixedBytes<32>,
        );

        let (validator_address, agent_id, request_uri, request_hash) =
            Erc8004Tuple::abi_decode_params(data).map_err(|e| Erc8004DecodeError(e.to_string()))?;

        Ok(Self {
            validator_address,
            agent_id,
            request_uri,
            request_hash,
        })
    }
}

/// Error type for ERC-8004 HTX decoding failures.
#[derive(Debug)]
pub struct Erc8004DecodeError(pub String);

impl std::fmt::Display for Erc8004DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ERC-8004 decode error: {}", self.0)
    }
}

impl std::error::Error for Erc8004DecodeError {}
