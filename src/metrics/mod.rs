//! Prometheus metrics module for HeartbeatManager monitoring.
//!
//! This module provides:
//! - Prometheus metrics registry with all metric definitions
//! - HTTP server for exposing /metrics endpoint
//! - Event collectors for populating metrics from blockchain events

pub mod collectors;
pub mod http_server;
pub mod registry;

// Re-exports for convenience
pub use collectors::{
    collect_htx_enqueued, collect_operator_voted, collect_round_started, load_historical_events,
    mark_htx_finalized, poll_operator_data, MetricsState,
};
pub use http_server::start_http_server;
pub use registry::register_metrics;
