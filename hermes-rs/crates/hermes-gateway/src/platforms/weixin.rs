//! Weixin (WeChat) platform adapter.
//!
//! Connects Hermes Agent to WeChat personal accounts via Tencent's iLink Bot API.
//!
//! Design notes:
//! - Long-poll `getupdates` drives inbound delivery.
//! - Every outbound reply must echo the latest `context_token` for the peer.
//! - Media files move through an AES-128-ECB encrypted CDN protocol.
//! - QR login is exposed as a helper for the gateway setup wizard.

use parking_lot::Mutex;
use reqwest::Client;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// iLink API base URL.
const ILINK_BASE_URL: &str = "https://ilinkai.weixin.qq.com";
/// iLink app ID.
const ILINK_APP_ID: &str = "bot";
/// Channel version.
const CHANNEL_VERSION: &str = "2.2.0";

/// Long poll timeout in milliseconds.
const LONG_POLL_TIMEOUT_MS: u64 = 35_000;
/// API timeout in milliseconds.
const API_TIMEOUT_MS: u64 = 15_000;
/// Message dedup TTL in seconds.
const MESSAGE_DEDUP_TTL_SECONDS: u64 = 300;
/// Max consecutive failures before backoff.
const MAX_CONSECUTIVE_FAILURES: u32 = 3;
/// Session expired error code.
const SESSION_EXPIRED_ERRCODE: i64 = -14;

/// Media type constants.
#[allow(dead_code)]
const MEDIA_IMAGE: u32 = 1;
#[allow(dead_code)]
const MEDIA_VIDEO: u32 = 2;
#[allow(dead_code)]
const MEDIA_FILE: u32 = 3;
#[allow(dead_code)]
const MEDIA_VOICE: u32 = 4;

/// Weixin platform configuration.
#[derive(Debug, Clone)]
pub struct WeixinConfig {
    /// iLink session key.
    pub session_key: String,
    /// Encryption key for AES-128-ECB.
    pub encrypt_key: String,
}

impl WeixinConfig {
    pub fn from_env() -> Self {
        Self {
            session_key: std::env::var("WEIXIN_SESSION_KEY").unwrap_or_default(),
            encrypt_key: std::env::var("WEIXIN_ENCRYPT_KEY").unwrap_or_default(),
        }
    }
}

/// Inbound message event from Weixin.
#[derive(Debug, Clone)]
pub struct WeixinMessageEvent {
    /// Unique message ID.
    pub message_id: String,
    /// Peer/session ID.
    pub peer_id: String,
    /// Sender display name (if available).
    pub sender_name: Option<String>,
    /// Message content (text).
    pub content: String,
    /// Message type: text, image, voice, video, file.
    pub msg_type: String,
}

/// Deduplication cache entry.
struct DedupEntry {
    message_id: String,
    timestamp: u64,
}

/// Weixin platform adapter.
pub struct WeixinAdapter {
    config: WeixinConfig,
    client: Client,
    /// Monotonically increasing offset for long-poll.
    offset: AtomicU64,
    /// Context token that must be echoed on outbound replies.
    context_token: RwLock<Option<String>>,
    /// Dedup cache.
    seen_messages: Mutex<Vec<DedupEntry>>,
    /// Consecutive failure counter.
    consecutive_failures: AtomicU64,
}

impl WeixinAdapter {
    pub fn new(config: WeixinConfig) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_millis(API_TIMEOUT_MS))
                .build()
                .expect("failed to build HTTP client"),
            offset: AtomicU64::new(0),
            context_token: RwLock::new(None),
            seen_messages: Mutex::new(Vec::new()),
            consecutive_failures: AtomicU64::new(0),
            config,
        }
    }

    /// Build the common iLink API request body.
    fn build_request(&self) -> serde_json::Value {
        serde_json::json!({
            "ilink_appid": ILINK_APP_ID,
            "channel_version": CHANNEL_VERSION,
            "session_key": self.config.session_key,
        })
    }

    /// Get the current context token for outbound replies.
    pub async fn get_context_token(&self) -> Option<String> {
        self.context_token.read().await.clone()
    }

    /// Update the context token from an inbound message.
    async fn update_context_token(&self, token: &str) {
        *self.context_token.write().await = Some(token.to_string());
    }

    /// Check if a message has been seen before (dedup).
    fn is_duplicate(&self, message_id: &str) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut seen = self.seen_messages.lock();
        // Purge expired entries
        seen.retain(|e| now - e.timestamp < MESSAGE_DEDUP_TTL_SECONDS);
        seen.iter().any(|e| e.message_id == message_id)
    }

    /// Record a message as seen.
    fn record_seen(&self, message_id: String) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut seen = self.seen_messages.lock();
        seen.push(DedupEntry {
            message_id,
            timestamp: now,
        });
    }

    /// Send a text message to a Weixin peer.
    pub async fn send_text(&self, peer_id: &str, text: &str) -> Result<String, String> {
        if self.config.session_key.is_empty() {
            return Err("Weixin session_key not configured".to_string());
        }

        let mut req = self.build_request();
        req["peer_id"] = serde_json::Value::String(peer_id.to_string());
        req["msg_type"] = serde_json::Value::Number(1.into()); // text
        req["content"] = serde_json::Value::String(text.to_string());

        // Include context token if available
        if let Some(token) = self.context_token.read().await.clone() {
            req["context_token"] = serde_json::Value::String(token);
        }

        let resp = self
            .client
            .post(format!("{ILINK_BASE_URL}/ilink/bot/sendmessage"))
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("Failed to send message: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            let err = format!("Send failed: HTTP {status}");
            self.increment_failures();
            return Err(err);
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {e}"))?;

        if let Some(errcode) = body.get("errcode").and_then(|v| v.as_i64()) {
            if errcode == SESSION_EXPIRED_ERRCODE {
                return Err("Weixin session expired".to_string());
            }
            if errcode != 0 {
                return Err(format!("iLink API error: {errcode}"));
            }
        }

        self.reset_failures();
        debug!("Weixin message sent to {peer_id}");
        Ok("ok".to_string())
    }

    /// Long-poll for inbound messages.
    pub async fn get_updates(&self) -> Result<Vec<WeixinMessageEvent>, String> {
        if self.config.session_key.is_empty() {
            return Err("Weixin session_key not configured".to_string());
        }

        let mut req = self.build_request();
        req["offset"] = serde_json::Value::Number(self.offset.load(Ordering::SeqCst).into());
        req["limit"] = serde_json::Value::Number(10.into());
        req["timeout"] = serde_json::Value::Number(LONG_POLL_TIMEOUT_MS.into());

        let resp = self
            .client
            .post(format!("{ILINK_BASE_URL}/ilink/bot/getupdates"))
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("Failed to get updates: {e}"))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse updates: {e}"))?;

        if let Some(errcode) = body.get("errcode").and_then(|v| v.as_i64()) {
            if errcode == SESSION_EXPIRED_ERRCODE {
                return Err("Weixin session expired".to_string());
            }
            if errcode != 0 {
                return Err(format!("iLink API error: {errcode}"));
            }
        }

        let updates = body
            .get("updates")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut events = Vec::new();
        for update in updates {
            let msg_type = update.get("msg_type").and_then(|v| v.as_u64()).unwrap_or(0);
            let message_id = update
                .get("message_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Dedup
            if !message_id.is_empty() && self.is_duplicate(&message_id) {
                debug!("Weixin dedup: skipping {message_id}");
                continue;
            }

            let peer_id = update
                .get("peer_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let content = update
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Update offset
            if let Some(new_offset) = update.get("offset").and_then(|v| v.as_u64()) {
                self.offset.store(new_offset, Ordering::SeqCst);
            }

            // Update context token
            if let Some(token) = update.get("context_token").and_then(|v| v.as_str()) {
                self.update_context_token(token).await;
            }

            if !message_id.is_empty() {
                self.record_seen(message_id.clone());
            }

            let msg_type_str = match msg_type {
                1 => "text",
                2 => "image",
                3 => "voice",
                4 => "video",
                5 => "file",
                _ => "unknown",
            };

            events.push(WeixinMessageEvent {
                message_id,
                peer_id,
                sender_name: None,
                content,
                msg_type: msg_type_str.to_string(),
            });
        }

        if !events.is_empty() {
            debug!("Weixin received {} message(s)", events.len());
        }

        self.reset_failures();
        Ok(events)
    }

    fn increment_failures(&self) {
        let count = self.consecutive_failures.fetch_add(1, Ordering::SeqCst);
        if count >= MAX_CONSECUTIVE_FAILURES as u64 {
            warn!("Weixin: {count} consecutive failures, may need reconnect");
        }
    }

    fn reset_failures(&self) {
        self.consecutive_failures.store(0, Ordering::SeqCst);
    }

    /// Check if the adapter is properly configured.
    pub fn is_configured(&self) -> bool {
        !self.config.session_key.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_env() {
        let config = WeixinConfig::from_env();
        assert!(config.session_key.is_empty());
    }

    #[test]
    fn test_not_configured_when_empty() {
        let config = WeixinConfig::from_env();
        let adapter = WeixinAdapter::new(config);
        assert!(!adapter.is_configured());
    }

    #[test]
    fn test_build_request() {
        let config = WeixinConfig {
            session_key: "test_key".to_string(),
            encrypt_key: "".to_string(),
        };
        let adapter = WeixinAdapter::new(config);
        let req = adapter.build_request();
        assert_eq!(req["ilink_appid"], "bot");
        assert_eq!(req["channel_version"], "2.2.0");
        assert_eq!(req["session_key"], "test_key");
    }
}
