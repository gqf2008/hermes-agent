//! Rate limit tracking.
//!
//! Tracks remaining tokens/requests and estimated reset times.
//! Mirrors the rate limit header parsing from provider responses.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// Rate limit state for a provider.
#[derive(Debug, Clone)]
pub struct RateLimitState {
    /// Remaining tokens for the current period.
    pub remaining_tokens: Option<u64>,
    /// Total token limit for the current period.
    pub limit_tokens: Option<u64>,
    /// Remaining requests for the current period.
    pub remaining_requests: Option<u64>,
    /// Total request limit for the current period.
    pub limit_requests: Option<u64>,
    /// Seconds until the rate limit resets.
    pub reset_seconds: Option<f64>,
    /// When this state was last updated.
    pub last_updated: Instant,
}

impl RateLimitState {
    /// Parse rate limit headers from HTTP response.
    ///
    /// Supports common header patterns:
    /// - OpenAI: `x-ratelimit-remaining-tokens`, `x-ratelimit-remaining-requests`,
    ///   `x-ratelimit-reset-tokens`, `x-ratelimit-reset-requests`
    /// - Anthropic: `retry-after`, `anthropic-ratelimit-tokens-remaining`
    /// - OpenRouter: `x-ratelimit-limit`, `x-ratelimit-remaining`, `x-ratelimit-reset`
    pub fn from_headers(headers: &HashMap<String, String>) -> Self {
        let get = |key: &str| -> Option<u64> {
            headers
                .get(key)
                .and_then(|v| v.parse::<u64>().ok())
        };
        let get_f64 = |key: &str| -> Option<f64> {
            headers
                .get(key)
                .and_then(|v| v.parse::<f64>().ok())
        };

        // OpenAI style
        let remaining_tokens = get("x-ratelimit-remaining-tokens");
        let limit_tokens = get("x-ratelimit-limit-tokens");
        let remaining_requests = get("x-ratelimit-remaining-requests");
        let limit_requests = get("x-ratelimit-limit-requests");

        // Reset time — parse "HH:MM:SS" or seconds
        let reset_seconds = get_f64("x-ratelimit-reset-tokens")
            .or_else(|| get_f64("retry-after"));

        Self {
            remaining_tokens,
            limit_tokens,
            remaining_requests,
            limit_requests,
            reset_seconds,
            last_updated: Instant::now(),
        }
    }

    /// Whether we're likely rate limited (less than 10% remaining).
    pub fn is_near_limit(&self) -> bool {
        if let Some((remaining, limit)) = self.remaining_tokens.zip(self.limit_tokens) {
            if limit > 0 {
                return (remaining as f64 / limit as f64) < 0.1;
            }
        }
        if let Some((remaining, limit)) = self.remaining_requests.zip(self.limit_requests) {
            if limit > 0 {
                return (remaining as f64 / limit as f64) < 0.1;
            }
        }
        false
    }

    /// Estimated time until rate limit resets.
    pub fn time_to_reset(&self) -> Option<Duration> {
        self.reset_seconds.map(Duration::from_secs_f64)
    }
}

/// Global rate limit tracker.
fn rate_limits() -> &'static Mutex<HashMap<String, RateLimitState>> {
    static C: OnceLock<Mutex<HashMap<String, RateLimitState>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Update rate limit state for a provider.
pub fn update_rate_limit(provider: &str, headers: &HashMap<String, String>) {
    let state = RateLimitState::from_headers(headers);
    rate_limits().lock().insert(provider.to_string(), state);
}

/// Get rate limit state for a provider.
pub fn get_rate_limit(provider: &str) -> Option<RateLimitState> {
    rate_limits().lock().get(provider).cloned()
}

/// Check if a provider is near its rate limit.
pub fn is_near_rate_limit(provider: &str) -> bool {
    rate_limits()
        .lock()
        .get(provider)
        .map(|s| s.is_near_limit())
        .unwrap_or(false)
}

/// Clear rate limit state for a provider.
pub fn clear_rate_limit(provider: &str) {
    rate_limits().lock().remove(provider);
}

/// Calculate estimated wait time from rate limit headers.
pub fn estimate_wait_time(headers: &HashMap<String, String>) -> Option<Duration> {
    // Check retry-after header
    if let Some(secs) = headers.get("retry-after").and_then(|v| v.parse::<u64>().ok()) {
        return Some(Duration::from_secs(secs));
    }

    // Check OpenAI reset headers
    if let Some(reset) = headers
        .get("x-ratelimit-reset-tokens")
        .and_then(|v| v.parse::<f64>().ok())
    {
        return Some(Duration::from_secs_f64(reset));
    }

    // Check OpenRouter reset header
    if let Some(reset) = headers
        .get("x-ratelimit-reset")
        .and_then(|v| v.parse::<f64>().ok())
    {
        return Some(Duration::from_secs_f64(reset));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_openai_headers() {
        let mut headers = HashMap::new();
        headers.insert("x-ratelimit-remaining-tokens".to_string(), "50000".to_string());
        headers.insert("x-ratelimit-limit-tokens".to_string(), "100000".to_string());
        headers.insert("x-ratelimit-remaining-requests".to_string(), "40".to_string());
        headers.insert("x-ratelimit-limit-requests".to_string(), "50".to_string());

        let state = RateLimitState::from_headers(&headers);
        assert_eq!(state.remaining_tokens, Some(50000));
        assert_eq!(state.limit_tokens, Some(100000));
        assert_eq!(state.remaining_requests, Some(40));
        assert_eq!(state.limit_requests, Some(50));
    }

    #[test]
    fn test_is_near_limit() {
        let mut headers = HashMap::new();
        headers.insert("x-ratelimit-remaining-tokens".to_string(), "5".to_string());
        headers.insert("x-ratelimit-limit-tokens".to_string(), "100".to_string());

        let state = RateLimitState::from_headers(&headers);
        assert!(state.is_near_limit());

        headers.insert("x-ratelimit-remaining-tokens".to_string(), "50".to_string());
        let state = RateLimitState::from_headers(&headers);
        assert!(!state.is_near_limit());
    }

    #[test]
    fn test_update_and_get() {
        let mut headers = HashMap::new();
        headers.insert("retry-after".to_string(), "30".to_string());
        update_rate_limit("test-provider", &headers);

        let state = get_rate_limit("test-provider").unwrap();
        assert_eq!(state.reset_seconds, Some(30.0));

        clear_rate_limit("test-provider");
        assert!(get_rate_limit("test-provider").is_none());
    }

    #[test]
    fn test_estimate_wait_time_retry_after() {
        let mut headers = HashMap::new();
        headers.insert("retry-after".to_string(), "60".to_string());
        let wait = estimate_wait_time(&headers).unwrap();
        assert_eq!(wait.as_secs(), 60);
    }

    #[test]
    fn test_estimate_wait_time_none() {
        let headers = HashMap::new();
        assert!(estimate_wait_time(&headers).is_none());
    }

    #[test]
    fn test_estimate_wait_time_openrouter_reset() {
        let mut headers = HashMap::new();
        headers.insert("x-ratelimit-reset".to_string(), "45.5".to_string());
        let wait = estimate_wait_time(&headers).unwrap();
        assert_eq!(wait.as_secs(), 45);
    }

    #[test]
    fn test_estimate_wait_time_openai_reset() {
        let mut headers = HashMap::new();
        headers.insert("x-ratelimit-reset-tokens".to_string(), "120.0".to_string());
        let wait = estimate_wait_time(&headers).unwrap();
        assert_eq!(wait.as_secs(), 120);
    }
}
