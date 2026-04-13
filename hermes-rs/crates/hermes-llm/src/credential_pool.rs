//! Credential pool management.
//!
//! Multiple API keys per provider with rotation on failure.
//! Mirrors the Python credential pool system in `run_agent.py`.

use std::sync::atomic::{AtomicUsize, Ordering};

/// A single credential entry.
#[derive(Debug, Clone)]
pub struct Credential {
    pub api_key: String,
    pub base_url: Option<String>,
    pub label: Option<String>,
}

/// Pool of credentials for a single provider with round-robin rotation.
#[derive(Debug)]
pub struct CredentialPool {
    pub provider: String,
    credentials: Vec<Credential>,
    index: AtomicUsize,
}

impl CredentialPool {
    /// Create a new credential pool.
    pub fn new(provider: String, credentials: Vec<Credential>) -> Self {
        Self {
            provider,
            credentials,
            index: AtomicUsize::new(0),
        }
    }

    /// Select the next credential (round-robin).
    pub fn select(&self) -> Option<&Credential> {
        if self.credentials.is_empty() {
            return None;
        }
        let idx = self.index.fetch_add(1, Ordering::SeqCst) % self.credentials.len();
        self.credentials.get(idx)
    }

    /// Select the first credential without advancing.
    pub fn first(&self) -> Option<&Credential> {
        self.credentials.first()
    }

    /// Number of credentials in the pool.
    pub fn len(&self) -> usize {
        self.credentials.len()
    }

    /// Whether the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.credentials.is_empty()
    }

    /// Reset rotation index to 0.
    pub fn reset(&self) {
        self.index.store(0, Ordering::SeqCst);
    }
}

/// Load credentials from environment variables for a given provider.
pub fn load_from_env(provider: &str) -> Option<CredentialPool> {
    let (key_env, url_env) = match provider {
        "openrouter" => ("OPENROUTER_API_KEY", None),
        "nous" => ("NOUS_API_KEY", None),
        "openai" | "openai-codex" => ("OPENAI_API_KEY", Some("OPENAI_BASE_URL")),
        "anthropic" => ("ANTHROPIC_API_KEY", None),
        "gemini" => ("GEMINI_API_KEY", None),
        _ => return None,
    };

    let api_key = std::env::var(key_env).ok()?;
    let base_url = url_env.and_then(|e| std::env::var(e).ok());

    Some(CredentialPool::new(
        provider.to_string(),
        vec![Credential {
            api_key,
            base_url,
            label: Some("env".to_string()),
        }],
    ))
}

/// Build a credential pool from a list of entries.
pub fn from_entries(provider: &str, entries: Vec<serde_json::Value>) -> Option<CredentialPool> {
    let credentials: Vec<Credential> = entries
        .into_iter()
        .filter_map(|entry| {
            let obj = entry.as_object()?;
            let api_key = obj.get("api_key")?.as_str()?.to_string();
            let base_url = obj.get("base_url").and_then(|v| v.as_str()).map(String::from);
            let label = obj.get("label").and_then(|v| v.as_str()).map(String::from);
            Some(Credential {
                api_key,
                base_url,
                label,
            })
        })
        .collect();

    if credentials.is_empty() {
        return None;
    }

    Some(CredentialPool::new(provider.to_string(), credentials))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_round_robin() {
        let pool = CredentialPool::new("test".to_string(), vec![
            Credential { api_key: "key1".to_string(), base_url: None, label: None },
            Credential { api_key: "key2".to_string(), base_url: None, label: None },
            Credential { api_key: "key3".to_string(), base_url: None, label: None },
        ]);

        assert_eq!(pool.select().unwrap().api_key, "key1");
        assert_eq!(pool.select().unwrap().api_key, "key2");
        assert_eq!(pool.select().unwrap().api_key, "key3");
        // Wraps around
        assert_eq!(pool.select().unwrap().api_key, "key1");
    }

    #[test]
    fn test_pool_empty() {
        let pool = CredentialPool::new("test".to_string(), vec![]);
        assert!(pool.select().is_none());
        assert!(pool.is_empty());
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn test_pool_reset() {
        let pool = CredentialPool::new("test".to_string(), vec![
            Credential { api_key: "a".to_string(), base_url: None, label: None },
            Credential { api_key: "b".to_string(), base_url: None, label: None },
        ]);
        pool.select();
        pool.select();
        pool.reset();
        assert_eq!(pool.select().unwrap().api_key, "a");
    }

    #[test]
    fn test_from_entries() {
        let entries = vec![
            serde_json::json!({ "api_key": "key1", "label": "primary" }),
            serde_json::json!({ "api_key": "key2", "base_url": "https://custom.api.com" }),
        ];
        let pool = from_entries("openai", entries).unwrap();
        assert_eq!(pool.len(), 2);
        assert_eq!(pool.first().unwrap().api_key, "key1");
    }

    #[test]
    fn test_from_entries_empty() {
        let pool = from_entries("openai", vec![]);
        assert!(pool.is_none());
    }
}
