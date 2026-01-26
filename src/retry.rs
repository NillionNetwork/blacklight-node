//! Generic retry mechanism for async operations.
//!
//! This module provides reusable retry utilities that automatically retry failed operations
//! with configurable delay and backoff strategies.
//!
//! # Usage
//!
//! The retry functions return `Result<T, E>` directly, which can be converted
//! to `anyhow::Result<T>` using `.into_anyhow()`:
//!
//! ```ignore
//! use crate::retry::{retry, RetryConfig, IntoAnyhow};
//!
//! async fn do_something() -> anyhow::Result<()> {
//!     retry(RetryConfig::default(), "my_operation", || async {
//!         fallible_operation().await
//!     }).await.into_anyhow()
//! }
//! ```

use std::future::Future;
use std::time::Duration;
use tracing::{debug, warn};

use crate::config::consts::{
    DEFAULT_MAX_RETRY_ATTEMPTS, DEFAULT_READ_RETRY_DELAY_SECS, DEFAULT_RETRY_DELAY_SECS,
};

/// Configuration for retry behavior.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of attempts. 0 means infinite retries.
    pub max_attempts: u32,
    /// Initial delay between retries.
    pub delay: Duration,
    /// Multiplier for exponential backoff. 1.0 = fixed delay, 2.0 = double each time.
    pub backoff_multiplier: f64,
    /// Maximum delay cap for exponential backoff.
    pub max_delay: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_RETRY_ATTEMPTS,
            delay: Duration::from_secs(DEFAULT_RETRY_DELAY_SECS),
            backoff_multiplier: 1.0,
            max_delay: Duration::from_secs(300), // 5 minute cap
        }
    }
}

impl RetryConfig {
    /// Create a fixed-delay retry configuration.
    ///
    /// # Arguments
    /// * `delay_secs` - Delay in seconds between retries
    /// * `max_attempts` - Maximum number of attempts (0 = infinite)
    pub fn fixed(delay_secs: u64, max_attempts: u32) -> Self {
        Self {
            max_attempts,
            delay: Duration::from_secs(delay_secs),
            backoff_multiplier: 1.0,
            max_delay: Duration::from_secs(delay_secs),
        }
    }

    /// Create a retry configuration optimized for read operations.
    /// Uses shorter delay (5s) since reads are fast and idempotent.
    pub fn for_reads() -> Self {
        Self::fixed(DEFAULT_READ_RETRY_DELAY_SECS, DEFAULT_MAX_RETRY_ATTEMPTS)
    }

    /// Create an exponential backoff retry configuration.
    ///
    /// # Arguments
    /// * `initial_delay_secs` - Initial delay in seconds
    /// * `max_attempts` - Maximum number of attempts (0 = infinite)
    /// * `multiplier` - Backoff multiplier (e.g., 2.0 doubles delay each time)
    /// * `max_delay_secs` - Maximum delay cap in seconds
    pub fn exponential(
        initial_delay_secs: u64,
        max_attempts: u32,
        multiplier: f64,
        max_delay_secs: u64,
    ) -> Self {
        Self {
            max_attempts,
            delay: Duration::from_secs(initial_delay_secs),
            backoff_multiplier: multiplier,
            max_delay: Duration::from_secs(max_delay_secs),
        }
    }

    /// Calculate delay for a given attempt number.
    fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if self.backoff_multiplier <= 1.0 {
            return self.delay;
        }

        let multiplier = self
            .backoff_multiplier
            .powi(attempt.saturating_sub(1) as i32);
        let delay_millis = (self.delay.as_millis() as f64 * multiplier) as u64;
        let delay = Duration::from_millis(delay_millis);

        std::cmp::min(delay, self.max_delay)
    }
}

/// Extension trait for converting `Result<T, E>` to `anyhow::Result<T>`.
///
/// This provides a clean way to convert retry results to anyhow results.
///
/// # Example
/// ```ignore
/// use crate::retry::{retry, RetryConfig, IntoAnyhow};
///
/// async fn example() -> anyhow::Result<()> {
///     retry(RetryConfig::default(), "operation", || async {
///         do_something().await
///     })
///     .await
///     .into_anyhow()
/// }
/// ```
pub trait IntoAnyhow<T> {
    fn into_anyhow(self) -> anyhow::Result<T>;
}

impl<T, E: Into<anyhow::Error>> IntoAnyhow<T> for Result<T, E> {
    fn into_anyhow(self) -> anyhow::Result<T> {
        self.map_err(Into::into)
    }
}

/// Retry an async operation until it succeeds or max attempts are exhausted.
///
/// All errors are considered retryable. Use `retry_with_classifier` if you need
/// to distinguish between retryable and non-retryable errors.
///
/// # Arguments
/// * `config` - Retry configuration
/// * `operation_name` - Name for logging purposes
/// * `operation` - The async operation to retry
///
/// # Example
/// ```ignore
/// let config = RetryConfig::fixed(30, 3);
/// let tx_hash = retry(config, "submit_tx", || async {
///     client.submit_htx(&htx).await
/// }).await.into_anyhow()?;
/// ```
pub async fn retry<F, Fut, T, E>(
    config: RetryConfig,
    operation_name: &str,
    operation: F,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    retry_with_classifier(config, operation_name, operation, |_| true).await
}

/// Retry an async operation with error classification.
///
/// Only retries errors for which the classifier returns true. Non-retryable errors
/// cause immediate failure without exhausting retry attempts.
///
/// # Arguments
/// * `config` - Retry configuration
/// * `operation_name` - Name for logging purposes
/// * `operation` - The async operation to retry
/// * `is_retryable` - Predicate that returns true if the error should be retried
///
/// # Example
/// ```ignore
/// retry_with_classifier(config, "operation", || async {
///     do_something().await
/// }, |e| e.is_transient()).await.into_anyhow()?;
/// ```
pub async fn retry_with_classifier<F, Fut, T, E, C>(
    config: RetryConfig,
    operation_name: &str,
    operation: F,
    is_retryable: C,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
    C: Fn(&E) -> bool,
{
    let mut attempt = 1u32;
    let max_attempts_str = if config.max_attempts == 0 {
        "∞".to_string()
    } else {
        config.max_attempts.to_string()
    };

    loop {
        match operation().await {
            Ok(result) => {
                if attempt > 1 {
                    debug!(
                        operation = operation_name,
                        attempt = attempt,
                        "Operation succeeded after retry"
                    );
                }
                return Ok(result);
            }
            Err(e) => {
                // Check if error is retryable
                if !is_retryable(&e) {
                    debug!(
                        operation = operation_name,
                        error = %e,
                        "Non-retryable error, failing immediately"
                    );
                    return Err(e);
                }

                // Check if we've exhausted attempts
                if config.max_attempts > 0 && attempt >= config.max_attempts {
                    warn!(
                        operation = operation_name,
                        attempt = attempt,
                        max_attempts = config.max_attempts,
                        error = %e,
                        "Max retry attempts exhausted"
                    );
                    return Err(e);
                }

                let delay = config.delay_for_attempt(attempt);
                warn!(
                    operation = operation_name,
                    attempt = attempt,
                    max_attempts = %max_attempts_str,
                    delay_secs = delay.as_secs(),
                    error = %e,
                    "Operation failed, retrying after delay"
                );

                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_retry_config_fixed() {
        let config = RetryConfig::fixed(30, 3);
        assert_eq!(config.max_attempts, 3);
        assert_eq!(config.delay, Duration::from_secs(30));
        assert_eq!(config.backoff_multiplier, 1.0);
    }

    #[test]
    fn test_retry_config_exponential() {
        let config = RetryConfig::exponential(1, 5, 2.0, 60);
        assert_eq!(config.max_attempts, 5);
        assert_eq!(config.delay, Duration::from_secs(1));
        assert_eq!(config.backoff_multiplier, 2.0);
        assert_eq!(config.max_delay, Duration::from_secs(60));
    }

    #[test]
    fn test_delay_for_attempt_fixed() {
        let config = RetryConfig::fixed(30, 3);
        assert_eq!(config.delay_for_attempt(1), Duration::from_secs(30));
        assert_eq!(config.delay_for_attempt(2), Duration::from_secs(30));
        assert_eq!(config.delay_for_attempt(3), Duration::from_secs(30));
    }

    #[test]
    fn test_delay_for_attempt_exponential() {
        let config = RetryConfig::exponential(1, 10, 2.0, 60);
        assert_eq!(config.delay_for_attempt(1), Duration::from_secs(1));
        assert_eq!(config.delay_for_attempt(2), Duration::from_secs(2));
        assert_eq!(config.delay_for_attempt(3), Duration::from_secs(4));
        assert_eq!(config.delay_for_attempt(4), Duration::from_secs(8));
        // Should cap at max_delay
        assert_eq!(config.delay_for_attempt(10), Duration::from_secs(60));
    }

    #[tokio::test]
    async fn test_retry_success_first_attempt() {
        let config = RetryConfig::fixed(1, 3);
        let result: Result<i32, &str> = retry(config, "test_op", || async { Ok(42) }).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_retry_success_after_failures() {
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = attempts.clone();

        let config = RetryConfig::fixed(0, 3); // 0 delay for fast test
        let result: Result<i32, String> = retry(config, "test_op", || {
            let attempts = attempts_clone.clone();
            async move {
                let current = attempts.fetch_add(1, Ordering::SeqCst);
                if current < 2 {
                    Err(format!("Attempt {} failed", current + 1))
                } else {
                    Ok(42)
                }
            }
        })
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_exhausted() {
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = attempts.clone();

        let config = RetryConfig::fixed(0, 3); // 0 delay for fast test
        let result: Result<i32, String> = retry(config, "test_op", || {
            let attempts = attempts_clone.clone();
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err("Always fails".to_string())
            }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_retry_with_classifier_non_retryable() {
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = attempts.clone();

        #[derive(Debug)]
        enum TestError {
            Retryable,
            NonRetryable,
        }

        impl std::fmt::Display for TestError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{:?}", self)
            }
        }

        let config = RetryConfig::fixed(0, 5);
        let result: Result<i32, TestError> = retry_with_classifier(
            config,
            "test_op",
            || {
                let attempts = attempts_clone.clone();
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    Err(TestError::NonRetryable)
                }
            },
            |e| matches!(e, TestError::Retryable),
        )
        .await;

        assert!(result.is_err());
        // Should only attempt once since error is non-retryable
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();
        assert_eq!(config.max_attempts, DEFAULT_MAX_RETRY_ATTEMPTS);
        assert_eq!(config.delay, Duration::from_secs(DEFAULT_RETRY_DELAY_SECS));
    }
}
