//! PII scrubbing — detect and redact sensitive patterns from memory content before export.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// Configuration for PII scrubbing patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrubConfig {
    /// Enable PII scrubbing (default true)
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Redact email addresses (default true)
    #[serde(default = "default_true")]
    pub emails: bool,

    /// Redact API keys / bearer tokens (default true)
    #[serde(default = "default_true")]
    pub api_keys: bool,

    /// Redact IP addresses (default true)
    #[serde(default = "default_true")]
    pub ip_addresses: bool,

    /// Redact absolute file paths (default true)
    #[serde(default = "default_true")]
    pub file_paths: bool,

    /// Additional custom regex patterns to redact
    #[serde(default)]
    pub custom_patterns: Vec<String>,

    /// Replacement string for redacted content
    #[serde(default = "default_replacement")]
    pub replacement: String,
}

fn default_true() -> bool {
    true
}

fn default_replacement() -> String {
    "[REDACTED]".to_string()
}

impl Default for ScrubConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            emails: true,
            api_keys: true,
            ip_addresses: true,
            file_paths: true,
            custom_patterns: Vec::new(),
            replacement: default_replacement(),
        }
    }
}

// Pre-compiled regexes for common PII patterns
static EMAIL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap());

static API_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:api[_-]?key|bearer|token|secret|password|auth)[=:\s]+['"]?([a-zA-Z0-9_\-./+=]{16,})['"]?"#,
    )
    .unwrap()
});

static IP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b").unwrap());

static FILE_PATH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:/home/[a-zA-Z0-9._-]+|/Users/[a-zA-Z0-9._-]+|C:\\Users\\[a-zA-Z0-9._-]+)(?:[/\\][^\s,;'")\]}>]+)*"#).unwrap()
});

/// Scrub PII from a string based on the provided config.
pub fn scrub(text: &str, config: &ScrubConfig) -> String {
    if !config.enabled {
        return text.to_string();
    }

    let mut result = text.to_string();
    let replacement = &config.replacement;

    if config.api_keys {
        result = API_KEY_RE.replace_all(&result, replacement).to_string();
    }

    if config.emails {
        result = EMAIL_RE.replace_all(&result, replacement).to_string();
    }

    if config.ip_addresses {
        // Skip common non-PII IPs (0.0.0.0, 127.0.0.1, localhost patterns)
        let ip_re = &*IP_RE;
        result = ip_re
            .replace_all(&result, |caps: &regex::Captures| {
                let ip = caps.get(0).unwrap().as_str();
                if ip == "127.0.0.1" || ip == "0.0.0.0" || ip.starts_with("192.168.") {
                    ip.to_string()
                } else {
                    replacement.clone()
                }
            })
            .to_string();
    }

    if config.file_paths {
        result = FILE_PATH_RE.replace_all(&result, replacement).to_string();
    }

    // Apply custom patterns
    for pattern in &config.custom_patterns {
        if let Ok(re) = Regex::new(pattern) {
            result = re.replace_all(&result, replacement.as_str()).to_string();
        }
    }

    result
}

/// Summary of what was scrubbed from a text.
pub struct ScrubReport {
    pub emails_found: usize,
    pub api_keys_found: usize,
    pub ips_found: usize,
    pub paths_found: usize,
    pub custom_found: usize,
}

/// Analyze text for PII without modifying it.
pub fn analyze(text: &str, config: &ScrubConfig) -> ScrubReport {
    let emails_found = if config.emails {
        EMAIL_RE.find_iter(text).count()
    } else {
        0
    };

    let api_keys_found = if config.api_keys {
        API_KEY_RE.find_iter(text).count()
    } else {
        0
    };

    let ips_found = if config.ip_addresses {
        IP_RE
            .find_iter(text)
            .filter(|m| {
                let ip = m.as_str();
                ip != "127.0.0.1" && ip != "0.0.0.0" && !ip.starts_with("192.168.")
            })
            .count()
    } else {
        0
    };

    let paths_found = if config.file_paths {
        FILE_PATH_RE.find_iter(text).count()
    } else {
        0
    };

    let custom_found: usize = config
        .custom_patterns
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .map(|re| re.find_iter(text).count())
        .sum();

    ScrubReport {
        emails_found,
        api_keys_found,
        ips_found,
        paths_found,
        custom_found,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrub_emails() {
        let config = ScrubConfig::default();
        let input = "Contact user@example.com for help";
        let result = scrub(input, &config);
        assert_eq!(result, "Contact [REDACTED] for help");
    }

    #[test]
    fn test_scrub_api_keys() {
        let config = ScrubConfig::default();
        let input = "Use api_key=sk-1234567890abcdef1234 to authenticate";
        let result = scrub(input, &config);
        assert!(!result.contains("sk-1234567890abcdef1234"));
        assert!(result.contains("[REDACTED]"));
    }

    #[test]
    fn test_scrub_bearer_token() {
        let config = ScrubConfig::default();
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.test.signature";
        let result = scrub(input, &config);
        assert!(!result.contains("eyJhbGciOiJIUzI1NiJ9"));
        assert!(result.contains("[REDACTED]"));
    }

    #[test]
    fn test_scrub_ip_addresses() {
        let config = ScrubConfig::default();
        let input = "Server at 10.0.1.42 responded, localhost is 127.0.0.1";
        let result = scrub(input, &config);
        assert!(!result.contains("10.0.1.42"));
        assert!(result.contains("127.0.0.1")); // preserved
    }

    #[test]
    fn test_scrub_file_paths() {
        let config = ScrubConfig::default();
        let input = "File at /home/john/secrets/key.pem was modified";
        let result = scrub(input, &config);
        assert!(!result.contains("/home/john"));
        assert!(result.contains("[REDACTED]"));
    }

    #[test]
    fn test_scrub_disabled() {
        let config = ScrubConfig {
            enabled: false,
            ..Default::default()
        };
        let input = "user@example.com api_key=sk-secret1234567890";
        let result = scrub(input, &config);
        assert_eq!(result, input);
    }

    #[test]
    fn test_scrub_selective() {
        let config = ScrubConfig {
            emails: true,
            api_keys: false,
            ip_addresses: false,
            file_paths: false,
            ..Default::default()
        };
        let input = "user@example.com at 10.0.1.42";
        let result = scrub(input, &config);
        assert!(!result.contains("user@example.com"));
        assert!(result.contains("10.0.1.42")); // not scrubbed
    }

    #[test]
    fn test_scrub_custom_pattern() {
        let config = ScrubConfig {
            custom_patterns: vec![r"SSN:\s*\d{3}-\d{2}-\d{4}".to_string()],
            ..Default::default()
        };
        let input = "Employee SSN: 123-45-6789 on file";
        let result = scrub(input, &config);
        assert!(!result.contains("123-45-6789"));
    }

    #[test]
    fn test_analyze_report() {
        let config = ScrubConfig::default();
        let input = "Email: a@b.com, key: api_key=abcdefghijklmnopqrst, IP: 10.0.0.1, path: /home/user/file";
        let report = analyze(input, &config);
        assert_eq!(report.emails_found, 1);
        assert_eq!(report.api_keys_found, 1);
        assert_eq!(report.ips_found, 1);
        assert_eq!(report.paths_found, 1);
    }

    #[test]
    fn test_custom_replacement() {
        let config = ScrubConfig {
            replacement: "***".to_string(),
            ..Default::default()
        };
        let input = "Contact user@example.com";
        let result = scrub(input, &config);
        assert!(result.contains("***"));
        assert!(!result.contains("[REDACTED]"));
    }
}
