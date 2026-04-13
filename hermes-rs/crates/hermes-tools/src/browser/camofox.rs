//! Camofox REST API client.
//!
//! Mirrors the Python `tools/browser_camofox.py`.
//! Camofox is a local anti-detection browser (Firefox fork with C++
//! fingerprint spoofing) that exposes a REST API.

/// Camofox client configuration.
#[derive(Debug, Clone)]
pub struct CamofoxConfig {
    /// REST API base URL (e.g. `http://localhost:9377`).
    pub url: String,
}

impl Default for CamofoxConfig {
    fn default() -> Self {
        Self {
            url: std::env::var("CAMOFOX_URL")
                .unwrap_or_else(|_| "http://localhost:9377".to_string()),
        }
    }
}

/// Camofox session tracking info.
#[derive(Debug, Clone)]
pub struct CamofoxSession {
    pub user_id: String,
    pub tab_id: Option<String>,
    pub session_key: String,
}

/// Camofox REST API client.
pub struct CamofoxClient {
    config: CamofoxConfig,
    client: reqwest::Client,
}

impl CamofoxClient {
    pub fn new(config: CamofoxConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_env() -> Self {
        Self::new(CamofoxConfig::default())
    }

    /// Check if Camofox is available.
    pub fn is_configured(&self) -> bool {
        !self.config.url.is_empty()
    }

    /// Build the base URL.
    fn base(&self) -> &str {
        &self.config.url
    }

    /// Create a new tab. Returns `tabId`.
    pub async fn create_tab(&self, user_id: &str) -> Result<String, String> {
        let url = format!("{}/tabs", self.base());
        let resp = self.client.post(&url)
            .json(&serde_json::json!({ "userId": user_id }))
            .send()
            .await
            .map_err(|e| format!("Camofox: create tab failed: {e}"))?;

        let status = resp.status();
        let body = resp.text().await
            .map_err(|e| format!("Camofox: read response failed: {e}"))?;

        if !status.is_success() {
            return Err(format!("Camofox: create tab failed ({status}): {body}"));
        }

        let data: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| format!("Camofox: parse error: {e}"))?;

        data.get("tabId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("Camofox: no tabId in response: {body}"))
    }

    /// Navigate a tab to a URL.
    pub async fn navigate(&self, tab_id: &str, url: &str, user_id: &str) -> Result<String, String> {
        let endpoint = format!("{}/tabs/{tab_id}/navigate", self.base());
        let resp = self.client.post(&endpoint)
            .json(&serde_json::json!({ "userId": user_id, "url": url }))
            .send()
            .await
            .map_err(|e| format!("Camofox: navigate failed: {e}"))?;

        let status = resp.status();
        let body = resp.text().await
            .map_err(|e| format!("Camofox: read response failed: {e}"))?;

        if !status.is_success() {
            return Err(format!("Camofox: navigate failed ({status}): {body}"));
        }

        Ok(body)
    }

    /// Get accessibility tree snapshot.
    pub async fn snapshot(&self, tab_id: &str, user_id: &str) -> Result<String, String> {
        let endpoint = format!("{}/tabs/{tab_id}/snapshot?userId={user_id}", self.base());
        let resp = self.client.get(&endpoint)
            .send()
            .await
            .map_err(|e| format!("Camofox: snapshot failed: {e}"))?;

        let status = resp.status();
        let body = resp.text().await
            .map_err(|e| format!("Camofox: read response failed: {e}"))?;

        if !status.is_success() {
            return Err(format!("Camofox: snapshot failed ({status}): {body}"));
        }

        Ok(body)
    }

    /// Click element by ref.
    pub async fn click(&self, tab_id: &str, ref_id: &str, user_id: &str) -> Result<String, String> {
        let endpoint = format!("{}/tabs/{tab_id}/click", self.base());
        let resp = self.client.post(&endpoint)
            .json(&serde_json::json!({ "userId": user_id, "ref": ref_id }))
            .send()
            .await
            .map_err(|e| format!("Camofox: click failed: {e}"))?;

        let status = resp.status();
        let body = resp.text().await
            .map_err(|e| format!("Camofox: read response failed: {e}"))?;

        if !status.is_success() {
            return Err(format!("Camofox: click failed ({status}): {body}"));
        }

        Ok(body)
    }

    /// Type text into element.
    pub async fn type_text(&self, tab_id: &str, ref_id: &str, text: &str, user_id: &str) -> Result<String, String> {
        let endpoint = format!("{}/tabs/{tab_id}/type", self.base());
        let resp = self.client.post(&endpoint)
            .json(&serde_json::json!({ "userId": user_id, "ref": ref_id, "text": text }))
            .send()
            .await
            .map_err(|e| format!("Camofox: type failed: {e}"))?;

        let status = resp.status();
        let body = resp.text().await
            .map_err(|e| format!("Camofox: read response failed: {e}"))?;

        if !status.is_success() {
            return Err(format!("Camofox: type failed ({status}): {body}"));
        }

        Ok(body)
    }

    /// Scroll page.
    pub async fn scroll(&self, tab_id: &str, direction: &str, user_id: &str) -> Result<String, String> {
        let endpoint = format!("{}/tabs/{tab_id}/scroll", self.base());
        let resp = self.client.post(&endpoint)
            .json(&serde_json::json!({ "userId": user_id, "direction": direction }))
            .send()
            .await
            .map_err(|e| format!("Camofox: scroll failed: {e}"))?;

        let status = resp.status();
        let body = resp.text().await
            .map_err(|e| format!("Camofox: read response failed: {e}"))?;

        if !status.is_success() {
            return Err(format!("Camofox: scroll failed ({status}): {body}"));
        }

        Ok(body)
    }

    /// Go back in history.
    pub async fn back(&self, tab_id: &str, user_id: &str) -> Result<String, String> {
        let endpoint = format!("{}/tabs/{tab_id}/back", self.base());
        let resp = self.client.post(&endpoint)
            .json(&serde_json::json!({ "userId": user_id }))
            .send()
            .await
            .map_err(|e| format!("Camofox: back failed: {e}"))?;

        let status = resp.status();
        let body = resp.text().await
            .map_err(|e| format!("Camofox: read response failed: {e}"))?;

        if !status.is_success() {
            return Err(format!("Camofox: back failed ({status}): {body}"));
        }

        Ok(body)
    }

    /// Press keyboard key.
    pub async fn press(&self, tab_id: &str, key: &str, user_id: &str) -> Result<String, String> {
        let endpoint = format!("{}/tabs/{tab_id}/press", self.base());
        let resp = self.client.post(&endpoint)
            .json(&serde_json::json!({ "userId": user_id, "key": key }))
            .send()
            .await
            .map_err(|e| format!("Camofox: press failed: {e}"))?;

        let status = resp.status();
        let body = resp.text().await
            .map_err(|e| format!("Camofox: read response failed: {e}"))?;

        if !status.is_success() {
            return Err(format!("Camofox: press failed ({status}): {body}"));
        }

        Ok(body)
    }

    /// Take a screenshot.
    pub async fn screenshot(&self, tab_id: &str, user_id: &str) -> Result<Vec<u8>, String> {
        let endpoint = format!("{}/tabs/{tab_id}/screenshot?userId={user_id}", self.base());
        let resp = self.client.get(&endpoint)
            .send()
            .await
            .map_err(|e| format!("Camofox: screenshot failed: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Camofox: screenshot failed ({status}): {body}"));
        }

        resp.bytes().await
            .map(|b| b.to_vec())
            .map_err(|e| format!("Camofox: read screenshot failed: {e}"))
    }

    /// Health check — returns VNC port if available.
    pub async fn health(&self) -> Result<CamofoxHealth, String> {
        let url = format!("{}/health", self.base());
        let resp = self.client.get(&url)
            .send()
            .await
            .map_err(|e| format!("Camofox: health check failed: {e}"))?;

        let status = resp.status();
        let body = resp.text().await
            .map_err(|e| format!("Camofox: read health failed: {e}"))?;

        if !status.is_success() {
            return Err(format!("Camofox: health check failed ({status}): {body}"));
        }

        let data: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| format!("Camofox: parse error: {e}"))?;

        Ok(CamofoxHealth {
            vnc_port: data.get("vncPort").and_then(|v| v.as_u64()).map(|v| v as u16),
            raw: data,
        })
    }

    /// Close session (soft cleanup).
    pub async fn close_session(&self, user_id: &str) -> bool {
        let url = format!("{}/sessions/{user_id}", self.base());
        let resp = self.client.delete(&url).send().await;
        match resp {
            Ok(r) => r.status().is_success() || r.status().as_u16() == 404,
            Err(e) => {
                tracing::warn!("Camofox: close session failed: {e}");
                false
            }
        }
    }
}

/// Health check response.
#[derive(Debug)]
pub struct CamofoxHealth {
    pub vnc_port: Option<u16>,
    #[allow(dead_code)]
    pub raw: serde_json::Value,
}

/// Derive a deterministic user_id from a task_id and profile path.
/// Mirrors the Python `uuid5(NAMESPACE_URL, f"camofox-user:{state_dir}").hex[:10]`.
pub fn derive_user_id(task_id: &str, profile_path: Option<&str>) -> String {
    use sha2::{Digest, Sha256};

    let input = format!("camofox-user:{}:{}", profile_path.unwrap_or("default"), task_id);
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let hex = hex::encode(digest);
    format!("hermes_{}", &hex[..10])
}

/// Derive a session key from task_id.
pub fn derive_session_key(task_id: &str, profile_path: Option<&str>) -> String {
    use sha2::{Digest, Sha256};

    let input = format!("camofox-session:{}:{}", profile_path.unwrap_or("default"), task_id);
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let hex = hex::encode(digest);
    format!("task_{}", &hex[..16])
}

// Simple hex encoding since we already have sha2 dependency
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_user_id_deterministic() {
        let id1 = derive_user_id("task-1", None);
        let id2 = derive_user_id("task-1", None);
        assert_eq!(id1, id2);
        assert!(id1.starts_with("hermes_"));
    }

    #[test]
    fn test_derive_session_key_deterministic() {
        let key1 = derive_session_key("task-1", None);
        let key2 = derive_session_key("task-1", None);
        assert_eq!(key1, key2);
        assert!(key1.starts_with("task_"));
    }

    #[test]
    fn test_derive_different_tasks() {
        let id1 = derive_user_id("task-1", None);
        let id2 = derive_user_id("task-2", None);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_config_from_env() {
        let config = CamofoxConfig::default();
        let client = CamofoxClient::new(config);
        assert!(client.is_configured()); // default URL is non-empty
    }
}
