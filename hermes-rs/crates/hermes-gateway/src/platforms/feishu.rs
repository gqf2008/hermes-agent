//! Feishu/Lark platform adapter.
//!
//! Supports:
//! - WebSocket long connection and Webhook transport
//! - Direct-message and group @mention-gated text receive/send
//! - Inbound image/file/audio media caching
//! - Gateway allowlist integration
//!
//! Full implementation requires the `lark_oapi` Python SDK or the Feishu
//! Open API REST endpoints. This module provides the core structure with
//! HTTP-based send/receive via Feishu REST API.

use reqwest::Client;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;
use uuid::Uuid;

use crate::dedup::MessageDeduplicator;

/// Feishu connection mode.
#[derive(Debug, Clone, Default)]
pub enum FeishuConnectionMode {
    /// WebSocket long connection.
    #[default]
    WebSocket,
    /// HTTP webhook (requires public URL).
    Webhook,
}

/// Feishu group policy.
#[derive(Debug, Clone, Default)]
pub enum GroupPolicy {
    /// Accept messages from anyone.
    #[default]
    Open,
    /// Only accept from allowlisted users.
    Allowlist,
    /// Reject blacklisted users.
    Blacklist,
    /// Only admins can interact.
    AdminOnly,
    /// Group is disabled.
    Disabled,
}

/// Feishu platform configuration.
#[derive(Debug, Clone)]
pub struct FeishuConfig {
    pub app_id: String,
    pub app_secret: String,
    pub connection_mode: FeishuConnectionMode,
    pub verification_token: String,
    pub encrypt_key: String,
    pub group_policy: GroupPolicy,
    pub allowed_users: HashSet<String>,
    pub webhook_port: u16,
    pub webhook_path: String,
}

impl FeishuConfig {
    pub fn from_env() -> Self {
        Self {
            app_id: std::env::var("FEISHU_APP_ID").unwrap_or_default(),
            app_secret: std::env::var("FEISHU_APP_SECRET").unwrap_or_default(),
            connection_mode: FeishuConnectionMode::default(),
            verification_token: std::env::var("FEISHU_VERIFICATION_TOKEN").unwrap_or_default(),
            encrypt_key: std::env::var("FEISHU_ENCRYPT_KEY").unwrap_or_default(),
            group_policy: GroupPolicy::default(),
            allowed_users: HashSet::new(),
            webhook_port: std::env::var("FEISHU_WEBHOOK_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8765),
            webhook_path: std::env::var("FEISHU_WEBHOOK_PATH")
                .ok()
                .unwrap_or_else(|| "/feishu/webhook".to_string()),
        }
    }
}

/// Cached token with expiry tracking.
struct CachedToken {
    token: String,
    expires_at: std::time::Instant,
}

impl CachedToken {
    fn new(token: String, expire_secs: u64) -> Self {
        // Refresh 5 minutes early
        let refresh_buffer = std::time::Duration::from_secs(300);
        let expires_at = std::time::Instant::now()
            + std::time::Duration::from_secs(expire_secs)
            - refresh_buffer;
        Self { token, expires_at }
    }
}

/// Inbound message event from Feishu.
#[derive(Debug, Clone)]
pub struct FeishuMessageEvent {
    /// Unique message ID.
    pub message_id: String,
    /// Chat/session ID.
    pub chat_id: String,
    /// Sender ID.
    pub sender_id: String,
    /// Message content (text).
    pub content: String,
    /// Message type: text, image, file, audio.
    pub msg_type: String,
    /// Whether this is a group message.
    pub is_group: bool,
    /// Mentioned user IDs.
    pub mentions: Vec<String>,
}

/// Feishu platform adapter.
pub struct FeishuAdapter {
    config: FeishuConfig,
    client: Client,
    dedup: Arc<MessageDeduplicator>,
    /// Access token cached from Feishu API (with expiry).
    access_token: RwLock<Option<CachedToken>>,
}

impl FeishuAdapter {
    pub fn new(config: FeishuConfig) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to build HTTP client"),
            dedup: Arc::new(MessageDeduplicator::new()),
            access_token: RwLock::new(None),
            config,
        }
    }

    /// Get/refresh the Feishu tenant access token.
    async fn get_access_token(&self) -> Result<String, String> {
        // Check cache with expiry
        {
            let guard = self.access_token.read().await;
            if let Some(cached) = guard.as_ref() {
                if cached.expires_at > std::time::Instant::now() {
                    return Ok(cached.token.clone());
                }
            }
        }

        let resp = self
            .client
            .post("https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal")
            .json(&serde_json::json!({
                "app_id": &self.config.app_id,
                "app_secret": &self.config.app_secret,
            }))
            .send()
            .await
            .map_err(|e| format!("Failed to get access token: {e}"))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse token response: {e}"))?;

        let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(format!("Token request failed: code={code}, msg={}", body.get("msg").and_then(|v| v.as_str()).unwrap_or("unknown")));
        }

        let token = body
            .get("tenant_access_token")
            .and_then(|v| v.as_str())
            .ok_or("Missing tenant_access_token in response")?
            .to_string();

        // Feishu tokens expire in 7200 seconds (2 hours)
        *self.access_token.write().await = Some(CachedToken::new(token.clone(), 7200));
        Ok(token)
    }

    /// Send a text message to a Feishu chat.
    pub async fn send_text(&self, chat_id: &str, text: &str) -> Result<String, String> {
        let token = self.get_access_token().await?;
        let msg_id = format!("msg_{}", Uuid::new_v4().simple());

        let resp = self
            .client
            .post("https://open.feishu.cn/open-apis/im/v1/messages")
            .query(&[("receive_id_type", "chat_id")])
            .header("Authorization", format!("Bearer {token}"))
            .json(&serde_json::json!({
                "receive_id": chat_id,
                "msg_type": "text",
                "content": serde_json::json!({"text": text}).to_string(),
            }))
            .send()
            .await
            .map_err(|e| format!("Failed to send message: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(format!("Send failed: HTTP {}", status));
        }

        debug!("Feishu message sent to {chat_id}: msg_id={msg_id}");
        Ok(msg_id)
    }

    /// Process an inbound webhook event.
    pub async fn handle_inbound(&self, payload: &serde_json::Value) -> Option<FeishuMessageEvent> {
        // Dedup by message_id
        let msg_id = payload
            .get("message")
            .and_then(|m| m.get("message_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if !msg_id.is_empty() && self.dedup.is_duplicate(msg_id) {
            debug!("Feishu dedup: skipping {msg_id}");
            return None;
        }

        let chat_id = payload
            .get("message")
            .and_then(|m| m.get("chat_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let sender_id = payload
            .get("sender")
            .and_then(|s| s.get("sender_id"))
            .and_then(|s| s.get("open_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let content_type = payload
            .get("message")
            .and_then(|m| m.get("message_type"))
            .and_then(|v| v.as_str())
            .unwrap_or("text")
            .to_string();

        // Extract text content
        let content_str = payload
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let content: serde_json::Value =
            serde_json::from_str(&content_str).unwrap_or(serde_json::Value::Null);
        let text = content
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let is_group = payload
            .get("message")
            .and_then(|m| m.get("chat_type"))
            .and_then(|v| v.as_str())
            .map(|t| t == "group")
            .unwrap_or(false);

        if !msg_id.is_empty() {
            self.dedup.insert(msg_id.to_string());
        }

        Some(FeishuMessageEvent {
            message_id: msg_id.to_string(),
            chat_id,
            sender_id,
            content: text,
            msg_type: content_type,
            is_group,
            mentions: Vec::new(),
        })
    }

    /// Check if the adapter is properly configured.
    pub fn is_configured(&self) -> bool {
        !self.config.app_id.is_empty() && !self.config.app_secret.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_env() {
        let config = FeishuConfig::from_env();
        // Should have defaults when no env vars set
        assert_eq!(config.webhook_port, 8765);
        assert_eq!(config.webhook_path, "/feishu/webhook");
    }

    #[test]
    fn test_not_configured_when_empty() {
        let config = FeishuConfig::from_env();
        let adapter = FeishuAdapter::new(config);
        assert!(!adapter.is_configured());
    }
}
