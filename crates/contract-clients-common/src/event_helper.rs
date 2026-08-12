//! # Event Helper
//!
//! This module provides common event listening and querying patterns to reduce boilerplate
//! across contract clients.
//!
//! ## Usage
//!
//! ```ignore
//! use contract_clients_common::event_helper::BlockRange;
//!
//! // Query with block range
//! let range = BlockRange::last_n_blocks(1000);
//! ```

use anyhow::{Result, bail};
use futures_util::StreamExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tracing::error;

/// Detects an event subscription that has gone silently dead.
///
/// An RPC can accept a WebSocket connection and still fail to register the
/// subscription server-side (for example by answering `eth_subscribe` with an
/// error while it is overloaded). Alloy re-issues subscriptions on reconnect but
/// only re-registers them on a *successful* response, and it discards failures,
/// so the local stream stays open and simply never yields again. No transport
/// error is reported and the stream never terminates, which means silence on the
/// wire is the only available signal.
#[derive(Clone, Debug)]
pub struct StreamWatchdog {
    /// How long the stream may stay silent before it is presumed dead.
    idle_timeout: Duration,
    /// Number of events delivered, so callers can distinguish a stream that is
    /// working from one that never delivered anything.
    events_seen: Arc<AtomicU64>,
}

impl StreamWatchdog {
    pub fn new(idle_timeout: Duration, events_seen: Arc<AtomicU64>) -> Self {
        Self {
            idle_timeout,
            events_seen,
        }
    }

    pub fn idle_timeout(&self) -> Duration {
        self.idle_timeout
    }
}

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
/// * `watchdog` - Optional liveness watchdog; returns an error if the stream
///   stays silent longer than its idle timeout
/// * `predicate` - Function that returns true if the event should be processed
/// * `callback` - Async function to process each matching event
pub async fn listen_events_filtered<E, Err, L, P, F, Fut>(
    mut stream: L,
    event_name: &str,
    watchdog: Option<StreamWatchdog>,
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
    loop {
        // Any item resets the idle timer, including a decode failure: it still
        // proves the subscription is delivering.
        let next_item = match &watchdog {
            Some(watchdog) => {
                match tokio::time::timeout(watchdog.idle_timeout, stream.next()).await {
                    Ok(item) => item,
                    // Deliberately worded to avoid the substring "Shutdown", which
                    // the node's supervisor uses to recognise an intentional stop.
                    Err(_elapsed) => bail!(
                        "no {} events received in {}s; subscription presumed dead",
                        event_name,
                        watchdog.idle_timeout.as_secs()
                    ),
                }
            }
            None => stream.next().await,
        };

        let Some(event_result) = next_item else {
            // Stream closed: the transport gave up, which the caller can see.
            break;
        };

        match event_result {
            Ok((event, _log)) => {
                // Counted before the predicate so liveness reflects the whole
                // subscription rather than this node's share of it.
                if let Some(watchdog) = &watchdog {
                    watchdog.events_seen.fetch_add(1, Ordering::Relaxed);
                }
                if predicate(&event)
                    && let Err(e) = callback(event).await
                {
                    error!("Error processing {} event: {}", event_name, e);
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
    listen_events_filtered(stream, event_name, None, |_| true, callback).await
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

    // ------------------------------------------------------------------------
    // Watchdog
    // ------------------------------------------------------------------------

    type TestItem = Result<(u8, alloy::rpc::types::Log), String>;

    fn watchdog(secs: u64) -> (StreamWatchdog, Arc<AtomicU64>) {
        let seen = Arc::new(AtomicU64::new(0));
        (
            StreamWatchdog::new(Duration::from_secs(secs), seen.clone()),
            seen,
        )
    }

    fn event(v: u8) -> TestItem {
        Ok((v, alloy::rpc::types::Log::default()))
    }

    #[tokio::test(start_paused = true)]
    async fn watchdog_errors_when_stream_goes_silent() {
        let (wd, _seen) = watchdog(900);
        let stream = futures_util::stream::pending::<TestItem>();

        let err = listen_events_filtered(
            stream,
            "RoundStarted",
            Some(wd),
            |_| true,
            |_| async { Ok(()) },
        )
        .await
        .expect_err("a silent stream must be reported");

        let message = err.to_string();
        assert!(message.contains("RoundStarted"), "message: {message}");
        assert!(message.contains("900"), "message: {message}");
        // The supervisor distinguishes an intentional stop by looking for this
        // substring, so the watchdog must never produce it.
        assert!(!message.contains("Shutdown"), "message: {message}");
    }

    #[tokio::test(start_paused = true)]
    async fn silent_stream_is_tolerated_without_a_watchdog() {
        // Ends only because the stream terminates; with `pending` this would
        // hang, which is exactly the pre-watchdog behaviour.
        let stream = futures_util::stream::iter(Vec::<TestItem>::new());
        listen_events_filtered(stream, "RoundStarted", None, |_| true, |_| async { Ok(()) })
            .await
            .expect("stream closing is not an error");
    }

    #[tokio::test(start_paused = true)]
    async fn events_are_counted_before_the_predicate() {
        let (wd, seen) = watchdog(900);
        let stream = futures_util::stream::iter(vec![event(1), event(2), event(3)]);
        let delivered = Arc::new(AtomicU64::new(0));
        let delivered_cb = delivered.clone();

        // Reject everything, as a node not in the committee would.
        listen_events_filtered(
            stream,
            "RoundStarted",
            Some(wd),
            |_| false,
            move |_| {
                let delivered = delivered_cb.clone();
                async move {
                    delivered.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                }
            },
        )
        .await
        .expect("stream completed");

        // Liveness must track the whole subscription, not this node's share:
        // rounds it is not a member of still prove the stream is alive.
        assert_eq!(seen.load(Ordering::Relaxed), 3);
        assert_eq!(delivered.load(Ordering::Relaxed), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn stream_errors_do_not_count_as_events() {
        let (wd, seen) = watchdog(900);
        let stream = futures_util::stream::iter(vec![Err("decode failed".to_string()), event(1)]);

        listen_events_filtered(
            stream,
            "RoundStarted",
            Some(wd),
            |_| true,
            |_| async { Ok(()) },
        )
        .await
        .expect("stream completed");

        assert_eq!(seen.load(Ordering::Relaxed), 1);
    }
}
