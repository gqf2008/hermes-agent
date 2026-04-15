//! API Server adapter — OpenAI-compatible HTTP API for Hermes Agent.
//!
//! Mirrors the Python `gateway/platforms/api_server.py`.
//! Hosts an HTTP server with OpenAI Chat Completions endpoints so that
//! any OpenAI-compatible frontend (Open WebUI, LobeChat, ChatBox, etc.)
//! can connect to Hermes Agent.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;
use axum::{
    Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{Json, Sse},
    routing::{delete, get, post},
};
use futures::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, oneshot};
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tracing::{error, info};

use crate::config::Platform;
use crate::runner::MessageHandler;

/// API Server configuration.
#[derive(Debug, Clone)]
pub struct ApiServerConfig {
    pub port: u16,
    pub host: String,
    pub api_key: String,
}

impl Default for ApiServerConfig {
    fn default() -> Self {
        Self {
            port: std::env::var("API_SERVER_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8642),
            host: std::env::var("API_SERVER_HOST")
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "127.0.0.1".to_string()),
            api_key: std::env::var("API_SERVER_KEY").unwrap_or_default(),
        }
    }
}

impl ApiServerConfig {
    pub fn from_env() -> Self {
        Self::default()
    }
}

/// Shared state passed to route handlers via axum State.
#[derive(Clone)]
pub struct ApiServerState {
    pub handler: Arc<Mutex<Option<Arc<dyn MessageHandler>>>>,
    pub api_key: String,
}

/// OpenAI-style chat completion request.
#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub messages: Vec<Message>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    /// Previous response ID for multi-turn session continuity.
    /// Mirrors Python PR 5cbb45d9 — reuses the stored session_id
    /// so the dashboard groups all turns under one session.
    #[serde(default)]
    pub previous_response_id: Option<String>,
}

/// OpenAI-style message.
#[derive(Debug, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(deserialize_with = "deserialize_content")]
    pub content: String,
}

/// OpenAI Responses API request.
/// Mirrors Python PR handling of `input`, `instructions`, `previous_response_id`,
/// `conversation`, `conversation_history`, `store`, `truncation`.
#[derive(Debug, Deserialize)]
pub struct ResponsesRequest {
    pub input: serde_json::Value,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub previous_response_id: Option<String>,
    #[serde(default)]
    pub conversation: Option<String>,
    #[serde(default)]
    pub conversation_history: Option<Vec<HistoryMessageInput>>,
    #[serde(default)]
    pub store: Option<bool>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub truncation: Option<String>,
}

/// Input message for Responses API (simpler than chat completions Message).
#[derive(Debug, Deserialize)]
pub struct HistoryMessageInput {
    pub role: String,
    #[serde(deserialize_with = "deserialize_content")]
    pub content: String,
}

/// Deserialize content that may be a string or an array of content parts.
fn deserialize_content<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    struct ContentVisitor;
    impl<'de> Visitor<'de> for ContentVisitor {
        type Value = String;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string or an array of content parts")
        }
        fn visit_str<E>(self, v: &str) -> Result<String, E>
        where
            E: de::Error,
        {
            Ok(v.to_string())
        }
        fn visit_seq<A>(self, mut seq: A) -> Result<String, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut parts: Vec<String> = Vec::new();
            while let Some(part) = seq.next_element::<serde_json::Value>()? {
                if let Some(text) = part.get("text").and_then(|v| v.as_str().map(String::from)) {
                    parts.push(text);
                } else if let Some(text) = part.as_str() {
                    parts.push(text.to_string());
                }
            }
            Ok(parts.join("\n"))
        }
    }
    deserializer.deserialize_any(ContentVisitor)
}

/// OpenAI-style chat completion response (non-streaming).
#[derive(Debug, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize)]
pub struct Choice {
    pub index: usize,
    pub message: ResponseMessage,
    pub finish_reason: String,
}

#[derive(Debug, Serialize)]
pub struct ResponseMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct Usage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

/// Response store entry — holds full response object for Responses API
/// chaining / GET retrieval. Mirrors Python ResponseStore class.
#[derive(Debug, Clone)]
pub struct ResponseStoreEntry {
    /// Full response data (for GET /v1/responses/{id})
    pub response_data: Option<ResponseData>,
    /// Conversation history with tool calls (for previous_response_id chaining)
    pub conversation_history: Vec<HistoryMessage>,
    /// Ephemeral system instructions (carried forward on chain)
    pub instructions: Option<String>,
    /// Session ID for dashboard grouping
    pub session_id: String,
    /// Conversation name (optional, for name-based chaining)
    pub conversation: Option<String>,
}

/// A message in conversation history.
#[derive(Debug, Clone)]
pub struct HistoryMessage {
    pub role: String,
    pub content: String,
}

/// In-memory response store. Mirrors Python's SQLite-backed ResponseStore.
/// Holds full response objects for Responses API stateful chaining.
static RESPONSE_STORE: LazyLock<std::sync::Mutex<ResponseStore>> =
    LazyLock::new(|| std::sync::Mutex::new(ResponseStore::default()));

/// Maximum entries before LRU eviction.
const RESPONSE_STORE_MAX: usize = 100;

/// SQLite-free in-memory response store with LRU eviction.
#[derive(Default)]
struct ResponseStore {
    entries: HashMap<String, ResponseStoreEntry>,
    /// Insertion order for LRU eviction (oldest first).
    order: Vec<String>,
    /// Conversation name -> response_id mapping.
    conversations: HashMap<String, String>,
}

impl ResponseStore {
    fn get(&self, response_id: &str) -> Option<&ResponseStoreEntry> {
        self.entries.get(response_id)
    }

    fn put(&mut self, response_id: String, entry: ResponseStoreEntry) {
        // If conversation name provided, map it
        if let Some(ref conv) = entry.conversation {
            self.conversations.insert(conv.clone(), response_id.clone());
        }
        self.entries.insert(response_id.clone(), entry);
        self.order.push(response_id);
        // LRU eviction
        while self.entries.len() > RESPONSE_STORE_MAX {
            if let Some(oldest) = self.order.first().cloned() {
                self.entries.remove(&oldest);
                self.order.remove(0);
            } else {
                break;
            }
        }
    }

    fn delete(&mut self, response_id: &str) -> bool {
        if self.entries.remove(response_id).is_some() {
            self.order.retain(|id| id != response_id);
            true
        } else {
            false
        }
    }

    fn get_conversation(&self, name: &str) -> Option<&String> {
        self.conversations.get(name)
    }

    fn set_conversation(&mut self, name: String, response_id: String) {
        self.conversations.insert(name, response_id);
    }
}

/// OpenAI Responses API response data.
#[derive(Debug, Clone, Serialize)]
pub struct ResponseData {
    pub id: String,
    pub object: String,
    pub status: String,
    pub created_at: i64,
    pub model: String,
    pub output: Vec<OutputItem>,
    pub usage: ResponseUsage,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputItem {
    #[serde(rename = "type")]
    pub item_type: String,
    pub role: String,
    pub content: Vec<ContentPart>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Vec<ContentPart>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContentPart {
    #[serde(rename = "type")]
    pub part_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponseUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub total_tokens: usize,
}

/// OpenAI-style model list response.
#[derive(Debug, Serialize)]
pub struct ModelsResponse {
    pub object: String,
    pub data: Vec<ModelInfo>,
}

#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub owned_by: String,
}

/// Health check response.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
}

/// API Server adapter — holds config, builds the HTTP router.
pub struct ApiServerAdapter {
    pub config: ApiServerConfig,
}

impl ApiServerAdapter {
    pub fn new(config: ApiServerConfig) -> Self {
        Self { config }
    }

    /// Build the axum router with all API endpoints.
    pub fn build_router(&self, state: ApiServerState) -> Router {
        let cors = CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods([axum::http::Method::GET, axum::http::Method::POST, axum::http::Method::OPTIONS])
            .allow_headers([axum::http::header::CONTENT_TYPE, axum::http::header::AUTHORIZATION]);

        Router::new()
            .route("/health", get(health_handler))
            .route("/v1/models", get(models_handler))
            .route("/v1/chat/completions", post(chat_completions_handler))
            .route("/v1/responses", post(responses_handler))
            .route("/v1/responses/{response_id}", get(get_response_handler))
            .route("/v1/responses/{response_id}", delete(delete_response_handler))
            .layer(cors)
            .layer(RequestBodyLimitLayer::new(1024 * 1024)) // 1MB max request body
            .with_state(state)
    }

    /// Run the HTTP server with graceful shutdown.
    /// Returns a oneshot sender that should be triggered to stop the server.
    pub async fn run(
        &self,
        state: ApiServerState,
        shutdown_rx: oneshot::Receiver<()>,
    ) -> Result<(), String> {
        let app = self.build_router(state);
        let listener = match tokio::net::TcpListener::bind(format!("{}:{}", self.config.host, self.config.port)).await {
            Ok(l) => l,
            Err(e) => return Err(format!("Failed to bind API server: {e}")),
        };
        let addr = format!("{}:{}", self.config.host, self.config.port);
        info!("API Server listening on http://{addr}");

        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
        {
            error!("API server error: {e}");
            return Err(format!("API server error: {e}"));
        }
        info!("API Server stopped gracefully");
        Ok(())
    }
}

// ── Route Handlers ──────────────────────────────────────────────

async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
    })
}

async fn models_handler() -> Json<ModelsResponse> {
    Json(ModelsResponse {
        object: "list".to_string(),
        data: vec![ModelInfo {
            id: "hermes-agent".to_string(),
            object: "model".to_string(),
            created: chrono::Utc::now().timestamp(),
            owned_by: "nous-research".to_string(),
        }],
    })
}

/// SSE chunk response for streaming mode.
/// Mirrors OpenAI's `chat.completion.chunk` format.
#[derive(Debug, Serialize)]
pub struct StreamChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<StreamChoice>,
}

#[derive(Debug, Serialize)]
pub struct StreamChoice {
    pub index: usize,
    pub delta: StreamDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StreamDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

async fn chat_completions_handler(
    State(state): State<ApiServerState>,
    headers: HeaderMap,
    Json(request): Json<ChatCompletionRequest>,
) -> Result<SseOrJson, (StatusCode, String)> {
    // Bearer token auth
    if !state.api_key.is_empty() {
        let auth = headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if auth != format!("Bearer {}", state.api_key) {
            return Err((StatusCode::UNAUTHORIZED, "Invalid API key".to_string()));
        }
    }

    // Extract the last user message
    let user_message = request
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_default();

    if user_message.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "No user message found".to_string()));
    }

    // Determine session ID — chain from previous_response_id if available.
    // Priority: explicit session_id > stored session_id from previous response > fresh UUID.
    let stored_session_id = if let Some(ref prev_id) = request.previous_response_id {
        RESPONSE_STORE.lock().unwrap().get(prev_id).map(|e| e.session_id.clone())
    } else {
        None
    };

    let session_id = request
        .session_id
        .or(stored_session_id)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Call the agent handler
    let handler_guard = state.handler.lock().await;
    let Some(handler) = handler_guard.as_ref() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "No message handler registered".to_string(),
        ));
    };

    let result = handler
        .handle_message(
            Platform::ApiServer,
            &session_id,
            &user_message,
        )
        .await
        .map_err(|e| {
            error!("Agent handler failed for API request: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Agent handler error: {e}"),
            )
        })?;

    let response = result.response;

    let model = request.model.clone().unwrap_or_else(|| "hermes-agent".to_string());
    let chat_id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
    let created = chrono::Utc::now().timestamp();

    // Store session_id so subsequent requests with previous_response_id
    // can reuse the same session (multi-turn continuity).
    {
        let mut store = RESPONSE_STORE.lock().unwrap();
        store.put(chat_id.clone(), ResponseStoreEntry {
            response_data: None,
            conversation_history: vec![],
            instructions: None,
            session_id,
            conversation: None,
        });
    }

    if request.stream {
        // Streaming mode: emit SSE events
        Ok(SseOrJson::Sse(build_sse_stream(chat_id, model, created, response)))
    } else {
        // Non-streaming mode
        Ok(SseOrJson::Json(Json(ChatCompletionResponse {
            id: chat_id,
            object: "chat.completion".to_string(),
            created,
            model,
            choices: vec![Choice {
                index: 0,
                message: ResponseMessage {
                    role: "assistant".to_string(),
                    content: response,
                },
                finish_reason: "stop".to_string(),
            }],
            usage: Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
        })))
    }
}

// ── Responses API Handlers ──────────────────────────────────────

/// POST /v1/responses — OpenAI Responses API format.
/// Stateful via previous_response_id; supports conversation naming,
/// explicit conversation_history, store flag, and truncation.
/// Mirrors Python _handle_responses (lines 1393–1645).
async fn responses_handler(
    State(state): State<ApiServerState>,
    headers: HeaderMap,
    Json(request): Json<ResponsesRequest>,
) -> Result<Json<ResponseData>, (StatusCode, String)> {
    // Bearer token auth
    if !state.api_key.is_empty() {
        let auth = headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if auth != format!("Bearer {}", state.api_key) {
            return Err((StatusCode::UNAUTHORIZED, "Invalid API key".to_string()));
        }
    }

    // Normalize input to message list
    let input_messages = normalize_responses_input(&request.input)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    // conversation and previous_response_id are mutually exclusive
    if request.conversation.is_some() && request.previous_response_id.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Cannot use both 'conversation' and 'previous_response_id'".to_string(),
        ));
    }

    // Resolve conversation name to latest response_id
    let mut prev_id = request.previous_response_id.clone();
    if let Some(ref conv) = request.conversation {
        let store = RESPONSE_STORE.lock().unwrap();
        if let Some(resp_id) = store.get_conversation(conv) {
            prev_id = Some(resp_id.clone());
        }
        // No error if conversation doesn't exist — it's a new conversation
    }

    // Accept explicit conversation_history from request body.
    // Precedence: conversation_history > previous_response_id.
    let mut conversation_history: Vec<HistoryMessage> = Vec::new();
    let mut stored_session_id: Option<String> = None;
    let mut stored_instructions: Option<String> = None;

    if let Some(ref raw_history) = request.conversation_history {
        for entry in raw_history {
            conversation_history.push(HistoryMessage {
                role: entry.role.clone(),
                content: entry.content.clone(),
            });
        }
    } else if let Some(ref prev_resp_id) = prev_id {
        let store = RESPONSE_STORE.lock().unwrap();
        if let Some(stored) = store.get(prev_resp_id) {
            conversation_history = stored.conversation_history.clone();
            stored_session_id = Some(stored.session_id.clone());
            stored_instructions = stored.instructions.clone();
        } else {
            return Err((
                StatusCode::NOT_FOUND,
                format!("Previous response not found: {}", prev_resp_id),
            ));
        }
    }

    // Carry forward instructions if not provided
    let instructions = request.instructions.or(stored_instructions);

    // Append all but last input message to history
    let all_but_last: Vec<_> = input_messages.iter().take(input_messages.len().saturating_sub(1)).cloned().collect();
    for msg in all_but_last {
        conversation_history.push(msg);
    }

    // Last input message is the user_message
    let user_message = input_messages
        .last()
        .map(|m| m.content.clone())
        .unwrap_or_default();

    if user_message.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "No user message found in input".to_string()));
    }

    // Truncation support: auto-truncate to last 100 messages
    if request.truncation.as_deref() == Some("auto") && conversation_history.len() > 100 {
        conversation_history = conversation_history.split_off(conversation_history.len() - 100);
    }

    // Reuse session from previous_response_id chain
    let session_id = stored_session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let store_response = request.store.unwrap_or(true);

    // Call the agent handler
    let handler_guard = state.handler.lock().await;
    let Some(handler) = handler_guard.as_ref() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "No message handler registered".to_string(),
        ));
    };

    let result = handler
        .handle_message(
            Platform::ApiServer,
            &session_id,
            &user_message,
        )
        .await
        .map_err(|e| {
            error!("Agent handler failed for responses request: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Agent handler error: {e}"),
            )
        })?;

    let response_text = result.response;
    let model = request.model.clone().unwrap_or_else(|| "hermes-agent".to_string());
    let response_id = format!("resp_{}", uuid::Uuid::new_v4().simple().to_string().chars().take(28).collect::<String>());
    let created_at = chrono::Utc::now().timestamp();

    // Build output items (Responses API format)
    let output_items = vec![OutputItem {
        item_type: "message".to_string(),
        role: "assistant".to_string(),
        content: vec![ContentPart {
            part_type: "output_text".to_string(),
            text: Some(response_text.clone()),
        }],
        call_id: None,
        name: None,
        arguments: None,
        output: None,
    }];

    let response_data = ResponseData {
        id: response_id.clone(),
        object: "response".to_string(),
        status: "completed".to_string(),
        created_at,
        model,
        output: output_items.clone(),
        usage: ResponseUsage {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
        },
    };

    // Store for future chaining if requested
    if store_response {
        let mut store = RESPONSE_STORE.lock().unwrap();
        // Build full history: existing history + user message + assistant response
        let mut full_history = conversation_history;
        full_history.push(HistoryMessage {
            role: "user".to_string(),
            content: user_message,
        });
        full_history.push(HistoryMessage {
            role: "assistant".to_string(),
            content: response_text,
        });

        store.put(response_id.clone(), ResponseStoreEntry {
            response_data: Some(response_data.clone()),
            conversation_history: full_history,
            instructions,
            session_id,
            conversation: request.conversation.clone(),
        });

        // Update conversation mapping
        if let Some(conv) = request.conversation {
            store.set_conversation(conv, response_id);
        }
    }

    Ok(Json(response_data))
}

/// GET /v1/responses/{response_id} — retrieve a stored response.
async fn get_response_handler(
    State(state): State<ApiServerState>,
    headers: HeaderMap,
    Path(response_id): Path<String>,
) -> Result<Json<ResponseData>, (StatusCode, String)> {
    // Bearer token auth
    if !state.api_key.is_empty() {
        let auth = headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if auth != format!("Bearer {}", state.api_key) {
            return Err((StatusCode::UNAUTHORIZED, "Invalid API key".to_string()));
        }
    }

    let store = RESPONSE_STORE.lock().unwrap();
    let Some(entry) = store.get(&response_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("Response not found: {}", response_id),
        ));
    };

    let Some(ref data) = entry.response_data else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("Response data not available for: {}", response_id),
        ));
    };

    Ok(Json(data.clone()))
}

/// DELETE /v1/responses/{response_id} — delete a stored response.
async fn delete_response_handler(
    State(state): State<ApiServerState>,
    headers: HeaderMap,
    Path(response_id): Path<String>,
) -> Result<Json<DeleteResponseResult>, (StatusCode, String)> {
    // Bearer token auth
    if !state.api_key.is_empty() {
        let auth = headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if auth != format!("Bearer {}", state.api_key) {
            return Err((StatusCode::UNAUTHORIZED, "Invalid API key".to_string()));
        }
    }

    let deleted = RESPONSE_STORE.lock().unwrap().delete(&response_id);
    if !deleted {
        return Err((
            StatusCode::NOT_FOUND,
            format!("Response not found: {}", response_id),
        ));
    }

    Ok(Json(DeleteResponseResult {
        id: response_id,
        object: "response".to_string(),
        deleted: true,
    }))
}

#[derive(Debug, Serialize)]
pub struct DeleteResponseResult {
    pub id: String,
    pub object: String,
    pub deleted: bool,
}

/// Normalize Responses API input into a list of history messages.
/// Accepts: string, or array of message objects / strings.
fn normalize_responses_input(
    input: &serde_json::Value,
) -> Result<Vec<HistoryMessage>, String> {
    if let Some(s) = input.as_str() {
        return Ok(vec![HistoryMessage {
            role: "user".to_string(),
            content: s.to_string(),
        }]);
    }

    if let Some(arr) = input.as_array() {
        let mut messages = Vec::new();
        for item in arr {
            if let Some(s) = item.as_str() {
                if !s.is_empty() {
                    messages.push(HistoryMessage {
                        role: "user".to_string(),
                        content: s.to_string(),
                    });
                }
            } else if let Some(obj) = item.as_object() {
                let role = obj
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("user")
                    .to_string();
                let content = extract_content_from_value(obj.get("content"));
                if !content.is_empty() {
                    messages.push(HistoryMessage { role, content });
                }
            }
        }
        return Ok(messages);
    }

    Err("'input' must be a string or array".to_string())
}

/// Extract content text from a JSON value (handles string or content part objects).
fn extract_content_from_value(value: Option<&serde_json::Value>) -> String {
    let Some(value) = value else { return String::new() };

    if let Some(s) = value.as_str() {
        return s.to_string();
    }

    // Try to extract from content part objects
    if let Some(obj) = value.as_object() {
        // Direct text field
        if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
            return text.to_string();
        }
    }

    // Try as array of content parts
    if let Some(arr) = value.as_array() {
        let mut parts: Vec<String> = Vec::new();
        for part in arr {
            if let Some(s) = part.as_str() {
                parts.push(s.to_string());
            } else if let Some(obj) = part.as_object() {
                if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                    parts.push(text.to_string());
                }
            }
        }
        return parts.join("\n");
    }

    String::new()
}

/// Stream type for SSE responses.
type SseStreamType = Pin<Box<dyn Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>> + Send>>;

/// Union type for streaming or non-streaming response.
enum SseOrJson {
    Sse(Sse<SseStreamType>),
    Json(Json<ChatCompletionResponse>),
}

#[async_trait::async_trait]
impl axum::response::IntoResponse for SseOrJson {
    fn into_response(self) -> axum::response::Response {
        match self {
            SseOrJson::Sse(sse) => sse.into_response(),
            SseOrJson::Json(json) => json.into_response(),
        }
    }
}

/// Build an SSE stream from a complete response text.
/// Splits the text into character-level chunks and emits them as delta events.
fn build_sse_stream(
    chat_id: String,
    model: String,
    created: i64,
    response: String,
) -> Sse<SseStreamType> {
    let stream: SseStreamType = Box::pin(async_stream::stream! {
        // First event: role announcement
        let role_chunk = StreamChunk {
            id: chat_id.clone(),
            object: "chat.completion.chunk".to_string(),
            created,
            model: model.clone(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: Some("assistant".to_string()),
                    content: None,
                },
                finish_reason: None,
            }],
        };
        let event = axum::response::sse::Event::default()
            .json_data(&role_chunk).unwrap();
        yield Ok::<_, std::convert::Infallible>(event);

        // Split response into character chunks (3 chars per chunk for pacing)
        let mut chars = response.chars().peekable();
        while chars.peek().is_some() {
            let text: String = chars.by_ref().take(3).collect();
            if text.is_empty() {
                break;
            }
            let content_chunk = StreamChunk {
                id: chat_id.clone(),
                object: "chat.completion.chunk".to_string(),
                created,
                model: model.clone(),
                choices: vec![StreamChoice {
                    index: 0,
                    delta: StreamDelta {
                        role: None,
                        content: Some(text),
                    },
                    finish_reason: None,
                }],
            };
            let event = axum::response::sse::Event::default()
                .json_data(&content_chunk).unwrap();
            yield Ok(event);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        // Final event: finish reason
        let finish_chunk = StreamChunk {
            id: chat_id.clone(),
            object: "chat.completion.chunk".to_string(),
            created,
            model: model.clone(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: None,
                    content: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
        };
        let event = axum::response::sse::Event::default()
            .json_data(&finish_chunk).unwrap();
        yield Ok(event);

        // Send [DONE] marker
        let done_event = axum::response::sse::Event::default()
            .data("[DONE]");
        yield Ok(done_event);
    });

    Sse::new(stream)
        .keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text(": ping"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = ApiServerConfig::default();
        assert_eq!(config.port, 8642);
        assert_eq!(config.host, "127.0.0.1");
        assert!(config.api_key.is_empty());
    }

    #[test]
    fn test_config_from_env() {
        let config = ApiServerConfig::from_env();
        assert!(config.port > 0);
    }

    #[test]
    fn test_health_response() {
        let resp = HealthResponse { status: "ok".to_string() };
        assert_eq!(resp.status, "ok");
    }

    #[test]
    fn test_models_response() {
        let resp = ModelsResponse {
            object: "list".to_string(),
            data: vec![ModelInfo {
                id: "hermes-agent".to_string(),
                object: "model".to_string(),
                created: 0,
                owned_by: "nous-research".to_string(),
            }],
        };
        assert_eq!(resp.data.len(), 1);
        assert_eq!(resp.data[0].id, "hermes-agent");
    }

    #[test]
    fn test_stream_chunk_serializes_cleanly() {
        // Ensure StreamChunk serializes to valid JSON without extra wrapping
        let chunk = StreamChunk {
            id: "test-id".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 0,
            model: "hermes-agent".to_string(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: Some("assistant".to_string()),
                    content: Some("Hello".to_string()),
                },
                finish_reason: None,
            }],
        };

        let json = serde_json::to_string(&chunk).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // Verify structure matches OpenAI format
        assert_eq!(parsed["id"], "test-id");
        assert_eq!(parsed["object"], "chat.completion.chunk");
        assert_eq!(parsed["model"], "hermes-agent");
        assert_eq!(parsed["choices"][0]["delta"]["content"], "Hello");

        // The JSON should NOT contain double-wrapped "data: " prefix
        assert!(!json.contains("data: data:"));
    }

    #[test]
    fn test_response_store_session_chain() {
        // Simulate: request 1 has no session_id, gets a UUID.
        // Request 2 uses previous_response_id from response 1,
        // and should inherit the same session_id.

        // Clear store first
        RESPONSE_STORE.lock().unwrap().entries.clear();
        RESPONSE_STORE.lock().unwrap().order.clear();
        RESPONSE_STORE.lock().unwrap().conversations.clear();

        // Simulate first turn
        let response_id_1 = "chatcmpl-first".to_string();
        let session_id_1 = "session-abc".to_string();
        RESPONSE_STORE.lock().unwrap().put(response_id_1.clone(), ResponseStoreEntry {
            response_data: None,
            conversation_history: vec![],
            instructions: None,
            session_id: session_id_1.clone(),
            conversation: None,
        });

        // Simulate second turn with previous_response_id
        let store = RESPONSE_STORE.lock().unwrap();
        let stored = store.get(&response_id_1).map(|e| e.session_id.clone());
        assert_eq!(stored, Some(session_id_1));

        // Without chain, new request should fall back to default
        let no_chain = store.get("nonexistent").map(|e| e.session_id.clone());
        assert_eq!(no_chain, None);
    }

    #[test]
    fn test_response_store_trim_on_overflow() {
        RESPONSE_STORE.lock().unwrap().entries.clear();
        RESPONSE_STORE.lock().unwrap().order.clear();
        RESPONSE_STORE.lock().unwrap().conversations.clear();

        // Insert RESPONSE_STORE_MAX + 1 entries
        for i in 0..=RESPONSE_STORE_MAX {
            RESPONSE_STORE.lock().unwrap().put(
                format!("resp-{i}"),
                ResponseStoreEntry {
                    response_data: None,
                    conversation_history: vec![],
                    instructions: None,
                    session_id: format!("session-{i}"),
                    conversation: None,
                },
            );
        }

        // Next insert should trigger LRU eviction
        {
            RESPONSE_STORE.lock().unwrap().put(
                "resp-new".to_string(),
                ResponseStoreEntry {
                    response_data: None,
                    conversation_history: vec![],
                    instructions: None,
                    session_id: "session-new".to_string(),
                    conversation: None,
                },
            );
        }

        // Store should have exactly RESPONSE_STORE_MAX entries
        let store = RESPONSE_STORE.lock().unwrap();
        assert_eq!(store.entries.len(), RESPONSE_STORE_MAX);
        // The new entry should be present
        assert!(store.entries.contains_key("resp-new"));
        // The oldest entry ("resp-0") should have been evicted
        assert!(!store.entries.contains_key("resp-0"));
    }

    #[test]
    fn test_response_store_conversation_mapping() {
        RESPONSE_STORE.lock().unwrap().entries.clear();
        RESPONSE_STORE.lock().unwrap().order.clear();
        RESPONSE_STORE.lock().unwrap().conversations.clear();

        // Store with conversation name
        RESPONSE_STORE.lock().unwrap().put(
            "resp-1".to_string(),
            ResponseStoreEntry {
                response_data: None,
                conversation_history: vec![],
                instructions: None,
                session_id: "session-1".to_string(),
                conversation: Some("my-chat".to_string()),
            },
        );

        // Lookup by conversation name
        let store = RESPONSE_STORE.lock().unwrap();
        let resp_id = store.get_conversation("my-chat");
        assert_eq!(resp_id, Some(&"resp-1".to_string()));

        // Unknown conversation
        assert!(store.get_conversation("unknown").is_none());
    }

    #[test]
    fn test_response_store_delete() {
        RESPONSE_STORE.lock().unwrap().entries.clear();
        RESPONSE_STORE.lock().unwrap().order.clear();
        RESPONSE_STORE.lock().unwrap().conversations.clear();

        RESPONSE_STORE.lock().unwrap().put(
            "resp-to-delete".to_string(),
            ResponseStoreEntry {
                response_data: None,
                conversation_history: vec![],
                instructions: None,
                session_id: "session-x".to_string(),
                conversation: None,
            },
        );

        assert!(RESPONSE_STORE.lock().unwrap().delete("resp-to-delete"));
        assert!(!RESPONSE_STORE.lock().unwrap().delete("resp-to-delete")); // already gone
        assert!(RESPONSE_STORE.lock().unwrap().get("resp-to-delete").is_none());
    }

    #[test]
    fn test_responses_input_normalization() {
        // String input
        let input = serde_json::json!("Hello");
        let msgs = normalize_responses_input(&input).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "Hello");

        // Array of strings
        let input = serde_json::json!(["Hello", "World"]);
        let msgs = normalize_responses_input(&input).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "Hello");
        assert_eq!(msgs[1].content, "World");

        // Array of message objects
        let input = serde_json::json!([
            {"role": "user", "content": "Hi"},
            {"role": "assistant", "content": "Hey"}
        ]);
        let msgs = normalize_responses_input(&input).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[1].role, "assistant");
    }

    #[test]
    fn test_response_data_serializes() {
        let data = ResponseData {
            id: "resp_test".to_string(),
            object: "response".to_string(),
            status: "completed".to_string(),
            created_at: 0,
            model: "hermes-agent".to_string(),
            output: vec![OutputItem {
                item_type: "message".to_string(),
                role: "assistant".to_string(),
                content: vec![ContentPart {
                    part_type: "output_text".to_string(),
                    text: Some("Hello world".to_string()),
                }],
                call_id: None,
                name: None,
                arguments: None,
                output: None,
            }],
            usage: ResponseUsage {
                input_tokens: 10,
                output_tokens: 20,
                total_tokens: 30,
            },
        };

        let json = serde_json::to_string(&data).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["id"], "resp_test");
        assert_eq!(parsed["object"], "response");
        assert_eq!(parsed["output"][0]["role"], "assistant");
        assert_eq!(parsed["output"][0]["content"][0]["text"], "Hello world");
    }
}
