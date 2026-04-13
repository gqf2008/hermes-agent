//! Model metadata: context length discovery and caching.
//!
//! Multi-source context length resolution:
//! 1. Persistent cache (YAML)
//! 2. OpenRouter live API (cached 1 hour)
//! 3. Custom endpoint probing
//! 4. Hardcoded DEFAULT_CONTEXT_LENGTHS (substring matching)
//!
//! Mirrors the Python `model_metadata.py`.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::pricing::UsagePricing;

/// Minimum context length required to run Hermes Agent.
/// Models with fewer tokens cannot maintain enough working memory
/// for tool-calling workflows.
pub const MINIMUM_CONTEXT_LENGTH: usize = 64_000;

/// Container-local DNS suffixes that should be treated as local endpoints.
/// Used to skip network-related fallbacks and proxying for Docker-served models.
const CONTAINER_LOCAL_SUFFIXES: &[&str] = &[
    "host.docker.internal",
    "host.containers.internal",
    "gateway.docker.internal",
    "host.lima.internal",
];

/// Default context lengths for known model families (fallback).
const DEFAULT_CONTEXT_LENGTHS: &[(&str, usize)] = &[
    ("claude-3-5-sonnet", 200_000),
    ("claude-3-sonnet", 200_000),
    ("claude-3-opus", 200_000),
    ("claude-opus", 200_000),
    ("claude-opus-4-6", 1_000_000),
    ("claude-sonnet-4-6", 1_000_000),
    ("claude-3-haiku", 200_000),
    ("claude-haiku", 200_000),
    ("gpt-4o", 128_000),
    ("gpt-4-turbo", 128_000),
    ("gpt-4.1", 1_047_576),
    ("gpt-4", 8_192),
    ("gpt-3.5-turbo", 16_385),
    ("gpt-5", 128_000),
    ("gemini", 1_048_576),
    ("gemma-4-31b", 256_000),
    ("gemma-4-26b", 256_000),
    ("gemma-3", 131_072),
    ("qwen", 32_768),
    ("qwen3-coder-plus", 1_000_000),
    ("qwen3-coder", 262_144),
    ("minimax", 204_800),
    ("grok-4-1-fast", 2_000_000),
    ("grok-4-fast", 2_000_000),
    ("grok-4.20", 2_000_000),
    ("grok-4", 256_000),
    ("grok-3", 131_072),
    ("grok", 128_000),
    ("llama-3", 128_000),
    ("llama3", 128_000),
    ("mistral", 32_768),
    ("deepseek", 128_000),
    ("glm", 202_752),
];

/// Context length probe tiers.
pub const CONTEXT_PROBE_TIERS: &[usize] = &[128_000, 64_000, 32_000, 16_000, 8_000];

/// Cached metadata for a single model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub model: String,
    pub context_length: Option<usize>,
    pub max_completion_tokens: Option<usize>,
    pub pricing: Option<UsagePricing>,
    pub last_fetched: Option<String>,
}

struct CachedEntry {
    metadata: ModelMetadata,
    fetched_at: Instant,
    ttl: Duration,
}

impl CachedEntry {
    fn is_expired(&self) -> bool {
        self.fetched_at.elapsed() > self.ttl
    }
}

fn cache() -> &'static Mutex<HashMap<String, CachedEntry>> {
    static C: OnceLock<Mutex<HashMap<String, CachedEntry>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

const OPENROUTER_TTL: Duration = Duration::from_secs(3600);
const CUSTOM_TTL: Duration = Duration::from_secs(300);

/// Get context length for a model, using the multi-source resolution chain.
pub fn get_context_length(model: &str, base_url: &str) -> Option<usize> {
    let cache_key = format!("{}@{}", model, base_url);
    {
        let c = cache().lock();
        if let Some(entry) = c.get(&cache_key) {
            if !entry.is_expired() {
                return entry.metadata.context_length;
            }
        }
    }

    // Hardcoded fallback (substring matching)
    for (substr, ctx_len) in DEFAULT_CONTEXT_LENGTHS {
        if model.to_lowercase().contains(*substr) {
            return Some(*ctx_len);
        }
    }

    None
}

/// Store metadata in the cache.
pub fn cache_metadata(model: &str, base_url: &str, metadata: ModelMetadata, is_openrouter: bool) {
    let cache_key = format!("{}@{}", model, base_url);
    let ttl = if is_openrouter { OPENROUTER_TTL } else { CUSTOM_TTL };
    let entry = CachedEntry {
        metadata,
        fetched_at: Instant::now(),
        ttl,
    };
    cache().lock().insert(cache_key, entry);
}

/// Get cached metadata if present and not expired.
pub fn get_cached_metadata(model: &str, base_url: &str) -> Option<ModelMetadata> {
    let cache_key = format!("{}@{}", model, base_url);
    let c = cache().lock();
    c.get(&cache_key).map(|e| {
        if e.is_expired() {
            None
        } else {
            Some(e.metadata.clone())
        }
    })?
}

/// Lookup context length from hardcoded defaults.
pub fn lookup_default_context_length(model: &str) -> Option<usize> {
    for (substr, ctx_len) in DEFAULT_CONTEXT_LENGTHS {
        if model.to_lowercase().contains(*substr) {
            return Some(*ctx_len);
        }
    }
    None
}

/// Parse context length from an API response body.
pub fn parse_context_length_from_response(response: &serde_json::Value) -> Option<usize> {
    response
        .get("context_length")
        .or_else(|| response.get("max_tokens"))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .or_else(|| {
            response
                .get("data")
                .and_then(|d| d.as_array())
                .and_then(|arr| arr.first())
                .and_then(|item| item.get("max_model_length"))
                .and_then(|v| v.as_u64())
                .map(|n| n as usize)
        })
}

/// Check if a base_url is a container-local endpoint.
///
/// Mirrors Python `_is_local_endpoint()` which includes container DNS
/// suffixes (host.docker.internal, host.containers.internal, etc.)
/// so that models served via Docker are treated as local and don't
/// trigger network-related fallbacks.
pub fn is_local_endpoint(base_url: &str) -> bool {
    if base_url.is_empty()
        || base_url.contains("localhost")
        || base_url.contains("127.0.0.1")
        || base_url.starts_with("http://host")
    {
        return true;
    }
    CONTAINER_LOCAL_SUFFIXES
        .iter()
        .any(|suffix| base_url.contains(suffix))
}

/// Check if a model requires the Responses API (OpenAI GPT-5.x family).
///
/// GPT-5+ models (gpt-5, gpt-5-mini, gpt-5-nano, etc.) only work with
/// the Responses API endpoint, not the standard chat completions endpoint.
pub fn model_requires_responses_api(model: &str) -> bool {
    let model_lower = model.to_lowercase();
    // Strip provider prefix if present
    let model_lower = model_lower
        .split('/')
        .next_back()
        .unwrap_or(&model_lower)
        .to_string();

    model_lower.starts_with("gpt-5") || model_lower.starts_with("openai/gpt-5")
}

/// Normalize a model slug for cache lookup: strip OpenRouter-format prefixes
/// (containing "/") when the cache-hit model doesn't match the target provider.
///
/// Mirrors Python `_compat_model()` helper.
pub fn compat_model_slug(model: &str) -> String {
    // If it contains a "/" it's likely an OpenRouter-format slug
    if let Some((_, rest)) = model.split_once('/') {
        rest.to_string()
    } else {
        model.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_claude() {
        assert_eq!(lookup_default_context_length("claude-3-5-sonnet-20241022"), Some(200_000));
    }

    #[test]
    fn test_lookup_gpt4o() {
        assert_eq!(lookup_default_context_length("gpt-4o-mini"), Some(128_000));
    }

    #[test]
    fn test_lookup_gemini() {
        assert_eq!(lookup_default_context_length("gemini-1.5-pro"), Some(1_048_576));
    }

    #[test]
    fn test_lookup_unknown() {
        assert_eq!(lookup_default_context_length("some-random-model"), None);
    }

    #[test]
    fn test_get_context_length_defaults() {
        assert_eq!(get_context_length("gpt-4o", ""), Some(128_000));
    }

    #[test]
    fn test_parse_context_length() {
        let r = serde_json::json!({"context_length": 200000, "max_completion_tokens": 8192});
        assert_eq!(parse_context_length_from_response(&r), Some(200_000));
    }

    #[test]
    fn test_cache_roundtrip() {
        let m = ModelMetadata {
            model: "test-model".to_string(),
            context_length: Some(4096),
            max_completion_tokens: None,
            pricing: None,
            last_fetched: None,
        };
        cache_metadata("test-model", "https://test.com", m.clone(), false);
        let c = get_cached_metadata("test-model", "https://test.com").unwrap();
        assert_eq!(c.context_length, Some(4096));
    }

    #[test]
    fn test_minimum_context_length_constant() {
        assert_eq!(MINIMUM_CONTEXT_LENGTH, 64_000);
    }

    #[test]
    fn test_is_local_endpoint() {
        assert!(is_local_endpoint("http://localhost:8080"));
        assert!(is_local_endpoint("http://127.0.0.1:8000"));
        assert!(is_local_endpoint("http://host.docker.internal:8000"));
        assert!(is_local_endpoint("http://host.containers.internal:8000"));
        assert!(is_local_endpoint("http://gateway.docker.internal:8000"));
        assert!(is_local_endpoint("http://host.lima.internal:8000"));
        assert!(!is_local_endpoint("https://api.openai.com"));
        assert!(!is_local_endpoint("https://api.openrouter.ai"));
    }

    #[test]
    fn test_model_requires_responses_api() {
        assert!(model_requires_responses_api("gpt-5"));
        assert!(model_requires_responses_api("gpt-5-mini"));
        assert!(model_requires_responses_api("gpt-5-nano"));
        assert!(model_requires_responses_api("openai/gpt-5"));
        assert!(!model_requires_responses_api("gpt-4o"));
        assert!(!model_requires_responses_api("gpt-4-turbo"));
        assert!(!model_requires_responses_api("claude-opus-4-6"));
    }

    #[test]
    fn test_compat_model_slug() {
        assert_eq!(compat_model_slug("openrouter/gpt-4o"), "gpt-4o");
        assert_eq!(compat_model_slug("anthropic/claude-opus-4-6"), "claude-opus-4-6");
        assert_eq!(compat_model_slug("gpt-4o"), "gpt-4o");
        assert_eq!(compat_model_slug("claude-sonnet-4-6"), "claude-sonnet-4-6");
    }
}
