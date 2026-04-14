//! API Server adapter — OpenAI-compatible HTTP API for Hermes Agent.
//!
//! Mirrors the Python `gateway/platforms/api_server.py`.
//! Hosts an HTTP server with OpenAI Chat Completions endpoints so that
//! any OpenAI-compatible frontend (Open WebUI, LobeChat, ChatBox, etc.)
//! can connect to Hermes Agent.

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Json, Sse},
    routing::{get, post},
};
use futures::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, oneshot};
use tower_http::cors::{Any, CorsLayer};
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
}

/// OpenAI-style message.
#[derive(Debug, Deserialize)]
pub struct Message {
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
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        Router::new()
            .route("/health", get(health_handler))
            .route("/v1/models", get(models_handler))
            .route("/v1/chat/completions", post(chat_completions_handler))
            .layer(cors)
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

    // Determine session ID
    let session_id = request
        .session_id
        .unwrap_or_else(|| "api-server-default".to_string());

    // Call the agent handler
    let handler_guard = state.handler.lock().await;
    let Some(handler) = handler_guard.as_ref() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "No message handler registered".to_string(),
        ));
    };

    let response = handler
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

    let model = request.model.clone().unwrap_or_else(|| "hermes-agent".to_string());
    let chat_id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
    let created = chrono::Utc::now().timestamp();

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
            .data(format!("data: {}\n\n", serde_json::to_string(&role_chunk).unwrap()));
        yield Ok::<_, std::convert::Infallible>(event);

        // Split response into character chunks (3 chars per chunk for pacing)
        let chars: Vec<char> = response.chars().collect();
        let chunk_size = 3;
        for chunk in chars.chunks(chunk_size) {
            let text: String = chunk.iter().collect();
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
                .data(format!("data: {}\n\n", serde_json::to_string(&content_chunk).unwrap()));
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
            .data(format!("data: {}\n\ndata: [DONE]\n\n", serde_json::to_string(&finish_chunk).unwrap()));
        yield Ok(event);
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
}
