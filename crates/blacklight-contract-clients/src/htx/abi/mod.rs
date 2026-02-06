//! ABI-Encoded HTX Formats
//!
//! This module contains all ABI-encoded HTX types.

pub mod erc8004;

pub use erc8004::*;

/// ABI-encoded HTX wrapper for all ABI-based formats.
#[derive(Debug, Clone)]
pub enum AbiHtx {
    Erc8004(Erc8004Htx),
}

impl AbiHtx {
    /// Try to decode ABI-encoded HTX data, attempting all known formats.
    ///
    /// # Errors
    ///
    /// Returns `AbiDecodeError::UnknownFormat` if the data doesn't match any
    /// supported ABI format.
    pub fn try_decode(data: &[u8]) -> Result<Self, AbiDecodeError> {
        if let Ok(erc8004_htx) = Erc8004Htx::try_decode(data) {
            return Ok(AbiHtx::Erc8004(erc8004_htx));
        }

        Err(AbiDecodeError::UnknownFormat)
    }
}

/// Error type for ABI HTX decoding failures.
#[derive(Debug, thiserror::Error)]
pub enum AbiDecodeError {
    #[error("Unknown ABI format: not valid ERC-8004 or other known ABI encoding")]
    UnknownFormat,
}
