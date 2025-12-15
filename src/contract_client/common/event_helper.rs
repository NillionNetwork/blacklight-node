//! # Event Helper
//!
//! This module provides common event listening and querying patterns to reduce boilerplate
//! across contract clients.
//!
//! ## Usage
//!
//! ```ignore
//! use crate::contract_client::event_helper::BlockRange;
//!
//! // Query with block range
//! let range = BlockRange::last_n_blocks(1000);
//! ```

use anyhow::Result;
use futures_util::StreamExt;
use tracing::error;

/// Represents a block range for event queries.
///
/// Provides convenient constructors for common query patterns.
#[derive(Debug, Clone, Copy)]
pub struct BlockRange {
    pub from_block: u64,
    pub to_block: Option<u64>,
}

impl BlockRange {
    /// Create a range from a specific block to the latest block.
    pub fn from(from_block: u64) -> Self {
        Self {
            from_block,
            to_block: None,
        }
    }

    /// Create a range between two specific blocks (inclusive).
    pub fn between(from_block: u64, to_block: u64) -> Self {
        Self {
            from_block,
            to_block: Some(to_block),
        }
    }

    /// Create a range for the last N blocks from the current block.
    ///
    /// Note: This requires knowing the current block number, so it returns
    /// a function that takes the current block and returns the range.
    pub fn from_lookback(current_block: u64, lookback_blocks: u64) -> Self {
        Self {
            from_block: current_block.saturating_sub(lookback_blocks),
            to_block: None,
        }
    }

    /// Query the entire blockchain history.
    pub fn all() -> Self {
        Self {
            from_block: 0,
            to_block: None,
        }
    }
}

impl Default for BlockRange {
    fn default() -> Self {
        Self::all()
    }
}

/// Listen to events with a filter predicate.
///
/// This is the base event listener that reduces boilerplate by handling:
/// - Stream iteration
/// - Error logging for both event processing and reception
/// - Graceful handling of stream termination
/// - Optional filtering via predicate
///
/// # Type Parameters
///
/// * `E` - The event type (must be Send)
/// * `Err` - The error type from the stream
/// * `L` - The stream type
/// * `P` - The predicate function type
/// * `F` - The callback function type
/// * `Fut` - The future returned by the callback
///
/// # Arguments
///
/// * `stream` - The event stream to listen to
/// * `event_name` - Name of the event for logging purposes
/// * `predicate` - Function that returns true if the event should be processed
/// * `callback` - Async function to process each matching event
pub async fn listen_events_filtered<E, Err, L, P, F, Fut>(
    mut stream: L,
    event_name: &str,
    predicate: P,
    mut callback: F,
) -> Result<()>
where
    E: Send,
    Err: std::fmt::Display,
    L: StreamExt<Item = Result<(E, alloy::rpc::types::Log), Err>> + Unpin + Send,
    P: Fn(&E) -> bool + Send,
    F: FnMut(E) -> Fut + Send,
    Fut: std::future::Future<Output = Result<()>> + Send,
{
    while let Some(event_result) = stream.next().await {
        match event_result {
            Ok((event, _log)) => {
                if predicate(&event) {
                    if let Err(e) = callback(event).await {
                        error!("Error processing {} event: {}", event_name, e);
                    }
                }
            }
            Err(e) => {
                error!("Error receiving {} event: {}", event_name, e);
            }
        }
    }
    Ok(())
}

/// Listen to events from a subscription and process them with a callback.
///
/// This is a convenience wrapper around [`listen_events_filtered`] that processes
/// all events without filtering.
///
/// # Arguments
///
/// * `stream` - The event stream to listen to
/// * `event_name` - Name of the event for logging purposes
/// * `callback` - Async function to process each event
pub async fn listen_events<E, Err, L, F, Fut>(
    stream: L,
    event_name: &str,
    callback: F,
) -> Result<()>
where
    E: Send,
    Err: std::fmt::Display,
    L: StreamExt<Item = Result<(E, alloy::rpc::types::Log), Err>> + Unpin + Send,
    F: FnMut(E) -> Fut + Send,
    Fut: std::future::Future<Output = Result<()>> + Send,
{
    listen_events_filtered(stream, event_name, |_| true, callback).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_range_from() {
        let range = BlockRange::from(100);
        assert_eq!(range.from_block, 100);
        assert_eq!(range.to_block, None);
    }

    #[test]
    fn test_block_range_between() {
        let range = BlockRange::between(100, 200);
        assert_eq!(range.from_block, 100);
        assert_eq!(range.to_block, Some(200));
    }

    #[test]
    fn test_block_range_lookback() {
        let range = BlockRange::from_lookback(1000, 100);
        assert_eq!(range.from_block, 900);
        assert_eq!(range.to_block, None);
    }

    #[test]
    fn test_block_range_lookback_underflow() {
        let range = BlockRange::from_lookback(50, 100);
        assert_eq!(range.from_block, 0); // saturating_sub prevents underflow
        assert_eq!(range.to_block, None);
    }

    #[test]
    fn test_block_range_all() {
        let range = BlockRange::all();
        assert_eq!(range.from_block, 0);
        assert_eq!(range.to_block, None);
    }
}
