use anyhow::{Error, Result};
use async_trait::async_trait;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;
use tracing::error;

/// Default slot interval in milliseconds.
#[cfg(debug_assertions)]
pub const DEFAULT_SLOT_MS: u64 = 3000;

#[cfg(not(debug_assertions))]
pub const DEFAULT_SLOT_MS: u64 = 5000;

pub const MAX_RETRIES: u32 = 3;
pub const RETRY_DELAY_MS: u64 = 500;

#[async_trait]
pub trait Simulator: Send + Sync + 'static {
    type Args: clap::Args + Send;

    async fn build(args: Self::Args) -> Result<Self>
    where
        Self: Sized;

    fn slot_ms(&self) -> u64;
    fn submission_error_message(&self) -> &'static str;

    async fn on_tick(&self, slot: u64) -> Result<()>;
}

pub async fn run_simulator<S: Simulator>(args: S::Args) -> Result<()> {
    let simulator = Arc::new(S::build(args).await?);
    run_slot_loop(simulator).await
}

async fn run_slot_loop<S: Simulator>(simulator: Arc<S>) -> Result<()> {
    let mut ticker = interval(Duration::from_millis(simulator.slot_ms()));
    let mut slot = 0u64;

    loop {
        ticker.tick().await;
        slot += 1;

        let simulator = Arc::clone(&simulator);
        tokio::spawn(async move {
            if let Err(e) = simulator.on_tick(slot).await {
                error!(slot, error = %e, "{}", simulator.submission_error_message());
            }
        });
    }
}

pub async fn retry_submit<F, Fut, R>(mut action: F, mut on_revert: R) -> Result<()>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<()>>,
    R: FnMut(u32, &Error),
{
    let mut last_error: Option<Error> = None;

    for attempt in 0..MAX_RETRIES {
        match action(attempt).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                if is_revert_error(&e) {
                    on_revert(attempt, &e);
                    last_error = Some(e);
                    tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS)).await;
                    continue;
                }
                return Err(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Max retries exceeded")))
}

fn is_revert_error(error: &Error) -> bool {
    error.to_string().contains("reverted on-chain")
}
