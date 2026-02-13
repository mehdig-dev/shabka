use std::future::Future;

use crate::error::Result;

/// Retry an async operation with exponential backoff for transient errors.
/// Non-transient errors are returned immediately.
pub async fn with_retry<F, Fut, T>(max_retries: usize, base_delay_ms: u64, f: F) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut last_err = None;
    for attempt in 0..=max_retries {
        match f().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                if !e.is_transient() || attempt == max_retries {
                    return Err(e);
                }
                let delay = base_delay_ms * 2u64.pow(attempt as u32);
                tracing::warn!(
                    attempt = attempt + 1,
                    max_retries,
                    delay_ms = delay,
                    error = %e,
                    "transient error, retrying"
                );
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::KaizenError;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn test_success_on_first_attempt() {
        let attempts = AtomicUsize::new(0);
        let result = with_retry(3, 1, || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Ok::<_, KaizenError>(42) }
        })
        .await;
        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_non_transient_error_no_retry() {
        let attempts = AtomicUsize::new(0);
        let result = with_retry(3, 1, || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Err::<i32, _>(KaizenError::Config("bad config".into())) }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_transient_error_retries_up_to_max() {
        let attempts = AtomicUsize::new(0);
        let result = with_retry(2, 1, || {
            attempts.fetch_add(1, Ordering::SeqCst);
            async { Err::<i32, _>(KaizenError::Storage("HTTP 503 unavailable".into())) }
        })
        .await;
        assert!(result.is_err());
        // initial attempt + 2 retries = 3 total
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_transient_error_succeeds_on_retry() {
        let attempts = AtomicUsize::new(0);
        let result = with_retry(3, 1, || {
            let attempt = attempts.fetch_add(1, Ordering::SeqCst);
            async move {
                if attempt < 2 {
                    Err::<i32, _>(KaizenError::Storage("HTTP 503 unavailable".into()))
                } else {
                    Ok(99)
                }
            }
        })
        .await;
        assert_eq!(result.unwrap(), 99);
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }
}
