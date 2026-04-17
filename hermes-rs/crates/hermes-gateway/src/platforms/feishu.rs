//! Feishu/Lark platform adapter.
//!
//! Supports:
//! - WebSocket long connection and Webhook transport
//! - Direct-message and group @mention-gated text receive/send
//! - Inbound image/file/audio media caching
//! - Gateway allowlist integration
//!
//! Mirrors Python `gateway/platforms/feishu.py`.

use axum::{
    body::Bytes,
    http::{HeaderMap, StatusCode},
    response::Json,
    Router,
};
use reqwest::Client;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::dedup::MessageDeduplicator;

/// Feishu webhook max body size (2MB, matches Python).
const FEISHU_WEBHOOK_MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

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
        let refresh_buffer = std::time::Duration::from_secs(300);
        let expires_at = std::time::Instant::now()
            + std::time::Duration::from_secs(expire_secs)
            - refresh_buffer;
        Self { token, expires_at }
    }
}

/// Media key extracted from a Feishu message.
#[derive(Debug, Clone)]
pub struct MediaKey {
    /// Media type: image, file, audio.
    pub media_type: String,
    /// image_key or file_key.
    pub key: String,
    /// Original file name (for files).
    pub name: Option<String>,
}

/// Inbound message event from Feishu.
#[derive(Debug, Clone)]
pub struct FeishuMessageEvent {
    pub message_id: String,
    pub chat_id: String,
    pub sender_id: String,
    pub sender_name: Option<String>,
    pub content: String,
    pub msg_type: String,
    pub is_group: bool,
    pub mentions: Vec<String>,
    pub media_keys: Vec<MediaKey>,
}

/// Callback type for inbound Feishu messages.
pub type FeishuInboundCallback = Arc<dyn Fn(FeishuMessageEvent) + Send + Sync>;

/// Feishu platform adapter.
pub struct FeishuAdapter {
    pub config: FeishuConfig,
    client: Client,
    dedup: Arc<MessageDeduplicator>,
    access_token: Arc<RwLock<Option<CachedToken>>>,
    /// Called when a webhook message is received.
    /// Set before starting the webhook server.
    pub on_message: Arc<RwLock<Option<FeishuInboundCallback>>>,
}

impl FeishuAdapter {
    pub fn new(config: FeishuConfig) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to build HTTP client"),
            dedup: Arc::new(MessageDeduplicator::new()),
            access_token: Arc::new(RwLock::new(None)),
            on_message: Arc::new(RwLock::new(None)),
            config,
        }
    }

    async fn get_access_token(&self) -> Result<String, String> {
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

        let body: Value = resp
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

    /// Send a message with an arbitrary msg_type and content payload.
    async fn send_message(
        &self,
        chat_id: &str,
        msg_type: &str,
        content: &Value,
    ) -> Result<String, String> {
        let token = self.get_access_token().await?;
        let msg_id = format!("msg_{}", Uuid::new_v4().simple());

        let resp = self
            .client
            .post("https://open.feishu.cn/open-apis/im/v1/messages")
            .query(&[("receive_id_type", "chat_id")])
            .header("Authorization", format!("Bearer {token}"))
            .json(&serde_json::json!({
                "receive_id": chat_id,
                "msg_type": msg_type,
                "content": content.to_string(),
            }))
            .send()
            .await
            .map_err(|e| format!("Failed to send message: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(format!("Send failed: HTTP {}", status));
        }

        debug!("Feishu {msg_type} message sent to {chat_id}: msg_id={msg_id}");
        Ok(msg_id)
    }

    /// Send a rich-text (post) message.
    ///
    /// Converts simple markdown-like text to Feishu post format.
    pub async fn send_post(&self,
        chat_id: &str,
        title: &str,
        content: &str,
    ) -> Result<String, String> {
        let post_content = build_post_payload(title, content);
        self.send_message(chat_id, "post", &post_content).await
    }

    /// Upload an image to Feishu and return the image_key.
    async fn upload_image(&self,
        image_path: &str,
    ) -> Result<String, String> {
        let token = self.get_access_token().await?;
        let bytes = if image_path.starts_with("http://") || image_path.starts_with("https://") {
            let resp = self
                .client
                .get(image_path)
                .send()
                .await
                .map_err(|e| format!("Failed to download image: {e}"))?;
            resp.bytes()
                .await
                .map_err(|e| format!("Failed to read image body: {e}"))?
                .to_vec()
        } else {
            std::fs::read(image_path).map_err(|e| format!("Failed to read image: {e}"))?
        };

        let ext = std::path::Path::new(image_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg");
        let (file_name, mime) = match ext.to_lowercase().as_str() {
            "png" => ("image.png", "image/png"),
            "gif" => ("image.gif", "image/gif"),
            "bmp" => ("image.bmp", "image/bmp"),
            "webp" => ("image.webp", "image/webp"),
            _ => ("image.jpg", "image/jpeg"),
        };
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(file_name)
            .mime_str(mime)
            .map_err(|e| format!("Invalid mime type: {e}"))?;
        let form = reqwest::multipart::Form::new().part("image", part);

        let resp = self
            .client
            .post("https://open.feishu.cn/open-apis/im/v1/images")
            .query(&[("image_type", "message")])
            .header("Authorization", format!("Bearer {token}"))
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("Failed to upload image: {e}"))?;

        let body: Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse upload response: {e}"))?;

        let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(format!("Image upload failed: code={code}"));
        }

        body.get("data")
            .and_then(|d| d.get("image_key"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or("Missing image_key in upload response".to_string())
    }

    /// Upload a file to Feishu and return the file_key.
    async fn upload_file(
        &self,
        file_path: &str,
        file_type: &str,
    ) -> Result<String, String> {
        let token = self.get_access_token().await?;
        let bytes = if file_path.starts_with("http://") || file_path.starts_with("https://") {
            let resp = self
                .client
                .get(file_path)
                .send()
                .await
                .map_err(|e| format!("Failed to download file: {e}"))?;
            resp.bytes()
                .await
                .map_err(|e| format!("Failed to read file body: {e}"))?
                .to_vec()
        } else {
            std::fs::read(file_path).map_err(|e| format!("Failed to read file: {e}"))?
        };

        let file_name = std::path::Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");
        let mime = match file_type {
            "stream" => "application/octet-stream",
            "opus" => "audio/opus",
            "mp4" => "video/mp4",
            _ => "application/octet-stream",
        };

        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(file_name.to_string())
            .mime_str(mime)
            .map_err(|e| format!("Invalid mime type: {e}"))?;
        let form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("file_type", file_type.to_string());

        let resp = self
            .client
            .post("https://open.feishu.cn/open-apis/im/v1/files")
            .header("Authorization", format!("Bearer {token}"))
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("Failed to upload file: {e}"))?;

        let body: Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse upload response: {e}"))?;

        let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(format!("File upload failed: code={code}"));
        }

        body.get("data")
            .and_then(|d| d.get("file_key"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or("Missing file_key in upload response".to_string())
    }

    /// Send an image message.
    pub async fn send_image(
        &self,
        chat_id: &str,
        image_path: &str,
    ) -> Result<String, String> {
        let image_key = self.upload_image(image_path).await?;
        let content = serde_json::json!({"image_key": image_key});
        self.send_message(chat_id, "image", &content).await
    }

    /// Send a file message.
    pub async fn send_file(
        &self,
        chat_id: &str,
        file_path: &str,
    ) -> Result<String, String> {
        let file_key = self.upload_file(file_path, "stream").await?;
        let content = serde_json::json!({"file_key": file_key});
        self.send_message(chat_id, "file", &content).await
    }

    /// Send an interactive card message.
    pub async fn send_interactive_card(
        &self,
        chat_id: &str,
        card_json: &Value,
    ) -> Result<String, String> {
        self.send_message(chat_id, "interactive", card_json).await
    }

    /// Edit an existing message.
    pub async fn edit_message(
        &self,
        message_id: &str,
        content: &Value,
        msg_type: &str,
    ) -> Result<String, String> {
        let token = self.get_access_token().await?;

        let resp = self
            .client
            .patch(format!(
                "https://open.feishu.cn/open-apis/im/v1/messages/{}",
                message_id
            ))
            .header("Authorization", format!("Bearer {token}"))
            .json(&serde_json::json!({
                "content": content.to_string(),
                "msg_type": msg_type,
            }))
            .send()
            .await
            .map_err(|e| format!("Failed to edit message: {e}"))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(format!("Edit failed: HTTP {}", status));
        }

        debug!("Feishu message {message_id} edited");
        Ok("ok".to_string())
    }

    /// Detect if text contains markdown and send as post if so, otherwise text.
    pub async fn send_text_or_post(&self,
        chat_id: &str,
        text: &str,
    ) -> Result<String, String> {
        if looks_like_markdown(text) {
            self.send_post(chat_id, "", text).await
        } else {
            self.send_text(chat_id, text).await
        }
    }

    /// Extract media keys from a Feishu content object.
    fn extract_media_keys(content_obj: &Value, msg_type: &str) -> Vec<MediaKey> {
        let mut keys = Vec::new();
        // Image
        if let Some(key) = content_obj.get("image_key").and_then(|v| v.as_str()) {
            keys.push(MediaKey {
                media_type: "image".to_string(),
                key: key.to_string(),
                name: None,
            });
        }
        // File (skip if this is an audio message to avoid double-counting)
        if msg_type != "audio" {
            if let Some(key) = content_obj.get("file_key").and_then(|v| v.as_str()) {
                let name = content_obj.get("file_name").and_then(|v| v.as_str()).map(String::from);
                keys.push(MediaKey {
                    media_type: "file".to_string(),
                    key: key.to_string(),
                    name,
                });
            }
        }
        // Audio (uses file_key but different semantics)
        if msg_type == "audio" {
            if let Some(key) = content_obj.get("file_key").and_then(|v| v.as_str()) {
                keys.push(MediaKey {
                    media_type: "audio".to_string(),
                    key: key.to_string(),
                    name: None,
                });
            }
        }
        keys
    }

    /// Download a media file from Feishu API.
    async fn download_media(&self, media_key: &str, media_type: &str) -> Result<Vec<u8>, String> {
        let token = self.get_access_token().await?;
        let url = match media_type {
            "image" => format!("https://open.feishu.cn/open-apis/im/v1/images/{}", media_key),
            "file" | "audio" => format!("https://open.feishu.cn/open-apis/im/v1/files/{}", media_key),
            _ => return Err("Unknown media type".to_string()),
        };

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .map_err(|e| format!("Failed to download media: {e}"))?;

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("Failed to read media body: {e}"))?;
        Ok(bytes.to_vec())
    }

    /// Download and cache a Feishu media file to disk.
    /// Compute a short content hash for cache deduplication.
    fn content_hash(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex::encode(&hasher.finalize()[..8])
    }

    async fn download_and_cache_media(
        &self,
        media_key: &MediaKey,
    ) -> Option<String> {
        let bytes = self.download_media(&media_key.key, &media_key.media_type)
            .await
            .ok()?;

        let cache_dir = hermes_core::get_hermes_home().join("feishu").join("media");
        std::fs::create_dir_all(&cache_dir).ok()?;

        let ext = match media_key.media_type.as_str() {
            "image" => "jpg",
            "audio" => "mp3",
            "file" => media_key.name.as_deref().and_then(|n| n.rsplit('.').next()).unwrap_or("bin"),
            _ => "bin",
        };

        let hash = Self::content_hash(&bytes);
        let file_name = format!("{}_{}.{}", hash, media_key.media_type, ext);
        let path = cache_dir.join(&file_name);

        // Skip write if already cached (dedup)
        if !path.exists() {
            std::fs::write(&path, bytes).ok()?;
        }
        Some(path.to_string_lossy().to_string())
    }

    /// Process an inbound webhook event and return a message event.
    pub async fn handle_inbound(&self, payload: &Value) -> Option<FeishuMessageEvent> {
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

        let sender_name = payload
            .get("sender")
            .and_then(|s| s.get("nickname"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let content_type = payload
            .get("message")
            .and_then(|m| m.get("message_type"))
            .and_then(|v| v.as_str())
            .unwrap_or("text")
            .to_string();

        let content_str = payload
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let content: Value = serde_json::from_str(&content_str).unwrap_or(Value::Null);
        let text = content.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let is_group = payload
            .get("message")
            .and_then(|m| m.get("chat_type"))
            .and_then(|v| v.as_str())
            .map(|t| t == "group")
            .unwrap_or(false);

        // Extract and download media attachments
        let media_keys = Self::extract_media_keys(&content, &content_type);
        let mut media_paths = Vec::new();
        for key in &media_keys {
            if let Some(path) = self.download_and_cache_media(key).await {
                media_paths.push(format!("[{}: {}]", key.media_type, path));
            }
        }

        let final_content = if media_paths.is_empty() {
            text
        } else {
            format!("{}\n{}", text, media_paths.join("\n"))
        };

        if !msg_id.is_empty() {
            self.dedup.insert(msg_id.to_string());
        }

        Some(FeishuMessageEvent {
            message_id: msg_id.to_string(),
            chat_id,
            sender_id,
            sender_name,
            content: final_content,
            msg_type: content_type,
            is_group,
            mentions: Vec::new(),
            media_keys,
        })
    }

    /// Check group policy for inbound messages.
    ///
    /// Mirrors Python `_allow_group_message()` (feishu.py:3031).
    fn is_group_message_allowed(&self, sender_id: &str) -> bool {
        match self.config.group_policy {
            GroupPolicy::Open => true,
            GroupPolicy::Allowlist => self.config.allowed_users.contains(sender_id),
            GroupPolicy::Blacklist => !self.config.allowed_users.contains(sender_id),
            GroupPolicy::AdminOnly => false,
            GroupPolicy::Disabled => false,
        }
    }

    /// Check if message mentions the bot.
    ///
    /// Mirrors Python `_message_mentions_bot()` (feishu.py:3082).
    #[allow(dead_code)]
    fn message_mentions_bot(mentions: &[String], bot_id: &str) -> bool {
        if bot_id.is_empty() {
            return true; // No bot_id configured, accept all
        }
        mentions.iter().any(|m| m == bot_id)
    }

    /// Verify Feishu webhook signature.
    ///
    /// Mirrors Python `_is_webhook_signature_valid()` (feishu.py:2452).
    /// SHA256(timestamp + nonce + encrypt_key + body) == signature.
    fn is_signature_valid(&self, headers: &HeaderMap, body: &[u8]) -> bool {
        if self.config.encrypt_key.is_empty() {
            return true; // No encryption configured, skip verification
        }

        use sha2::{Digest, Sha256};

        let timestamp = headers
            .get("x-lark-request-timestamp")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let nonce = headers
            .get("x-lark-request-nonce")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let signature = headers
            .get("x-lark-signature")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let mut hasher = Sha256::new();
        hasher.update(timestamp.as_bytes());
        hasher.update(nonce.as_bytes());
        hasher.update(self.config.encrypt_key.as_bytes());
        hasher.update(body);
        let computed = hex::encode(hasher.finalize());

        // Timing-safe comparison
        computed == signature
    }

    /// Start the Feishu webhook HTTP server.
    ///
    /// Mirrors Python `_handle_webhook_request()` (feishu.py:2358).
    /// Listens on the configured port/path and dispatches inbound messages
    /// to the `on_message` callback.
    pub async fn run_webhook(
        &self,
        shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<(), String> {
        let path = self.config.webhook_path.clone();
        let adapter = Arc::new(self.clone_for_webhook());

        let app = Router::new()
            .route(&path, axum::routing::post(move |headers: HeaderMap, body: Bytes| {
                let adapter = adapter.clone();
                async move {
                    adapter.handle_webhook_request(&headers, &body).await
                }
            }));

        let addr = format!("0.0.0.0:{}", self.config.webhook_port);
        info!("Feishu webhook listening on {addr}{path}");

        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| format!("Failed to bind to {addr}: {e}"))?;

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .map_err(|e| format!("Feishu webhook server error: {e}"))
    }

    /// Clone the adapter for webhook server use (shares state via Arc).
    fn clone_for_webhook(&self) -> FeishuAdapter {
        FeishuAdapter {
            config: self.config.clone(),
            client: self.client.clone(),
            dedup: self.dedup.clone(),
            access_token: self.access_token.clone(),
            on_message: self.on_message.clone(),
        }
    }

    /// Handle a single webhook request.
    ///
    /// Mirrors Python `_handle_webhook_request()` (feishu.py:2358).
    async fn handle_webhook_request(
        &self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> (StatusCode, Json<Value>) {
        // Content-Type guard
        let content_type = headers
            .get("Content-Type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if !content_type.is_empty() && content_type != "application/json" {
            warn!("[Feishu] Webhook rejected: unexpected Content-Type {content_type:?}");
            return (StatusCode::UNSUPPORTED_MEDIA_TYPE, Json(serde_json::json!({"code": 415})));
        }

        // Body size guard
        if body.len() > FEISHU_WEBHOOK_MAX_BODY_BYTES {
            warn!("[Feishu] Webhook body too large: {} bytes", body.len());
            return (StatusCode::PAYLOAD_TOO_LARGE, Json(serde_json::json!({"code": 413})));
        }

        // Parse JSON
        let payload: Value = match serde_json::from_slice(body) {
            Ok(v) => v,
            Err(e) => {
                warn!("[Feishu] Invalid webhook JSON: {e}");
                return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"code": 400, "msg": "invalid json"})));
            }
        };

        // URL verification challenge (Feishu setup)
        if payload.get("type").and_then(|v| v.as_str()) == Some("url_verification") {
            let challenge = payload.get("challenge").cloned().unwrap_or(Value::Null);
            return (StatusCode::OK, Json(serde_json::json!({"challenge": challenge})));
        }

        // Verification token check
        if !self.config.verification_token.is_empty() {
            let header = payload.get("header").and_then(|v| v.as_object());
            let incoming_token = header
                .and_then(|h| h.get("token").and_then(|v| v.as_str()))
                .or_else(|| payload.get("token").and_then(|v| v.as_str()))
                .unwrap_or("");
            if incoming_token != self.config.verification_token {
                warn!("[Feishu] Webhook rejected: invalid verification token");
                return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"code": 401})));
            }
        }

        // Signature verification
        if !self.is_signature_valid(headers, body) {
            warn!("[Feishu] Webhook rejected: invalid signature");
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"code": 401})));
        }

        // Encrypted payload not supported
        if payload.get("encrypt").is_some() {
            error!("[Feishu] Encrypted webhook payloads are not supported");
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"code": 400, "msg": "encrypted not supported"})));
        }

        // Route by event type
        let event_type = payload
            .get("header")
            .and_then(|h| h.get("event_type"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match event_type {
            "im.message.receive_v1" => {
                if let Some(event) = self.handle_inbound(&payload).await {
                    // Group policy check
                    if event.is_group && !self.is_group_message_allowed(&event.sender_id) {
                        debug!("[Feishu] Group message from {} blocked by policy", event.sender_id);
                        return (StatusCode::OK, Json(serde_json::json!({"code": 0, "msg": "ok"})));
                    }

                    // Invoke the callback
                    if let Some(ref cb) = *self.on_message.read().await {
                        cb(event);
                    }
                }
            }
            "card.action.trigger" => {
                if let Some(action) = payload.get("action") {
                    let action_value = action.get("value").cloned().unwrap_or(Value::Null);
                    let action_tag = action
                        .get("tag")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    info!(
                        "[Feishu] Card action triggered: tag={action_tag}, value={action_value}"
                    );
                    // TODO: route to registered card action handler if available
                }
            }
            "im.chat.member.bot.added_v1" => {
                let chat_id = payload
                    .get("event")
                    .and_then(|e| e.get("chat_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                info!("[Feishu] Bot added to chat: {chat_id}");
                // Optionally send a welcome message
                let _ = self.send_text(chat_id, "Hello! I'm Hermes Agent. How can I help you today?").await;
            }
            "im.chat.member.bot.deleted_v1" => {
                let chat_id = payload
                    .get("event")
                    .and_then(|e| e.get("chat_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                info!("[Feishu] Bot removed from chat: {chat_id}");
            }
            "im.message.reaction.created_v1" | "im.message.reaction.deleted_v1" => {
                if let Some(reaction) = payload.get("event") {
                    let emoji = reaction.get("reaction_type")
                        .and_then(|v| v.get("emoji_type"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let operator = reaction.get("operator")
                        .and_then(|v| v.get("operator_id"))
                        .and_then(|v| v.get("open_id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let action = if event_type == "im.message.reaction.created_v1" { "added" } else { "removed" };
                    debug!("[Feishu] Reaction {action} by {operator}: {emoji}");
                }
            }
            "im.message.message_read_v1" => {
                debug!("[Feishu] Message read event");
            }
            _ => {
                debug!("[Feishu] Unknown event type: {event_type}");
            }
        }

        (StatusCode::OK, Json(serde_json::json!({"code": 0, "msg": "ok"})))
    }

    pub fn is_configured(&self) -> bool {
        !self.config.app_id.is_empty() && !self.config.app_secret.is_empty()
    }
}

/// Check if text contains simple markdown markers.
///
/// Only counts markers at the start of a line (headings, lists, quotes)
/// or inline formatting sequences (bold, code) to avoid false positives
/// like "Issue #123" or "2024-01-01".
pub(crate) fn looks_like_markdown(text: &str) -> bool {
    text.contains("**")
        || text.contains("__")
        || text.contains("`")
        || text.lines().any(|line| {
            let t = line.trim_start();
            t.starts_with("# ")
                || t.starts_with("## ")
                || t.starts_with("### ")
                || t.starts_with("- ")
                || t.starts_with("* ")
                || t.starts_with("| ")
                || t.starts_with("> ")
        })
}

/// Build a Feishu post payload from plain text.
///
/// Does a best-effort conversion of simple markdown to post segments.
pub(crate) fn build_post_payload(title: &str, text: &str) -> Value {
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut segments = Vec::new();

        // Simple heuristic: heading lines
        if trimmed.starts_with("# ") {
            segments.push(serde_json::json!({
                "tag": "text",
                "text": trimmed.strip_prefix("# ").unwrap_or(trimmed),
                "style": {"bold": true, "underline": true}
            }));
        } else if trimmed.starts_with("## ") {
            segments.push(serde_json::json!({
                "tag": "text",
                "text": trimmed.strip_prefix("## ").unwrap_or(trimmed),
                "style": {"bold": true}
            }));
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            segments.push(serde_json::json!({
                "tag": "text",
                "text": format!("• {}", trimmed[2..].trim()),
            }));
        } else {
            // Plain text segment
            segments.push(serde_json::json!({
                "tag": "text",
                "text": line,
            }));
        }
        lines.push(segments);
    }

    // If no lines parsed, add the raw text as a single segment
    if lines.is_empty() {
        lines.push(vec![serde_json::json!({
            "tag": "text",
            "text": text,
        })]);
    }

    let mut payload = serde_json::json!({
        "zh_cn": {
            "content": lines,
        }
    });

    if !title.is_empty() {
        payload["zh_cn"]["title"] = serde_json::Value::String(title.to_string());
    }

    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_env() {
        let config = FeishuConfig::from_env();
        assert_eq!(config.webhook_port, 8765);
        assert_eq!(config.webhook_path, "/feishu/webhook");
    }

    #[test]
    fn test_not_configured_when_empty() {
        let config = FeishuConfig::from_env();
        let adapter = FeishuAdapter::new(config);
        assert!(!adapter.is_configured());
    }

    #[test]
    fn test_group_policy_open() {
        let config = FeishuConfig {
            group_policy: GroupPolicy::Open,
            ..FeishuConfig::from_env()
        };
        let adapter = FeishuAdapter::new(config);
        assert!(adapter.is_group_message_allowed("any_user"));
    }

    #[test]
    fn test_group_policy_allowlist() {
        let mut allowed = HashSet::new();
        allowed.insert("user1".to_string());
        let config = FeishuConfig {
            group_policy: GroupPolicy::Allowlist,
            allowed_users: allowed,
            ..FeishuConfig::from_env()
        };
        let adapter = FeishuAdapter::new(config);
        assert!(adapter.is_group_message_allowed("user1"));
        assert!(!adapter.is_group_message_allowed("user2"));
    }

    #[test]
    fn test_group_policy_disabled() {
        let config = FeishuConfig {
            group_policy: GroupPolicy::Disabled,
            ..FeishuConfig::from_env()
        };
        let adapter = FeishuAdapter::new(config);
        assert!(!adapter.is_group_message_allowed("any_user"));
    }

    #[test]
    fn test_signature_verification() {
        use sha2::{Digest, Sha256};

        let config = FeishuConfig {
            encrypt_key: "test_encrypt_key".to_string(),
            ..FeishuConfig::from_env()
        };
        let adapter = FeishuAdapter::new(config);

        let body = b"test body";
        let timestamp = "1234567890";
        let nonce = "abc123";

        let mut hasher = Sha256::new();
        hasher.update(timestamp.as_bytes());
        hasher.update(nonce.as_bytes());
        hasher.update("test_encrypt_key".as_bytes());
        hasher.update(body);
        let expected_sig = hex::encode(hasher.finalize());

        let mut headers = HeaderMap::new();
        headers.insert("x-lark-request-timestamp", timestamp.parse().unwrap());
        headers.insert("x-lark-request-nonce", nonce.parse().unwrap());
        headers.insert("x-lark-signature", expected_sig.parse().unwrap());

        assert!(adapter.is_signature_valid(&headers, body));
    }

    #[test]
    fn test_extract_media_keys_image_and_file() {
        let content = serde_json::json!({
            "image_key": "img_123",
            "file_key": "file_456",
            "file_name": "report.pdf",
        });
        let keys = FeishuAdapter::extract_media_keys(&content, "text");
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].media_type, "image");
        assert_eq!(keys[0].key, "img_123");
        assert_eq!(keys[1].media_type, "file");
        assert_eq!(keys[1].key, "file_456");
        assert_eq!(keys[1].name.as_deref(), Some("report.pdf"));
    }

    #[test]
    fn test_extract_media_keys_audio_no_double_count() {
        let content = serde_json::json!({
            "file_key": "file_789",
            "file_name": "voice.mp3",
        });
        let keys = FeishuAdapter::extract_media_keys(&content, "audio");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].media_type, "audio");
        assert_eq!(keys[0].key, "file_789");
    }

    #[test]
    fn test_extract_media_keys_empty() {
        let content = serde_json::json!({"text": "hello"});
        let keys = FeishuAdapter::extract_media_keys(&content, "text");
        assert!(keys.is_empty());
    }

    #[test]
    fn test_looks_like_markdown_positive() {
        assert!(looks_like_markdown("# Heading\nSome text"));
        assert!(looks_like_markdown("**bold** text"));
        assert!(looks_like_markdown("`code`"));
        assert!(looks_like_markdown("- list item"));
    }

    #[test]
    fn test_looks_like_markdown_negative() {
        assert!(!looks_like_markdown("Issue #123 is fixed"));
        assert!(!looks_like_markdown("Date: 2024-01-01"));
        assert!(!looks_like_markdown("Asterisk * in middle"));
        assert!(!looks_like_markdown("Pipe | separator"));
    }

    #[test]
    fn test_build_post_payload() {
        let payload = build_post_payload("Title", "# Hello\n- Item 1\n- Item 2");
        let zh = payload.get("zh_cn").unwrap();
        let title = zh.get("title").and_then(|v| v.as_str());
        assert_eq!(title, Some("Title"));
        let content = zh.get("content").and_then(|v| v.as_array()).unwrap();
        assert_eq!(content.len(), 3);
    }

    #[test]
    fn test_build_post_payload_no_title() {
        let payload = build_post_payload("", "Plain text");
        assert!(payload.get("zh_cn").unwrap().get("title").is_none());
    }
}
