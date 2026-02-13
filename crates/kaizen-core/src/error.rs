use thiserror::Error;

#[derive(Debug, Error)]
pub enum KaizenError {
    #[error("Storage error: {0}")]
    Storage(String),

    #[error("HelixDB error: {0}")]
    Helix(#[from] helix_rs::HelixError),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Embedding error: {0}")]
    Embedding(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

impl KaizenError {
    /// Returns `true` when the error is likely transient and worth retrying
    /// (e.g. HTTP 429/5xx, network timeouts, connection refused).
    pub fn is_transient(&self) -> bool {
        match self {
            // reqwest errors are almost always network-level / transient
            Self::Http(_) => true,
            // Check embedded error messages for transient HTTP status codes
            Self::Embedding(msg) | Self::Storage(msg) => is_transient_message(msg),
            _ => false,
        }
    }
}

fn is_transient_message(msg: &str) -> bool {
    let msg_lower = msg.to_lowercase();
    // HTTP status codes that are retryable
    for code in ["429", "500", "502", "503", "504"] {
        if msg_lower.contains(code) {
            return true;
        }
    }
    // Network-level transient patterns
    let patterns = [
        "timeout",
        "timed out",
        "connection refused",
        "connection reset",
        "broken pipe",
        "temporarily unavailable",
    ];
    patterns.iter().any(|p| msg_lower.contains(p))
}

pub type Result<T> = std::result::Result<T, KaizenError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transient_429() {
        let err = KaizenError::Embedding("API error 429: rate limit exceeded".into());
        assert!(err.is_transient());
    }

    #[test]
    fn test_transient_503() {
        let err = KaizenError::Embedding("API error 503: service unavailable".into());
        assert!(err.is_transient());
    }

    #[test]
    fn test_transient_timeout() {
        let err = KaizenError::Embedding("connection timed out".into());
        assert!(err.is_transient());
    }

    #[test]
    fn test_transient_500() {
        let err = KaizenError::Storage("HTTP 500 internal server error".into());
        assert!(err.is_transient());
    }

    #[test]
    fn test_permanent_401() {
        let err = KaizenError::Embedding("API error 401: unauthorized".into());
        assert!(!err.is_transient());
    }

    #[test]
    fn test_permanent_config() {
        let err = KaizenError::Config("missing API key".into());
        assert!(!err.is_transient());
    }

    #[test]
    fn test_permanent_not_found() {
        let err = KaizenError::NotFound("memory xyz".into());
        assert!(!err.is_transient());
    }
}
