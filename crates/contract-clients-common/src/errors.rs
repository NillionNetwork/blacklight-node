//! # Error Handling for Solidity Revert Data
//!
//! Decodes Solidity revert data into human-readable error messages.
//!
//! ## Supported Error Types
//!
//! 1. **Standard `Error(string)`** - From `require(condition, "message")`
//! 2. **Standard `Panic(uint256)`** - From `assert()` failures and arithmetic errors
//! 3. **Custom Contract Errors** - Via a caller-provided decoder function
//!
//! ## Main Entry Points
//!
//! - [`extract_revert_from_contract_error_with_custom`] - For Alloy's `ContractError` type
//! - [`decode_revert_with_custom`] - For raw `Bytes` revert data

use alloy::{
    contract::Error as ContractError, hex, primitives::Bytes, sol, sol_types::SolInterface,
    transports::TransportError,
};

// ============================================================================
// Standard Solidity Errors
// ============================================================================

sol! {
    #[derive(Debug, PartialEq, Eq)]
    library StandardErrors {
        error Error(string message);
        error Panic(uint256 code);
    }
}

// ============================================================================
// DecodedRevert
// ============================================================================

#[derive(Debug, Clone)]
pub enum DecodedRevert {
    /// Standard `Error(string)` from `require()`.
    ErrorString(String),
    /// Panic error with a numeric code. See [`panic_reason`].
    Panic(u64),
    /// Custom error decoded by a consumer-provided decoder.
    CustomError(String),
    /// Raw revert data that couldn't be decoded.
    RawRevert(Bytes),
    /// No revert data was available.
    NoRevertData(String),
}

impl std::fmt::Display for DecodedRevert {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodedRevert::ErrorString(msg) => write!(f, "{}", msg),
            DecodedRevert::Panic(code) => write!(f, "Panic({}): {}", code, panic_reason(*code)),
            DecodedRevert::CustomError(msg) => write!(f, "{}", msg),
            DecodedRevert::RawRevert(data) => write!(f, "Raw revert data: {}", data),
            DecodedRevert::NoRevertData(details) => write!(f, "No revert data ({})", details),
        }
    }
}

// ============================================================================
// Panic Code Meanings
// ============================================================================

pub fn panic_reason(code: u64) -> &'static str {
    match code {
        0x00 => "generic compiler panic",
        0x01 => "assertion failed",
        0x11 => "arithmetic overflow/underflow",
        0x12 => "division by zero",
        0x21 => "invalid enum value",
        0x22 => "storage byte array encoding error",
        0x31 => "pop on empty array",
        0x32 => "array index out of bounds",
        0x41 => "memory allocation overflow",
        0x51 => "zero-initialized function pointer call",
        _ => "unknown panic code",
    }
}

// ============================================================================
// Core Decoding
// ============================================================================

/// Decode raw revert bytes: StandardErrors first, then custom decoder, then raw fallback.
pub fn decode_revert_with_custom<F>(data: &Bytes, custom_decoder: F) -> DecodedRevert
where
    F: FnOnce(&Bytes) -> Option<DecodedRevert>,
{
    if data.is_empty() {
        return DecodedRevert::NoRevertData("empty revert data".to_string());
    }

    if let Ok(err) = StandardErrors::StandardErrorsErrors::abi_decode(data) {
        match err {
            StandardErrors::StandardErrorsErrors::Error(e) => {
                return DecodedRevert::ErrorString(e.message);
            }
            StandardErrors::StandardErrorsErrors::Panic(p) => {
                return DecodedRevert::Panic(p.code.try_into().unwrap_or(0));
            }
        }
    }

    if let Some(decoded) = custom_decoder(data) {
        return decoded;
    }

    DecodedRevert::RawRevert(data.clone())
}

// ============================================================================
// ContractError Extraction
// ============================================================================

/// Extract and decode revert data from an Alloy [`ContractError`] with a custom decoder.
pub fn extract_revert_from_contract_error_with_custom<F>(
    error: &ContractError,
    custom_decoder: F,
) -> DecodedRevert
where
    F: Fn(&Bytes) -> Option<DecodedRevert>,
{
    match error {
        ContractError::TransportError(transport_err) => {
            extract_revert_from_transport_error(transport_err, &custom_decoder)
        }
        ContractError::AbiError(abi_err) => {
            DecodedRevert::NoRevertData(format!("ABI error: {}", abi_err))
        }
        _ => {
            let debug_str = format!("{:?}", error);
            try_extract_from_string(&debug_str, &custom_decoder).unwrap_or_else(|| {
                DecodedRevert::NoRevertData(format!("Unknown error type: {}", error))
            })
        }
    }
}

fn extract_revert_from_transport_error<F>(
    error: &TransportError,
    custom_decoder: &F,
) -> DecodedRevert
where
    F: Fn(&Bytes) -> Option<DecodedRevert>,
{
    match error {
        TransportError::ErrorResp(err_resp) => {
            if let Some(data) = &err_resp.data {
                let data_str = data.get();
                let data_str = data_str.trim_matches('"');

                if let Some(hex_data) = data_str.strip_prefix("0x")
                    && let Ok(bytes) = hex::decode(hex_data)
                {
                    return decode_revert_with_custom(&Bytes::from(bytes), |b| custom_decoder(b));
                }
                return DecodedRevert::NoRevertData(format!("Error data: {}", data_str));
            }
            DecodedRevert::NoRevertData(format!("RPC error: {}", err_resp.message))
        }
        _ => {
            let err_str = error.to_string();
            try_extract_from_string(&err_str, custom_decoder).unwrap_or_else(|| {
                DecodedRevert::NoRevertData(format!("Transport error: {}", err_str))
            })
        }
    }
}

// ============================================================================
// String Pattern Extraction (internal fallback)
// ============================================================================

fn try_extract_from_string<F>(error_str: &str, custom_decoder: &F) -> Option<DecodedRevert>
where
    F: Fn(&Bytes) -> Option<DecodedRevert>,
{
    const PATTERNS: &[&str] = &[
        "execution reverted: 0x",
        "reverted with data: 0x",
        "revert data: 0x",
        "data: 0x",
        "0x08c379a0",
        "0x4e487b71",
    ];

    for pattern in PATTERNS {
        if let Some(start) = error_str.find(pattern) {
            let hex_start = if pattern.ends_with("0x") {
                start + pattern.len() - 2
            } else {
                start
            };

            let remaining = &error_str[hex_start..];

            let hex_end = if remaining.starts_with("0x") {
                2 + remaining
                    .strip_prefix("0x")
                    .unwrap_or(remaining)
                    .chars()
                    .take_while(|c| c.is_ascii_hexdigit())
                    .count()
            } else {
                remaining
                    .chars()
                    .take_while(|c| c.is_ascii_hexdigit())
                    .count()
            };

            let hex_str = &remaining[..hex_end];
            if hex_str.len() >= 10 {
                let without_prefix = hex_str.strip_prefix("0x").unwrap_or(hex_str);
                if let Ok(bytes) = hex::decode(without_prefix) {
                    return Some(decode_revert_with_custom(&Bytes::from(bytes), |b| {
                        custom_decoder(b)
                    }));
                }
            }
        }
    }

    // Plain text error after "execution reverted:"
    if let Some(idx) = error_str.find("execution reverted:") {
        let after = &error_str[idx + 19..];
        let msg = after.trim().trim_matches('"').trim();
        if !msg.is_empty() && !msg.starts_with("0x") {
            return Some(DecodedRevert::ErrorString(msg.to_string()));
        }
    }

    None
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_error_string() {
        let data = hex::decode(
            "08c379a0\
             0000000000000000000000000000000000000000000000000000000000000020\
             0000000000000000000000000000000000000000000000000000000000000017\
             626c61636b6c696768743a20756e6b6e6f776e20485458000000000000000000",
        )
        .unwrap();

        let decoded = decode_revert_with_custom(&Bytes::from(data), |_| None);
        assert!(
            matches!(decoded, DecodedRevert::ErrorString(msg) if msg == "blacklight: unknown HTX")
        );
    }

    #[test]
    fn test_decode_panic() {
        let data = hex::decode(
            "4e487b71\
             0000000000000000000000000000000000000000000000000000000000000001",
        )
        .unwrap();

        let decoded = decode_revert_with_custom(&Bytes::from(data), |_| None);
        assert!(matches!(decoded, DecodedRevert::Panic(1)));
    }

    #[test]
    fn test_display() {
        assert_eq!(
            format!("{}", DecodedRevert::ErrorString("test error".to_string())),
            "test error"
        );
        assert_eq!(
            format!("{}", DecodedRevert::Panic(1)),
            "Panic(1): assertion failed"
        );
        assert_eq!(
            format!("{}", DecodedRevert::CustomError("Custom error".to_string())),
            "Custom error"
        );
    }

    #[test]
    fn test_try_extract_from_string() {
        let error_msg = "execution reverted: 0x08c379a00000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000001f626c61636b6c696768743a204854582020616c7265616479206578697374730000";
        let decoded = try_extract_from_string(error_msg, &|_| None);
        assert!(decoded.is_some());
        if let Some(DecodedRevert::ErrorString(msg)) = decoded {
            assert!(msg.contains("blacklight"));
        }
    }

    #[test]
    fn test_panic_reasons() {
        assert_eq!(panic_reason(0x01), "assertion failed");
        assert_eq!(panic_reason(0x11), "arithmetic overflow/underflow");
        assert_eq!(panic_reason(0x12), "division by zero");
    }

    #[test]
    fn test_custom_decoder() {
        let data = hex::decode("deadbeef").unwrap();

        let decoded = decode_revert_with_custom(&Bytes::from(data.clone()), |_| None);
        assert!(matches!(decoded, DecodedRevert::RawRevert(_)));

        let decoded = decode_revert_with_custom(&Bytes::from(data), |bytes| {
            if bytes.starts_with(&[0xde, 0xad, 0xbe, 0xef]) {
                Some(DecodedRevert::CustomError("Known custom error".to_string()))
            } else {
                None
            }
        });
        assert!(matches!(decoded, DecodedRevert::CustomError(msg) if msg == "Known custom error"));
    }
}
