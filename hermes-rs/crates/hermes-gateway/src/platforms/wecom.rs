//! WeCom (企业微信) platform adapter.
//!
//! Mirrors the Python `gateway/platforms/wecom.py`.
//!
//! Supports:
//! - WebSocket long connection to openws.work.weixin.qq.com
//! - Direct-message and group text receive/send
//! - Message deduplication
//! - Auto-reconnect with exponential backoff
//! - Application-level heartbeat
//! - Request/response correlation via req_id
//!
//! The adapter connects to WeCom's WebSocket endpoint, authenticates
//! with bot_id + secret, and receives messages as JSON frames.

use futures::{SinkExt, StreamExt};
use reqwest::Client;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex, Semaphore};
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};
use tracing::{debug, error, info, warn};

/// Deduplication cache for inbound messages.
struct DedupCache {
    entries: parking_lot::Mutex<HashSet<String>>,
    max_size: usize,
}

impl DedupCache {
    fn new(max_size: usize) -> Self {
        Self {
            entries: parking_lot::Mutex::new(HashSet::with_capacity(max_size)),
            max_size,
        }
    }

    fn contains(&self, key: &str) -> bool {
        self.entries.lock().contains(key)
    }

    fn insert(&self, key: String) {
        let mut set = self.entries.lock();
        if set.len() >= self.max_size {
            set.clear();
        }
        set.insert(key);
    }
}

/// Truncate text to at most `max_chars` characters (UTF-8 safe).
fn truncate_text(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// WeCom platform configuration.
#[derive(Debug, Clone)]
pub struct WeComConfig {
    pub bot_id: String,
    pub secret: String,
    pub websocket_url: String,
}

impl Default for WeComConfig {
    fn default() -> Self {
        Self {
            bot_id: std::env::var("WECOM_BOT_ID").unwrap_or_default(),
            secret: std::env::var("WECOM_SECRET").unwrap_or_default(),
            websocket_url: std::env::var("WECOM_WEBSOCKET_URL")
                .ok()
                .unwrap_or_else(|| "wss://openws.work.weixin.qq.com".to_string()),
        }
    }
}

impl WeComConfig {
    pub fn from_env() -> Self {
        Self::default()
    }
}

/// Inbound message event from WeCom.
#[derive(Debug, Clone)]
pub struct WeComMessageEvent {
    /// Unique message ID.
    pub message_id: String,
    /// Chat/session ID.
    pub chat_id: String,
    /// Sender user ID.
    pub sender_id: String,
    /// Message content (text).
    pub content: String,
    /// Message type: text, image, file, etc.
    pub msg_type: String,
    /// Whether this is a group message.
    pub is_group: bool,
    /// The original req_id from the callback, for reply correlation.
    pub req_id: String,
}

/// Internal command to send via WebSocket.
#[derive(Debug)]
enum WsCommand {
    /// Send a proactive message to a chat.
    SendText { chat_id: String, text: String, reply_tx: oneshot::Sender<Result<String, String>> },
    /// Reply to a specific inbound callback.
    RespondText { req_id: String, text: String, reply_tx: oneshot::Sender<Result<String, String>> },
}

/// Shared state for the WebSocket connection.
#[allow(dead_code)]
struct WsState {
    /// Command channel sender.
    cmd_tx: mpsc::Sender<WsCommand>,
    /// Running flag.
    running: Arc<std::sync::atomic::AtomicBool>,
    /// Inbound event channel.
    event_tx: mpsc::Sender<WeComMessageEvent>,
    /// Reply_req_id mapping: message_id -> req_id (for aibot_respond_msg).
    reply_req_ids: Arc<parking_lot::Mutex<std::collections::HashMap<String, String>>>,
}

/// WeCom platform adapter.
pub struct WeComAdapter {
    config: WeComConfig,
    client: Client,
    dedup: DedupCache,
    /// WebSocket state, set when connected.
    ws_state: Mutex<Option<Arc<WsState>>>,
    /// Counter for generating unique req_ids.
    seq: AtomicUsize,
    /// Semaphore to limit concurrent event handler tasks.
    handler_semaphore: Arc<Semaphore>,
}

impl WeComAdapter {
    pub fn new(config: WeComConfig) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to build HTTP client"),
            dedup: DedupCache::new(2048),
            config,
            ws_state: Mutex::new(None),
            seq: AtomicUsize::new(0),
            handler_semaphore: Arc::new(Semaphore::new(100)),
        }
    }

    /// Generate a unique req_id with the given prefix.
    fn gen_req_id(&self, prefix: &str) -> String {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        format!("{prefix}-{seq:08x}-{}", uuid::Uuid::new_v4().simple())
    }

    /// Send a text message to a WeCom chat.
    ///
    /// If the adapter is connected via WebSocket, uses `aibot_send_msg`.
    /// Falls back to HTTP API if WebSocket is not available.
    pub async fn send_text(&self, chat_id: &str, text: &str) -> Result<String, String> {
        // Try WebSocket first
        if let Some(state) = self.ws_state.lock().await.clone() {
            let (reply_tx, reply_rx) = oneshot::channel();
            state
                .cmd_tx
                .send(WsCommand::SendText {
                    chat_id: chat_id.to_string(),
                    text: text.to_string(),
                    reply_tx,
                })
                .await
                .map_err(|_| "WebSocket command channel closed".to_string())?;

            return reply_rx
                .await
                .map_err(|_| "WebSocket response channel closed".to_string())?;
        }

        // Fallback: HTTP API
        self.send_text_http(chat_id, text).await
    }

    /// Send text via WeCom HTTP API (fallback when WebSocket not connected).
    async fn send_text_http(&self, chat_id: &str, text: &str) -> Result<String, String> {
        let token = self.get_access_token().await?;

        let is_dm = chat_id.starts_with("dm:");
        let user_or_chat_id = if is_dm {
            chat_id.strip_prefix("dm:").unwrap_or(chat_id)
        } else {
            chat_id
        };

        if is_dm {
            let resp = self
                .client
                .post(format!(
                    "https://qyapi.weixin.qq.com/cgi-bin/message/send?access_token={token}"
                ))
                .json(&serde_json::json!({
                    "touser": user_or_chat_id,
                    "msgtype": "text",
                    "agentid": self.get_agent_id(),
                    "text": {
                        "content": text,
                    },
                }))
                .send()
                .await
                .map_err(|e| format!("Failed to send message: {e}"))?;

            let body: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("Failed to parse send response: {e}"))?;

            let errcode = body.get("errcode").and_then(|v| v.as_i64()).unwrap_or(-1);
            if errcode != 0 {
                return Err(format!(
                    "WeCom send failed: errcode={errcode}, errmsg={}",
                    body.get("errmsg").and_then(|v| v.as_str()).unwrap_or("")
                ));
            }

            debug!("WeCom message sent to {chat_id} via HTTP");
            Ok("ok".to_string())
        } else {
            let resp = self
                .client
                .post(format!(
                    "https://qyapi.weixin.qq.com/cgi-bin/appchat/send?access_token={token}"
                ))
                .json(&serde_json::json!({
                    "chatid": user_or_chat_id,
                    "msgtype": "text",
                    "text": {
                        "content": text,
                    },
                }))
                .send()
                .await
                .map_err(|e| format!("Failed to send group message: {e}"))?;

            let body: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("Failed to parse send response: {e}"))?;

            let errcode = body.get("errcode").and_then(|v| v.as_i64()).unwrap_or(-1);
            if errcode != 0 {
                return Err(format!(
                    "WeCom group send failed: errcode={errcode}, errmsg={}",
                    body.get("errmsg").and_then(|v| v.as_str()).unwrap_or("")
                ));
            }

            debug!("WeCom group message sent to {chat_id} via HTTP");
            Ok("ok".to_string())
        }
    }

    /// Get the agent_id from config or env.
    fn get_agent_id(&self) -> i64 {
        std::env::var("WECOM_AGENT_ID")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    }

    /// Get/refresh the WeCom access token (HTTP fallback).
    async fn get_access_token(&self) -> Result<String, String> {
        // Use POST with JSON body to avoid leaking credentials in URL query strings
        let resp = self
            .client
            .post("https://qyapi.weixin.qq.com/cgi-bin/gettoken")
            .json(&serde_json::json!({
                "corpid": &self.config.bot_id,
                "corpsecret": &self.config.secret,
            }))
            .send()
            .await
            .map_err(|e| format!("Failed to get access token: {e}"))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse token response: {e}"))?;

        let errcode = body.get("errcode").and_then(|v| v.as_i64()).unwrap_or(-1);
        if errcode != 0 {
            return Err(format!(
                "WeCom token failed: errcode={errcode}, errmsg={}",
                body.get("errmsg").and_then(|v| v.as_str()).unwrap_or("")
            ));
        }

        body.get("access_token")
            .and_then(|v| v.as_str())
            .map(String::from)
            .ok_or_else(|| "Missing access_token in response".to_string())
    }

    /// Process an inbound WebSocket message event.
    pub fn handle_inbound(&self, event: &serde_json::Value) -> Option<WeComMessageEvent> {
        let body = event.get("body").unwrap_or(event);

        let msg_id = body
            .get("msgid")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let req_id = event
            .get("headers")
            .and_then(|h| h.get("req_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if !msg_id.is_empty() && self.dedup.contains(msg_id) {
            debug!("WeCom dedup: skipping {msg_id}");
            return None;
        }

        let content = Self::extract_text(body).unwrap_or_default();
        if content.is_empty() {
            return None;
        }

        let chat_type = body
            .get("chattype")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let chat_id_raw = body
            .get("chatid")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Build chat_id with dm: or group: prefix for routing
        let chat_id = if chat_type == "group" || chat_type == "2" {
            if chat_id_raw.is_empty() {
                let sender = body
                    .get("from")
                    .and_then(|f| f.get("userid"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                format!("group:{sender}")
            } else {
                format!("group:{chat_id_raw}")
            }
        } else {
            let sender = body
                .get("from")
                .and_then(|f| f.get("userid"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            format!("dm:{sender}")
        };

        let sender_id = body
            .get("from")
            .and_then(|f| f.get("userid"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let msg_type = body
            .get("msgtype")
            .and_then(|v| v.as_str())
            .unwrap_or("text")
            .to_string();

        let is_group = chat_type == "group" || chat_type == "2";

        if !msg_id.is_empty() {
            self.dedup.insert(msg_id.to_string());
        }

        Some(WeComMessageEvent {
            message_id: msg_id.to_string(),
            chat_id,
            sender_id,
            content,
            msg_type,
            is_group,
            req_id,
        })
    }

    /// Extract text from inbound event body.
    ///
    /// Handles text, mixed (text + images), voice, appmsg, and quoted messages.
    fn extract_text(body: &serde_json::Value) -> Option<String> {
        // Try text.content
        if let Some(text_obj) = body.get("text") {
            if let Some(content) = text_obj.get("content").and_then(|v| v.as_str()) {
                if !content.trim().is_empty() {
                    return Some(content.trim().to_string());
                }
            }
        }

        // Try voice.content
        if let Some(voice) = body.get("voice") {
            if let Some(content) = voice.get("content").and_then(|v| v.as_str()) {
                if !content.trim().is_empty() {
                    return Some(format!("[voice] {content}"));
                }
            }
        }

        // Try appmsg.title
        if let Some(appmsg) = body.get("appmsg") {
            if let Some(title) = appmsg.get("title").and_then(|v| v.as_str()) {
                if !title.trim().is_empty() {
                    return Some(format!("[appmsg] {title}"));
                }
            }
        }

        // Try mixed content (text + images)
        if let Some(items) = body.get("mixed").and_then(|v| v.as_array()) {
            let parts: Vec<String> = items
                .iter()
                .filter_map(|item| {
                    if item.get("msgtype").and_then(|v| v.as_str()) == Some("text") {
                        item.get("text")
                            .and_then(|t| t.get("content"))
                            .and_then(|v| v.as_str())
                            .map(String::from)
                    } else {
                        None
                    }
                })
                .collect();
            let combined = parts.join("\n");
            if !combined.is_empty() {
                return Some(combined);
            }
        }

        // Try quote (reply with quoted text)
        if let Some(quote) = body.get("quote") {
            let reply_text = quote
                .get("content")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_default();

            let original_text = quote
                .get("original")
                .and_then(|o| o.get("content"))
                .and_then(|v| v.as_str())
                .map(|s| format!("\n---\n{s}"))
                .unwrap_or_default();

            let combined = format!("{reply_text}{original_text}");
            if !combined.trim().is_empty() {
                return Some(combined.trim().to_string());
            }
        }

        None
    }

    /// Check if the adapter is properly configured.
    pub fn is_configured(&self) -> bool {
        !self.config.bot_id.is_empty() && !self.config.secret.is_empty()
    }

    /// Send a reply via WebSocket, trying respond_msg first then falling back to send_msg.
    async fn send_reply(
        ws_state: &WsState,
        req_id: &str,
        chat_id: &str,
        response: String,
    ) {
        if !req_id.is_empty() {
            let (reply_tx, reply_rx) = oneshot::channel();
            if ws_state
                .cmd_tx
                .send(WsCommand::RespondText {
                    req_id: req_id.to_string(),
                    text: response.clone(),
                    reply_tx,
                })
                .await
                .is_ok()
            {
                if let Ok(Ok(_)) = reply_rx.await {
                    return; // respond_msg succeeded
                }
            }
        }
        // Fallback: proactive send
        let (reply_tx, _) = oneshot::channel();
        let _ = ws_state
            .cmd_tx
            .send(WsCommand::SendText {
                chat_id: chat_id.to_string(),
                text: response,
                reply_tx,
            })
            .await;
    }

    /// Run the WeCom WebSocket connection loop.
    ///
    /// Connects to the WeCom WebSocket endpoint, authenticates with
    /// `aibot_subscribe`, and processes inbound messages forever.
    /// Auto-reconnects with exponential backoff on failure.
    pub async fn run(
        &self,
        handler: Arc<tokio::sync::Mutex<Option<Arc<dyn crate::runner::MessageHandler>>>>,
        running: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        const RECONNECT_BACKOFF: &[u64] = &[2, 5, 10, 30, 60];
        let mut backoff_idx = 0;

        while running.load(Ordering::SeqCst) {
            match self.connect_and_run(&handler, &running).await {
                Ok(()) => {
                    // Clean disconnect
                    backoff_idx = 0;
                }
                Err(e) => {
                    if !running.load(Ordering::SeqCst) {
                        break;
                    }
                    error!("WeCom connection error: {e}");
                    // Clear ws_state on disconnect
                    *self.ws_state.lock().await = None;

                    let delay = RECONNECT_BACKOFF[backoff_idx.min(RECONNECT_BACKOFF.len() - 1)];
                    info!("WeCom reconnecting in {delay}s...");
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    backoff_idx = (backoff_idx + 1).min(RECONNECT_BACKOFF.len() - 1);
                }
            }
        }

        *self.ws_state.lock().await = None;
        info!("WeCom WebSocket loop stopped");
    }

    /// Connect to WeCom WebSocket and run the message loop.
    /// Returns Ok(()) on clean disconnect, Err on connection failure.
    async fn connect_and_run(
        &self,
        handler: &Arc<tokio::sync::Mutex<Option<Arc<dyn crate::runner::MessageHandler>>>>,
        running: &Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(), String> {
        use tokio_tungstenite::{connect_async, tungstenite::http::Uri};

        let ws_url = &self.config.websocket_url;
        let uri: Uri = ws_url
            .parse()
            .map_err(|e| format!("Invalid WebSocket URL: {e}"))?;

        info!("WeCom connecting to {ws_url}...");

        let (ws_stream, _response) = connect_async(uri)
            .await
            .map_err(|e| format!("WebSocket connect failed: {e}"))?;

        info!("WeCom WebSocket connected");

        // Authenticate with aibot_subscribe
        let subscribe_req_id = self.gen_req_id("subscribe");
        let subscribe_frame = serde_json::json!({
            "cmd": "aibot_subscribe",
            "headers": {"req_id": &subscribe_req_id},
            "body": {
                "bot_id": &self.config.bot_id,
                "secret": &self.config.secret,
            },
        });

        let (mut write_half, read_half) = ws_stream.split();

        write_half
            .send(Message::Text(Utf8Bytes::from(subscribe_frame.to_string())))
            .await
            .map_err(|e| format!("Subscribe send failed: {e}"))?;

        // Wait for subscribe response
        let subscribed;
        let mut reader = read_half.fuse();

        loop {
            match tokio::time::timeout(
                Duration::from_secs(10),
                reader.next(),
            )
            .await
            {
                Ok(Some(Ok(Message::Text(text)))) => {
                    if let Ok(frame) = serde_json::from_str::<serde_json::Value>(&text) {
                        let frame_req_id = frame
                            .get("headers")
                            .and_then(|h| h.get("req_id"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        if frame_req_id == subscribe_req_id {
                            let errcode = frame
                                .get("body")
                                .and_then(|b| b.get("errcode"))
                                .and_then(|v| v.as_i64())
                                .unwrap_or(-1);

                            if errcode == 0 {
                                info!("WeCom subscription confirmed");
                                subscribed = true;
                                break;
                            } else {
                                let errmsg = frame
                                    .get("body")
                                    .and_then(|b| b.get("errmsg"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                return Err(format!("WeCom subscribe failed: errcode={errcode}, errmsg={errmsg}"));
                            }
                        }
                    }
                }
                Ok(Some(Ok(_))) => continue,
                Ok(Some(Err(e))) => return Err(format!("WebSocket read error: {e}")),
                Ok(None) => return Err("WebSocket closed before subscribe".to_string()),
                Err(_) => return Err("Subscribe timeout (10s)".to_string()),
            }
        }

        if !subscribed {
            return Err("Subscription not confirmed".to_string());
        }

        // Set up command channel for outbound sends
        let (cmd_tx, mut cmd_rx) = mpsc::channel::<WsCommand>(32);
        let (event_tx, mut event_rx) = mpsc::channel::<WeComMessageEvent>(64);

        let reply_req_ids: Arc<parking_lot::Mutex<std::collections::HashMap<String, String>>> =
            Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new()));

        let ws_state = Arc::new(WsState {
            cmd_tx: cmd_tx.clone(),
            running: running.clone(),
            event_tx: event_tx.clone(),
            reply_req_ids: reply_req_ids.clone(),
        });

        *self.ws_state.lock().await = Some(ws_state.clone());

        // Unified select! loop: read, send, event — all in one task.
        // This avoids leaking spawned tasks on reconnect.
        let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(30));
        heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                // Read inbound WebSocket messages
                result = tokio::time::timeout(Duration::from_secs(60), reader.next()) => {
                    match result {
                        Ok(Some(Ok(Message::Text(text)))) => {
                            if let Ok(frame) = serde_json::from_str::<serde_json::Value>(&text) {
                                // Dispatch message to event channel
                                self.dispatch_frame(
                                    &frame,
                                    &event_tx,
                                    &reply_req_ids,
                                )
                                .await;
                            }
                        }
                        Ok(Some(Ok(Message::Close(_)))) => {
                            info!("WeCom WebSocket closed by server");
                            return Err("WebSocket closed by server".to_string());
                        }
                        Ok(Some(Ok(Message::Ping(_)))) => {
                            debug!("WeCom ping received");
                        }
                        Ok(Some(Ok(_))) => {
                            // Binary, Pong: ignore
                        }
                        Ok(Some(Err(e))) => {
                            return Err(format!("WebSocket read error: {e}"));
                        }
                        Ok(None) => {
                            return Err("WebSocket stream ended".to_string());
                        }
                        Err(_) => {
                            // 60s read timeout
                            if !running.load(Ordering::SeqCst) {
                                return Ok(());
                            }
                            debug!("WeCom read timeout, reconnecting");
                            return Err("Read timeout".to_string());
                        }
                    }
                }
                // Handle outbound commands
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(WsCommand::SendText { chat_id, text, reply_tx }) => {
                            let req_id = format!("send-{}-{}", chrono::Utc::now().timestamp_millis(), uuid::Uuid::new_v4().simple());
                            let frame = serde_json::json!({
                                "cmd": "aibot_send_msg",
                                "headers": {"req_id": &req_id},
                                "body": {
                                    "chatid": chat_id,
                                    "msgtype": "markdown",
                                    "markdown": {
                                        "content": truncate_text(&text, 4000),
                                    },
                                },
                            });

                            match write_half.send(Message::Text(Utf8Bytes::from(frame.to_string()))).await {
                                Ok(()) => {
                                    debug!("WeCom aibot_send_msg sent");
                                    let _ = reply_tx.send(Ok("ok".to_string()));
                                }
                                Err(e) => {
                                    let _ = reply_tx.send(Err(format!("WebSocket send error: {e}")));
                                }
                            }
                        }
                        Some(WsCommand::RespondText { req_id, text, reply_tx }) => {
                            let stream_id = format!("stream-{}", uuid::Uuid::new_v4().simple());
                            let frame = serde_json::json!({
                                "cmd": "aibot_respond_msg",
                                "headers": {"req_id": &req_id},
                                "body": {
                                    "msgtype": "stream",
                                    "stream": {
                                        "id": stream_id,
                                        "finish": true,
                                        "content": truncate_text(&text, 4000),
                                    },
                                },
                            });

                            match write_half.send(Message::Text(Utf8Bytes::from(frame.to_string()))).await {
                                Ok(()) => {
                                    debug!("WeCom aibot_respond_msg sent");
                                    let _ = reply_tx.send(Ok("ok".to_string()));
                                }
                                Err(e) => {
                                    let _ = reply_tx.send(Err(format!("WebSocket respond error: {e}")));
                                }
                            }
                        }
                        None => {
                            info!("WeCom command channel closed");
                            return Err("Command channel closed".to_string());
                        }
                    }
                }
                // Handle inbound events (route to agent handler)
                event = event_rx.recv() => {
                    match event {
                        Some(event) => {
                            if event.content.is_empty() {
                                continue;
                            }

                            info!(
                                "WeCom message from {} via {}: {}",
                                event.sender_id,
                                event.chat_id,
                                event.content.chars().take(50).collect::<String>(),
                            );

                            // Acquire semaphore permit (limits concurrent handlers to 100)
                            let permit = self.handler_semaphore.clone()
                                .try_acquire_owned()
                                .map_err(|_| "Too many concurrent handlers")
                                .ok();

                            if permit.is_none() {
                                warn!("WeCom event rejected: too many concurrent handlers");
                                continue;
                            }

                            // Clone handler to avoid holding lock across await
                            let handler_clone = handler.clone();
                            let event_req_id = event.req_id.clone();
                            let event_chat_id = event.chat_id.clone();
                            let ws_state_clone = ws_state.clone();

                            // Spawn handler task (permit released when task completes)
                            tokio::spawn(async move {
                                // _permit is dropped here, releasing the semaphore
                                let _permit = permit;
                                let handler_guard = handler_clone.lock().await;
                                if let Some(handler) = handler_guard.as_ref() {
                                    match handler
                                        .handle_message(
                                            crate::config::Platform::Wecom,
                                            &event_chat_id,
                                            &event.content,
                                        )
                                        .await
                                    {
                                        Ok(result) => {
                                            if !result.response.is_empty() {
                                                Self::send_reply(
                                                    &ws_state_clone,
                                                    &event_req_id,
                                                    &event_chat_id,
                                                    result.response,
                                                )
                                                .await;
                                            }
                                        }
                                        Err(e) => {
                                            error!("Agent handler failed for WeCom message: {e}");
                                            let (reply_tx, _) = oneshot::channel();
                                            let _ = ws_state_clone
                                                .cmd_tx
                                                .send(WsCommand::SendText {
                                                    chat_id: event_chat_id.clone(),
                                                    text: "Sorry, I encountered an error processing your message.".to_string(),
                                                    reply_tx,
                                                })
                                                .await;
                                        }
                                    }
                                } else {
                                    warn!("No message handler registered for WeCom messages");
                                }
                            });
                        }
                        None => {
                            info!("WeCom event channel closed");
                            return Err("Event channel closed".to_string());
                        }
                    }
                }
                // Application-level heartbeat
                _ = heartbeat_interval.tick() => {
                    let ping_id = format!("ping-{}", uuid::Uuid::new_v4().simple());
                    let frame = serde_json::json!({
                        "cmd": "ping",
                        "headers": {"req_id": &ping_id},
                        "body": {},
                    });
                    if let Err(e) = write_half.send(Message::Text(Utf8Bytes::from(frame.to_string()))).await {
                        warn!("WeCom heartbeat failed: {e}");
                    }
                }
                // Check running flag periodically (200ms for responsive shutdown)
                _ = tokio::time::sleep(Duration::from_millis(200)) => {
                    if !running.load(Ordering::SeqCst) {
                        return Ok(());
                    }
                }
            }
        }
    }

    /// Dispatch an inbound WebSocket frame.
    async fn dispatch_frame(
        &self,
        frame: &serde_json::Value,
        event_tx: &mpsc::Sender<WeComMessageEvent>,
        reply_req_ids: &Arc<parking_lot::Mutex<std::collections::HashMap<String, String>>>,
    ) {
        let cmd = frame
            .get("cmd")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match cmd {
            "aibot_msg_callback" | "aibot_callback" => {
                // Reuse handle_inbound for parsing and dedup
                let Some(event) = self.handle_inbound(frame) else {
                    return;
                };

                // Store req_id mapping for later replies
                if !event.message_id.is_empty() && !event.req_id.is_empty() {
                    let mut map = reply_req_ids.lock();
                    map.insert(event.message_id.clone(), event.req_id.clone());
                    // Clean up old entries periodically
                    if map.len() > 2048 {
                        let to_remove: Vec<String> =
                            map.keys().take(map.len() - 1024).cloned().collect();
                        for k in to_remove {
                            map.remove(&k);
                        }
                    }
                }

                let _ = event_tx.send(event).await;
            }
            "aibot_event_callback" => {
                debug!("WeCom event ignored");
            }
            _ => {
                debug!("WeCom unhandled frame: cmd={cmd}");
            }
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = WeComConfig::default();
        assert_eq!(
            config.websocket_url,
            "wss://openws.work.weixin.qq.com"
        );
    }

    #[test]
    fn test_config_from_env() {
        let config = WeComConfig::from_env();
        assert!(config.websocket_url.starts_with("wss://"));
    }

    #[test]
    fn test_not_configured_when_empty() {
        let config = WeComConfig::default();
        let adapter = WeComAdapter::new(config);
        assert!(!adapter.is_configured());
    }

    #[test]
    fn test_extract_text_content() {
        let event = serde_json::json!({
            "body": {
                "msgid": "wecom_msg_1",
                "chattype": "1",
                "from": {"userid": "user123"},
                "msgtype": "text",
                "text": {"content": "hello wecom"},
            }
        });
        let adapter = WeComAdapter::new(WeComConfig::default());
        let evt = adapter.handle_inbound(&event).unwrap();
        assert_eq!(evt.content, "hello wecom");
        assert!(!evt.is_group);
        assert_eq!(evt.chat_id, "dm:user123");
    }

    #[test]
    fn test_extract_group_message() {
        let event = serde_json::json!({
            "body": {
                "msgid": "wecom_msg_2",
                "chattype": "group",
                "chatid": "group456",
                "from": {"userid": "user789"},
                "msgtype": "text",
                "text": {"content": "group message"},
            }
        });
        let adapter = WeComAdapter::new(WeComConfig::default());
        let evt = adapter.handle_inbound(&event).unwrap();
        assert!(evt.is_group);
        assert_eq!(evt.chat_id, "group:group456");
    }

    #[test]
    fn test_extract_mixed_content() {
        let event = serde_json::json!({
            "body": {
                "msgid": "wecom_msg_3",
                "chattype": "1",
                "from": {"userid": "user1"},
                "msgtype": "mixed",
                "mixed": [
                    {"msgtype": "text", "text": {"content": "text before"}},
                    {"msgtype": "image", "image": {"url": "http://..."}},
                    {"msgtype": "text", "text": {"content": "text after"}},
                ],
            }
        });
        let adapter = WeComAdapter::new(WeComConfig::default());
        let evt = adapter.handle_inbound(&event).unwrap();
        assert_eq!(evt.content, "text before\ntext after");
    }

    #[test]
    fn test_extract_voice_content() {
        let event = serde_json::json!({
            "body": {
                "msgid": "wecom_msg_4",
                "chattype": "1",
                "from": {"userid": "user1"},
                "msgtype": "voice",
                "voice": {"content": "语音消息"},
            }
        });
        let adapter = WeComAdapter::new(WeComConfig::default());
        let evt = adapter.handle_inbound(&event).unwrap();
        assert_eq!(evt.content, "[voice] 语音消息");
    }

    #[test]
    fn test_extract_appmsg_title() {
        let event = serde_json::json!({
            "body": {
                "msgid": "wecom_msg_5",
                "chattype": "1",
                "from": {"userid": "user1"},
                "msgtype": "appmsg",
                "appmsg": {"title": "Article Title"},
            }
        });
        let adapter = WeComAdapter::new(WeComConfig::default());
        let evt = adapter.handle_inbound(&event).unwrap();
        assert_eq!(evt.content, "[appmsg] Article Title");
    }

    #[test]
    fn test_dedup() {
        let adapter = WeComAdapter::new(WeComConfig::default());
        let event = serde_json::json!({
            "body": {
                "msgid": "dedup_wecom_1",
                "chattype": "1",
                "from": {"userid": "user1"},
                "text": {"content": "hello"},
            }
        });
        assert!(adapter.handle_inbound(&event).is_some());
        assert!(adapter.handle_inbound(&event).is_none());
    }

    #[test]
    fn test_req_id_extracted() {
        let event = serde_json::json!({
            "cmd": "aibot_msg_callback",
            "headers": {"req_id": "callback-test-123"},
            "body": {
                "msgid": "wecom_msg_6",
                "chattype": "1",
                "from": {"userid": "user1"},
                "text": {"content": "hello"},
            }
        });
        let adapter = WeComAdapter::new(WeComConfig::default());
        let evt = adapter.handle_inbound(&event).unwrap();
        assert_eq!(evt.req_id, "callback-test-123");
    }

    #[test]
    fn test_extract_quote_message() {
        let event = serde_json::json!({
            "body": {
                "msgid": "wecom_msg_7",
                "chattype": "1",
                "from": {"userid": "user1"},
                "msgtype": "text",
                "quote": {
                    "content": "this is a reply",
                    "original": {"content": "original message"},
                },
            }
        });
        let adapter = WeComAdapter::new(WeComConfig::default());
        let evt = adapter.handle_inbound(&event).unwrap();
        assert!(evt.content.contains("this is a reply"));
        assert!(evt.content.contains("original message"));
    }
}
