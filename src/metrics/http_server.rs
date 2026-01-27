//! HTTP server for exposing Prometheus metrics endpoint.

use crate::metrics::registry::REGISTRY;
use axum::{routing::get, Router};
use prometheus::Encoder;
use std::net::SocketAddr;
use tracing::info;

/// Handler for GET /metrics endpoint
async fn metrics_handler() -> String {
    let encoder = prometheus::TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = Vec::new();
    encoder
        .encode(&metric_families, &mut buffer)
        .expect("Failed to encode metrics");
    String::from_utf8(buffer).expect("Metrics output is not valid UTF-8")
}

/// Handler for GET /health endpoint
async fn health_handler() -> &'static str {
    "OK"
}

/// Start the HTTP server for metrics exposure
pub async fn start_http_server(port: u16) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/health", get(health_handler));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Starting metrics HTTP server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
