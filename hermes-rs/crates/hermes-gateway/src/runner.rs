//! Gateway runner entry point for messaging platform integrations.
//!
//! Manages the gateway lifecycle:
//! - Loads platform configuration
//! - Starts configured platform adapters (Feishu, Weixin)
//! - Routes messages to the agent engine
//! - Handles graceful shutdown

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::oneshot;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

use crate::config::{Platform, PlatformConfig};
use crate::platforms::api_server::{ApiServerAdapter, ApiServerConfig, ApiServerState};
use crate::platforms::dingtalk::{DingtalkAdapter, DingtalkConfig};
use crate::platforms::feishu::{FeishuAdapter, FeishuConfig, FeishuConnectionMode, FeishuMessageEvent};
use crate::platforms::wecom::{WeComAdapter, WeComConfig};
use crate::platforms::weixin::{WeixinAdapter, WeixinConfig, WeixinMessageEvent};

/// Gateway configuration.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// Platform configurations.
    pub platforms: Vec<PlatformConfigEntry>,
    /// Default model to use.
    pub default_model: String,
}

/// A platform configuration entry with its enabled status.
#[derive(Debug, Clone)]
pub struct PlatformConfigEntry {
    pub platform: Platform,
    pub enabled: bool,
    pub config: PlatformConfig,
}

/// Result from a message handler, including metadata for gateway-level handling.
#[derive(Debug, Clone)]
pub struct HandlerResult {
    /// Response text to send to the user.
    pub response: String,
    /// Complete agent message history after the turn (includes tool_calls).
    /// Mirrors Python result["messages"] — used by Responses API to produce
    /// function_call/function_call_output output items.
    pub messages: Vec<serde_json::Value>,
    /// Compression was exhausted — gateway should auto-reset the session
    /// to break the infinite loop. Mirrors Python PR c5688e7c.
    pub compression_exhausted: bool,
    /// Token usage from the LLM response (if available).
    pub usage: Option<TokenUsage>,
}

/// Token usage info from the LLM.
#[derive(Debug, Clone)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// Message handler trait -- called when a platform receives a message.
#[async_trait::async_trait]
pub trait MessageHandler: Send + Sync + 'static {
    async fn handle_message(
        &self,
        platform: Platform,
        chat_id: &str,
        content: &str,
    ) -> Result<HandlerResult, String>;

    /// Signal the handler to interrupt its current conversation turn.
    /// Default is no-op for handlers that don't support interruption.
    /// Mirrors Python PR a8b7db35 — immediate interrupt on user message.
    fn interrupt(&self, _chat_id: &str, _new_message: &str) {
        // no-op by default
    }
}

/// Gateway runner managing platform adapter lifecycles.
pub struct GatewayRunner {
    config: GatewayConfig,
    feishu_adapter: Option<Arc<FeishuAdapter>>,
    weixin_adapter: Option<Arc<WeixinAdapter>>,
    api_server_adapter: Option<Arc<ApiServerAdapter>>,
    dingtalk_adapter: Option<Arc<DingtalkAdapter>>,
    wecom_adapter: Option<Arc<WeComAdapter>>,
    api_server_shutdown_tx: Vec<oneshot::Sender<()>>,
    dingtalk_shutdown_tx: Vec<oneshot::Sender<()>>,
    feishu_shutdown_tx: Vec<oneshot::Sender<()>>,
    message_handler: Arc<Mutex<Option<Arc<dyn MessageHandler>>>>,
    running: Arc<AtomicBool>,
    /// Track which sessions are currently running (chat_id -> start timestamp).
    /// Used for busy-session interrupt logic (Python PR a8b7db35).
    /// std::sync::Mutex — critical sections are trivially fast (HashMap insert/get).
    running_sessions: Arc<std::sync::Mutex<HashMap<String, f64>>>,
    /// Busy ack timestamps for debouncing (chat_id -> last ack time).
    busy_ack_ts: Arc<std::sync::Mutex<HashMap<String, f64>>>,
}

impl GatewayRunner {
    pub fn new(config: GatewayConfig) -> Self {
        Self {
            config,
            feishu_adapter: None,
            weixin_adapter: None,
            api_server_adapter: None,
            dingtalk_adapter: None,
            wecom_adapter: None,
            api_server_shutdown_tx: Vec::new(),
            dingtalk_shutdown_tx: Vec::new(),
            feishu_shutdown_tx: Vec::new(),
            message_handler: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            running_sessions: Arc::new(std::sync::Mutex::new(HashMap::new())),
            busy_ack_ts: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Set the message handler (agent engine).
    pub async fn set_message_handler(&self, handler: Arc<dyn MessageHandler>) {
        *self.message_handler.lock().await = Some(handler);
    }

    /// Initialize platform adapters based on config.
    pub fn initialize(&mut self) {
        for entry in &self.config.platforms {
            if !entry.enabled {
                info!("Platform {} disabled, skipping", entry.platform.as_str());
                continue;
            }
            match entry.platform {
                Platform::Feishu => {
                    let feishu_config = FeishuConfig::from_env();
                    if !feishu_config.app_id.is_empty() && !feishu_config.app_secret.is_empty() {
                        info!("Initializing Feishu adapter...");
                        self.feishu_adapter = Some(Arc::new(FeishuAdapter::new(feishu_config)));
                    } else {
                        warn!("Feishu enabled but not configured (missing FEISHU_APP_ID/SECRET)");
                    }
                }
                Platform::Weixin => {
                    let weixin_config = WeixinConfig::from_env();
                    if !weixin_config.session_key.is_empty() {
                        info!("Initializing Weixin adapter...");
                        self.weixin_adapter = Some(Arc::new(WeixinAdapter::new(weixin_config)));
                    } else {
                        warn!("Weixin enabled but not configured (missing WEIXIN_SESSION_KEY)");
                    }
                }
                Platform::ApiServer => {
                    let api_config = ApiServerConfig::from_env();
                    info!(
                        "Initializing API Server adapter on {}:{}...",
                        api_config.host, api_config.port
                    );
                    self.api_server_adapter = Some(Arc::new(ApiServerAdapter::new(api_config)));
                }
                Platform::Dingtalk => {
                    let dingtalk_config = DingtalkConfig::from_env();
                    if !dingtalk_config.client_id.is_empty() && !dingtalk_config.client_secret.is_empty() {
                        info!("Initializing Dingtalk adapter...");
                        self.dingtalk_adapter =
                            Some(Arc::new(DingtalkAdapter::new(dingtalk_config)));
                    } else {
                        warn!(
                            "Dingtalk enabled but not configured \
                             (missing DINGTALK_CLIENT_ID/SECRET)"
                        );
                    }
                }
                Platform::Wecom => {
                    let wecom_config = WeComConfig::from_env();
                    if !wecom_config.bot_id.is_empty() && !wecom_config.secret.is_empty() {
                        info!("Initializing WeCom adapter...");
                        self.wecom_adapter = Some(Arc::new(WeComAdapter::new(wecom_config)));
                    } else {
                        warn!(
                            "WeCom enabled but not configured \
                             (missing WECOM_BOT_ID/SECRET)"
                        );
                    }
                }
                _ => {
                    warn!("Platform {} not yet implemented in Rust", entry.platform.as_str());
                }
            }
        }

        let feishu_count = self.feishu_adapter.is_some() as usize;
        let weixin_count = self.weixin_adapter.is_some() as usize;
        let api_server_count = self.api_server_adapter.is_some() as usize;
        let dingtalk_count = self.dingtalk_adapter.is_some() as usize;
        let wecom_count = self.wecom_adapter.is_some() as usize;
        let feishu_webhook_count = self.feishu_adapter.as_ref()
            .map(|a| matches!(a.config.connection_mode, FeishuConnectionMode::Webhook))
            .unwrap_or(false) as usize;
        info!(
            "Gateway initialized: {} platform(s) ready",
            feishu_count + weixin_count + api_server_count + dingtalk_count + wecom_count
        );
        if feishu_webhook_count > 0 {
            info!("Feishu webhook: port={} path={}",
                self.feishu_adapter.as_ref().unwrap().config.webhook_port,
                self.feishu_adapter.as_ref().unwrap().config.webhook_path
            );
        }
    }

    /// Start the gateway main loop.
    pub async fn run(&mut self) -> Result<(), String> {
        self.running.store(true, Ordering::SeqCst);
        info!("Gateway starting...");

        // Spawn platform-specific polling tasks
        let mut handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();

        if let Some(adapter) = &self.weixin_adapter {
            let adapter = adapter.clone();
            let handler = self.message_handler.clone();
            let running = self.running.clone();
            let running_sessions = self.running_sessions.clone();
            let busy_ack_ts = self.busy_ack_ts.clone();
            let handle = tokio::spawn(async move {
                run_weixin_poll(adapter, handler, running, running_sessions, busy_ack_ts).await;
            });
            handles.push(handle);
        }

        // Feishu: start webhook server (Webhook mode) or log WebSocket mode
        if let Some(adapter) = &self.feishu_adapter {
            let adapter = adapter.clone();
            let handler = self.message_handler.clone();
            let running = self.running.clone();

            match adapter.config.connection_mode {
                FeishuConnectionMode::Webhook => {
                    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
                    let handle = tokio::spawn(async move {
                        // Set up the on_message callback to route to handler
                        let adapter_for_cb = adapter.clone();
                        adapter.on_message.write().await.replace(Arc::new(
                            move |event: FeishuMessageEvent| {
                                let handler = handler.clone();
                                let running = running.clone();
                                let adapter = adapter_for_cb.clone();
                                let event = event;
                                tokio::spawn(async move {
                                    if !running.load(Ordering::SeqCst) {
                                        return;
                                    }
                                    let guard = handler.lock().await;
                                    if let Some(h) = guard.as_ref() {
                                        info!(
                                            "Feishu message from {} via {}: {}",
                                            event.sender_id,
                                            event.chat_id,
                                            event.content.chars().take(50).collect::<String>(),
                                        );
                                        match h
                                            .handle_message(
                                                Platform::Feishu,
                                                &event.chat_id,
                                                &event.content,
                                            )
                                            .await
                                        {
                                            Ok(result) => {
                                                if !result.response.is_empty() {
                                                    if let Err(e) =
                                                        adapter.send_text_or_post(&event.chat_id, &result.response).await
                                                    {
                                                        error!("Feishu send failed: {e}");
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                error!("Agent handler failed for Feishu message: {e}");
                                                let _ = adapter
                                                    .send_text(
                                                        &event.chat_id,
                                                        "Sorry, I encountered an error processing your message.",
                                                    )
                                                    .await;
                                            }
                                        }
                                    }
                                });
                            },
                        ));

                        if let Err(e) = adapter.run_webhook(shutdown_rx).await {
                            error!("Feishu webhook error: {e}");
                        }
                    });
                    self.feishu_shutdown_tx.push(shutdown_tx);
                    handles.push(handle);
                }
                FeishuConnectionMode::WebSocket => {
                    let ws_client = crate::platforms::feishu_ws::FeishuWsClient::new(adapter.config.clone());
                    let handle = tokio::spawn(async move {
                        ws_client.run(handler).await;
                    });
                    handles.push(handle);
                }
            }
        }

        // API Server: start HTTP server
        if let Some(adapter) = &self.api_server_adapter {
            let adapter = adapter.clone();
            let handler = self.message_handler.clone();
            let api_key = adapter.config.api_key.clone();
            let model_name = adapter.config.model_name.clone();
            let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
            let handle = tokio::spawn(async move {
                let state = ApiServerState {
                    handler,
                    api_key,
                    model_name,
                };
                if let Err(e) = adapter.run(state, shutdown_rx).await {
                    error!("API Server error: {e}");
                }
            });
            self.api_server_shutdown_tx.push(shutdown_tx);
            handles.push(handle);
        }

        // WeCom: start WebSocket connection
        if let Some(adapter) = &self.wecom_adapter {
            let adapter = adapter.clone();
            let handler = self.message_handler.clone();
            let running = self.running.clone();
            let handle = tokio::spawn(async move {
                adapter.run(handler, running).await;
            });
            handles.push(handle);
        }

        // Dingtalk: start webhook HTTP server
        if let Some(adapter) = &self.dingtalk_adapter {
            let adapter = adapter.clone();
            let handler = self.message_handler.clone();
            let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
            let handle = tokio::spawn(async move {
                if let Err(e) = adapter.run(handler, shutdown_rx).await {
                    error!("Dingtalk webhook error: {e}");
                }
            });
            self.dingtalk_shutdown_tx.push(shutdown_tx);
            handles.push(handle);
        }

        // Wait for all platform tasks
        for handle in handles {
            if let Err(e) = handle.await {
                error!("Platform task panicked: {e}");
            }
        }

        info!("Gateway stopped");
        Ok(())
    }

    /// Stop the gateway gracefully.
    pub fn stop(&mut self) {
        // Trigger API server graceful shutdown
        let senders = std::mem::take(&mut self.api_server_shutdown_tx);
        for tx in senders {
            let _ = tx.send(());
        }
        // Trigger Dingtalk webhook graceful shutdown
        let senders = std::mem::take(&mut self.dingtalk_shutdown_tx);
        for tx in senders {
            let _ = tx.send(());
        }
        // Trigger Feishu webhook graceful shutdown
        let senders = std::mem::take(&mut self.feishu_shutdown_tx);
        for tx in senders {
            let _ = tx.send(());
        }
        self.running.store(false, Ordering::SeqCst);
        // Clear tracking state so it doesn't leak across stop/restart cycles.
        self.running_sessions.lock().unwrap().clear();
        self.busy_ack_ts.lock().unwrap().clear();
        info!("Gateway stop requested");
    }

    /// Check if the gateway is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get status information.
    pub fn status(&self) -> GatewayStatus {
        GatewayStatus {
            running: self.is_running(),
            feishu_configured: self.feishu_adapter.is_some(),
            weixin_configured: self.weixin_adapter.is_some(),
            api_server_configured: self.api_server_adapter.is_some(),
            dingtalk_configured: self.dingtalk_adapter.is_some(),
            wecom_configured: self.wecom_adapter.is_some(),
            platform_count: self.config.platforms.iter().filter(|p| p.enabled).count(),
        }
    }
}

/// Gateway status information.
#[derive(Debug, Clone)]
pub struct GatewayStatus {
    pub running: bool,
    pub feishu_configured: bool,
    pub weixin_configured: bool,
    pub api_server_configured: bool,
    pub dingtalk_configured: bool,
    pub wecom_configured: bool,
    pub platform_count: usize,
}

/// Poll Weixin for inbound messages and route to the agent.
async fn run_weixin_poll(
    adapter: Arc<WeixinAdapter>,
    handler: Arc<Mutex<Option<Arc<dyn MessageHandler>>>>,
    running: Arc<AtomicBool>,
    running_sessions: Arc<std::sync::Mutex<HashMap<String, f64>>>,
    busy_ack_ts: Arc<std::sync::Mutex<HashMap<String, f64>>>,
) {
    let mut poll_interval = interval(Duration::from_secs(2));
    let mut consecutive_errors = 0u32;

    info!("Weixin poll loop started");

    while running.load(Ordering::SeqCst) {
        poll_interval.tick().await;

        match adapter.get_updates().await {
            Ok(events) => {
                consecutive_errors = 0;
                for event in events {
                    // Check busy + interrupt before acquiring handler lock.
                    // This lets us call interrupt() on the handler Arc
                    // without needing to hold the Mutex guard.
                    let handler_guard = handler.lock().await;
                    let handler_ref = handler_guard.as_ref().cloned();
                    drop(handler_guard); // Release lock before routing

                    route_weixin_message(
                        &adapter, handler_ref.as_ref(), &event,
                        &running_sessions, &busy_ack_ts,
                    ).await;
                }
            }
            Err(e) => {
                consecutive_errors += 1;
                if e.contains("session expired") {
                    error!("Weixin session expired, stopping poll");
                    break;
                }
                if consecutive_errors > 5 {
                    warn!("Weixin: {consecutive_errors} consecutive errors: {e}");
                } else {
                    error!("Weixin poll error: {e}");
                }
                // Backoff on errors
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }

    info!("Weixin poll loop stopped");
}

/// Route a Weixin message to the agent handler.
///
/// If the session is already running (agent is busy), interrupt the agent,
/// send a busy ack to the user, and queue the message for the next cycle.
/// Mirrors Python PR a8b7db35 — immediate interrupt on user message.
async fn route_weixin_message(
    adapter: &WeixinAdapter,
    handler: Option<&Arc<dyn MessageHandler>>,
    event: &WeixinMessageEvent,
    running_sessions: &Arc<std::sync::Mutex<HashMap<String, f64>>>,
    busy_ack_ts: &Arc<std::sync::Mutex<HashMap<String, f64>>>,
) {
    use std::time::{SystemTime, UNIX_EPOCH};

    if event.content.is_empty() {
        return;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();

    let chat_id = &event.peer_id;

    // Check if this session is already running (busy session handling)
    let busy_elapsed_min: Option<f64> = {
        let sessions = running_sessions.lock().unwrap();
        sessions.get(chat_id).map(|&start_ts| {
            let elapsed_secs = now - start_ts;
            elapsed_secs / 60.0
        })
    };

    if let Some(elapsed_min) = busy_elapsed_min {
        // Session is busy — interrupt the running agent and ack

        // Busy ack debounce: only send every 30 seconds
        let should_ack = {
            let mut ack_map = busy_ack_ts.lock().unwrap();
            let last_ack = ack_map.get(chat_id).copied().unwrap_or(0.0);
            if now - last_ack < 30.0 {
                false // Debounced
            } else {
                ack_map.insert(chat_id.to_string(), now);
                true
            }
        };

        if should_ack {
            // Signal interrupt to the running agent
            if let Some(h) = handler {
                h.interrupt(chat_id, &event.content);
            }
            info!(
                "Session {chat_id}: busy — agent interrupted after {elapsed_min:.1} min"
            );

            // Send busy status to user
            let busy_msg = format!(
                "Still processing your previous message ({elapsed_min:.0}m elapsed). \
                 Please wait for my response before sending another prompt."
            );
            let _ = adapter.send_text(chat_id, &busy_msg).await;
        }
        return;
    }

    // Session not running — proceed with normal handling
    info!(
        "Weixin message from {}: {}",
        chat_id,
        event.content.chars().take(50).collect::<String>(),
    );

    // Mark session as running
    {
        let mut sessions = running_sessions.lock().unwrap();
        sessions.insert(chat_id.clone(), now);
    }

    let Some(handler_ref) = handler else {
        running_sessions.lock().unwrap().remove(chat_id);
        warn!("No message handler registered for Weixin messages");
        return;
    };

    match handler_ref
        .handle_message(Platform::Weixin, chat_id, &event.content)
        .await
    {
        Ok(result) => {
            // Clear session running flag
            running_sessions.lock().unwrap().remove(chat_id);
            // Clear busy ack timestamp
            busy_ack_ts.lock().unwrap().remove(chat_id);

            // Compression exhaustion — log warning so gateway operator
            // knows to implement session auto-reset policy.
            // Mirrors Python PR c5688e7c.
            if result.compression_exhausted {
                warn!(
                    "Session {}: compression exhausted — context too large after max attempts. \
                     Consider resetting the session.",
                    chat_id
                );
            }
            if !result.response.is_empty() {
                if let Err(e) = adapter.send_text(chat_id, &result.response).await {
                    error!("Weixin send failed: {e}");
                }
            }
        }
        Err(e) => {
            // Clear session running flag on error too
            running_sessions.lock().unwrap().remove(chat_id);
            busy_ack_ts.lock().unwrap().remove(chat_id);

            error!("Agent handler failed for Weixin message: {e}");
            let _ = adapter
                .send_text(chat_id, "Sorry, I encountered an error processing your message.")
                .await;
        }
    }
}

/// Load gateway config from config.yaml.
pub fn load_gateway_config() -> GatewayConfig {
    use hermes_core::hermes_home::get_hermes_home;

    let config_path = get_hermes_home().join("config.yaml");
    let mut platforms = Vec::new();
    let mut default_model = "gpt-4".to_string();

    if let Ok(content) = std::fs::read_to_string(&config_path) {
        if let Ok(config) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
            // Read gateway config
            if let Some(gateway) = config.get("gateway") {
                if let Some(model) = gateway.get("default_model").and_then(|v| v.as_str()) {
                    default_model = model.to_string();
                }
                if let Some(platforms_cfg) = gateway.get("platforms") {
                    if let Some(arr) = platforms_cfg.as_sequence() {
                        for item in arr {
                            if let Some(platform_str) = item.get("platform").and_then(|v| v.as_str()) {
                                let enabled = item
                                    .get("enabled")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(true);
                                let platform = match platform_str {
                                    "feishu" => Platform::Feishu,
                                    "weixin" => Platform::Weixin,
                                    "wecom" => Platform::Wecom,
                                    "telegram" => Platform::Telegram,
                                    "discord" => Platform::Discord,
                                    "api_server" => Platform::ApiServer,
                                    _ => Platform::Local,
                                };
                                let cfg = PlatformConfig::default();
                                platforms.push(PlatformConfigEntry {
                                    platform,
                                    enabled,
                                    config: cfg,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Fallback: check env vars for enabled platforms
    if platforms.is_empty() {
        if std::env::var("FEISHU_APP_ID").is_ok() {
            platforms.push(PlatformConfigEntry {
                platform: Platform::Feishu,
                enabled: true,
                config: PlatformConfig::default(),
            });
        }
        if std::env::var("WEIXIN_SESSION_KEY").is_ok() {
            platforms.push(PlatformConfigEntry {
                platform: Platform::Weixin,
                enabled: true,
                config: PlatformConfig::default(),
            });
        }
        if std::env::var("API_SERVER_PORT").is_ok() || std::env::var("API_SERVER_KEY").is_ok() {
            platforms.push(PlatformConfigEntry {
                platform: Platform::ApiServer,
                enabled: true,
                config: PlatformConfig::default(),
            });
        }
        if std::env::var("DINGTALK_CLIENT_ID").is_ok() {
            platforms.push(PlatformConfigEntry {
                platform: Platform::Dingtalk,
                enabled: true,
                config: PlatformConfig::default(),
            });
        }
        if std::env::var("WECOM_BOT_ID").is_ok() {
            platforms.push(PlatformConfigEntry {
                platform: Platform::Wecom,
                enabled: true,
                config: PlatformConfig::default(),
            });
        }
    }

    GatewayConfig {
        platforms,
        default_model,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config_defaults() {
        let config = load_gateway_config();
        // Should have defaults even without config file
        assert!(!config.default_model.is_empty());
    }

    #[test]
    fn test_gateway_status() {
        let config = GatewayConfig {
            platforms: vec![],
            default_model: "test".to_string(),
        };
        let runner = GatewayRunner::new(config);
        let status = runner.status();
        assert!(!status.running);
        assert!(!status.feishu_configured);
        assert!(!status.weixin_configured);
        assert!(!status.api_server_configured);
        assert!(!status.dingtalk_configured);
        assert!(!status.wecom_configured);
    }
}
