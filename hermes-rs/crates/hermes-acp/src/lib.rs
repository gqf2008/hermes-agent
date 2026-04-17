/// Agent Client Protocol (ACP) server for hermes-agent.
///
/// Exposes Hermes Agent to IDE extensions (VS Code, Zed, JetBrains) via
/// JSON-RPC 2.0 over stdin/stdout, matching the `agent-client-protocol` spec.
///
/// # Protocol Methods
/// | Method | Direction | Description |
/// |--------|-----------|-------------|
/// | `initialize` | client → agent | Handshake, returns agent capabilities |
/// | `session/new` | client → agent | Create a new session |
/// | `session/load` | client → agent | Load an existing session |
/// | `session/resume` | client → agent | Resume a previous session |
/// | `session/list` | client → agent | List all active sessions |
/// | `session/get` | client → agent | Get session details |
/// | `session/fork` | client → agent | Fork a session |
/// | `prompt` | client → agent | Send a message to the agent |
/// | `session/cancel` | client → agent | Cancel in-progress request |
/// | `tools/list` | client → agent | List available tools |
///
/// # Python Reference
/// See `acp_adapter/server.py` (HermesACPAgent) for the full Python implementation.
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::sync::Arc;

use indexmap::IndexMap;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::watch;

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 types
// ---------------------------------------------------------------------------

/// A JSON-RPC 2.0 request from the IDE client.
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: Option<String>,
    pub method: String,
    pub params: Option<Value>,
    pub id: Option<Value>,
}

/// A JSON-RPC 2.0 response back to the IDE client.
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: Option<Value>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// A JSON-RPC 2.0 notification (no id, no response expected).
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

// Standard JSON-RPC error codes.
pub const PARSE_ERROR: i32 = -32700;
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;

// ---------------------------------------------------------------------------
// ACP protocol types (based on acp/schema Python types)
// ---------------------------------------------------------------------------

/// The ACP protocol version we implement.
/// Mirrors `acp.PROTOCOL_VERSION` from the Python agent-client-protocol package.
pub const ACP_PROTOCOL_VERSION: i64 = 1;

/// Agent version (mirrors `HERMES_VERSION`).
pub const AGENT_VERSION: &str = "0.1.0";

// --- Initialize ---

#[derive(Debug, Clone, Deserialize)]
pub struct InitializeRequest {
    pub protocol_version: Option<i64>,
    #[serde(default)]
    pub client_capabilities: Option<ClientCapabilities>,
    #[serde(default)]
    pub client_info: Option<Implementation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResponse {
    pub protocol_version: i64,
    pub agent_info: Implementation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_capabilities: Option<AgentCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_methods: Option<Vec<AuthMethodAgent>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Implementation {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fs: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_session: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_capabilities: Option<SessionCapabilities>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fork: Option<SessionForkCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<SessionListCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume: Option<SessionResumeCapabilities>,
}

impl Default for SessionCapabilities {
    fn default() -> Self {
        Self {
            fork: Some(SessionForkCapabilities),
            list: Some(SessionListCapabilities),
            resume: Some(SessionResumeCapabilities),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionForkCapabilities;
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionListCapabilities;
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionResumeCapabilities;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthMethodAgent {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// --- Session ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewSessionRequest {
    pub cwd: String,
    #[serde(default)]
    pub mcp_servers: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewSessionResponse {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadSessionRequest {
    pub cwd: String,
    pub session_id: String,
    #[serde(default)]
    pub mcp_servers: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoadSessionResponse {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeSessionRequest {
    pub cwd: String,
    pub session_id: String,
    #[serde(default)]
    pub mcp_servers: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResumeSessionResponse {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSessionsRequest {
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSessionsResponse {
    pub sessions: Vec<SessionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkSessionRequest {
    pub cwd: String,
    pub session_id: String,
    #[serde(default)]
    pub mcp_servers: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkSessionResponse {
    pub session_id: String,
}

// --- Prompt ---

/// A text content block within a prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextContentBlock {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

/// A prompt is a list of content blocks.
/// In the Python reference this is:
/// `list[TextContentBlock | ImageContentBlock | ...]`
pub type Prompt = Vec<PromptContentBlock>;

/// Simplified prompt content block — we handle text natively and
/// pass through other types as raw JSON.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PromptContentBlock {
    Text(TextContentBlock),
    Other(Value),
}

impl PromptContentBlock {
    /// Extract text from this block, returning empty string for non-text.
    pub fn as_text(&self) -> String {
        match self {
            Self::Text(t) => t.text.clone(),
            Self::Other(v) => v
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or_default()
                .to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PromptRequest {
    pub prompt: Prompt,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_read_tokens: Option<u64>,
}

// --- Cancel ---

#[derive(Debug, Clone, Deserialize)]
pub struct CancelRequest {
    pub session_id: String,
}

// --- Tools ---

/// A tool definition matching OpenAI function tool format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsListResponse {
    pub tools: Vec<ToolDefinition>,
}

// --- Available commands (slash commands) ---

#[derive(Debug, Clone, Serialize)]
pub struct AvailableCommand {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<UnstructuredCommandInput>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnstructuredCommandInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

// --- Session config / model / mode ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetSessionModelRequest {
    pub session_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetSessionModelResponse {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetSessionModeRequest {
    pub session_id: String,
    pub mode_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetSessionModeResponse {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetConfigOptionRequest {
    pub session_id: String,
    pub config_id: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetConfigOptionResponse {}

// ---------------------------------------------------------------------------
// Session state
// ---------------------------------------------------------------------------

/// In-memory session state, mirroring Python's `SessionState`.
#[derive(Debug)]
pub struct Session {
    pub session_id: String,
    pub cwd: String,
    pub history: Vec<serde_json::Map<String, Value>>,
    pub cancel_tx: Option<watch::Sender<bool>>,
    pub config_options: HashMap<String, String>,
    pub model: Option<String>,
    pub mode: Option<String>,
}

impl Session {
    fn new(session_id: String, cwd: String) -> Self {
        Self {
            session_id,
            cwd,
            history: Vec::new(),
            cancel_tx: None,
            config_options: HashMap::new(),
            model: None,
            mode: None,
        }
    }
}

/// Session manager mirroring Python's `SessionManager`.
#[derive(Debug, Default)]
pub struct SessionManager {
    sessions: Mutex<IndexMap<String, Arc<Mutex<Session>>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new session and return it.
    pub fn create_session(&self, cwd: String) -> Arc<Mutex<Session>> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let session = Arc::new(Mutex::new(Session::new(session_id.clone(), cwd)));
        self.sessions.lock().insert(session_id, Arc::clone(&session));
        session
    }

    /// Get a session by ID.
    pub fn get_session(&self, session_id: &str) -> Option<Arc<Mutex<Session>>> {
        self.sessions.lock().get(session_id).cloned()
    }

    /// List all sessions.
    pub fn list_sessions(&self) -> Vec<SessionInfo> {
        self.sessions
            .lock()
            .values()
            .map(|s| {
                let guard = s.lock();
                SessionInfo {
                    session_id: guard.session_id.clone(),
                    cwd: Some(guard.cwd.clone()),
                }
            })
            .collect()
    }

    /// Fork a session (copies history to new session).
    pub fn fork_session(&self, session_id: &str, cwd: String) -> Option<Arc<Mutex<Session>>> {
        let source = self.get_session(session_id)?;
        let new_id = uuid::Uuid::new_v4().to_string();
        let source_guard = source.lock();
        let mut new_session = Session::new(new_id.clone(), cwd);
        new_session.history = source_guard.history.clone();
        new_session.model = source_guard.model.clone();
        let new_session = Arc::new(Mutex::new(new_session));
        self.sessions.lock().insert(new_id, Arc::clone(&new_session));
        Some(new_session)
    }

    /// Cancel a session's in-progress work.
    pub fn cancel_session(&self, session_id: &str) {
        if let Some(session) = self.get_session(session_id) {
            let guard = session.lock();
            if let Some(tx) = &guard.cancel_tx {
                let _ = tx.send(true);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ACP Server
// ---------------------------------------------------------------------------

/// ACP server that handles JSON-RPC requests from IDE extensions.
///
/// Mirrors `HermesACPAgent` from `acp_adapter/server.py`.
pub struct AcpServer {
    /// Session store.
    session_manager: Arc<SessionManager>,
    /// Registered tool definitions. In a full implementation these come from
    /// the hermes-tools crate; here we provide a placeholder that can be
    /// extended via `register_tool()`.
    tools: Mutex<Vec<ToolDefinition>>,
    /// Slash commands advertised to the IDE.
    slash_commands: Mutex<Vec<AvailableCommand>>,
}

impl Default for AcpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl AcpServer {
    pub fn new() -> Self {
        let server = Self {
            session_manager: Arc::new(SessionManager::new()),
            tools: Mutex::new(Vec::new()),
            slash_commands: Mutex::new(Vec::new()),
        };
        server.register_default_commands();
        server
    }

    /// Register a tool definition. Call during setup to populate the tool surface.
    pub fn register_tool(&self, tool: ToolDefinition) {
        self.tools.lock().push(tool);
    }

    fn register_default_commands(&self) {
        let commands = vec![
            AvailableCommand {
                name: "help".into(),
                description: Some("List available commands".into()),
                input: None,
            },
            AvailableCommand {
                name: "model".into(),
                description: Some("Show current model and provider, or switch models".into()),
                input: Some(UnstructuredCommandInput {
                    hint: Some("model name to switch to".into()),
                }),
            },
            AvailableCommand {
                name: "tools".into(),
                description: Some("List available tools with descriptions".into()),
                input: None,
            },
            AvailableCommand {
                name: "reset".into(),
                description: Some("Clear conversation history".into()),
                input: None,
            },
            AvailableCommand {
                name: "compact".into(),
                description: Some("Compress conversation context".into()),
                input: None,
            },
            AvailableCommand {
                name: "version".into(),
                description: Some("Show Hermes version".into()),
                input: None,
            },
        ];
        *self.slash_commands.lock() = commands;
    }

    // --- Request handlers ---------------------------------------------------

    /// Handle `initialize` — handshake with the IDE.
    fn handle_initialize(&self, params: Option<Value>) -> Result<Value, JsonRpcError> {
        let req: InitializeRequest = params
            .map(|v| serde_json::from_value(v).map_err(|e| JsonRpcError {
                code: INVALID_PARAMS,
                message: format!("invalid initialize params: {e}"),
                data: None,
            }))
            .transpose()?
            .unwrap_or(InitializeRequest {
                protocol_version: None,
                client_capabilities: None,
                client_info: None,
            });

        let client_name = req
            .client_info
            .as_ref()
            .map(|i| i.name.as_str())
            .unwrap_or("unknown");

        tracing::info!("ACP initialize from {} (protocol v{:?})", client_name, req.protocol_version);

        let resp = InitializeResponse {
            protocol_version: ACP_PROTOCOL_VERSION,
            agent_info: Implementation {
                name: "hermes-agent".into(),
                version: Some(AGENT_VERSION.into()),
            },
            agent_capabilities: Some(AgentCapabilities {
                load_session: Some(true),
                session_capabilities: Some(SessionCapabilities::default()),
            }),
            auth_methods: None,
        };

        serde_json::to_value(resp).map_err(|e| JsonRpcError {
            code: INTERNAL_ERROR,
            message: format!("failed to serialize initialize response: {e}"),
            data: None,
        })
    }

    /// Handle `session/new` — create a new session.
    fn handle_new_session(&self, params: Option<Value>) -> Result<Value, JsonRpcError> {
        let req: NewSessionRequest = params
            .map(|v| serde_json::from_value(v).map_err(|e| JsonRpcError {
                code: INVALID_PARAMS,
                message: format!("invalid new_session params: {e}"),
                data: None,
            }))
            .transpose()?
            .ok_or_else(|| JsonRpcError {
                code: INVALID_PARAMS,
                message: "missing new_session params".into(),
                data: None,
            })?;

        let session = self.session_manager.create_session(req.cwd.clone());
        let session_id = session.lock().session_id.clone();

        tracing::info!("ACP new_session: {} (cwd={})", session_id, req.cwd);

        let resp = NewSessionResponse { session_id };
        serde_json::to_value(resp).map_err(|e| JsonRpcError {
            code: INTERNAL_ERROR,
            message: format!("failed to serialize new_session response: {e}"),
            data: None,
        })
    }

    /// Handle `session/load` — load an existing session.
    fn handle_load_session(&self, params: Option<Value>) -> Result<Value, JsonRpcError> {
        let req: LoadSessionRequest = params
            .map(|v| serde_json::from_value(v).map_err(|e| JsonRpcError {
                code: INVALID_PARAMS,
                message: format!("invalid load_session params: {e}"),
                data: None,
            }))
            .transpose()?
            .ok_or_else(|| JsonRpcError {
                code: INVALID_PARAMS,
                message: "missing load_session params".into(),
                data: None,            })?;

        let session = self.session_manager.get_session(&req.session_id).ok_or_else(|| JsonRpcError {
            code: INVALID_PARAMS,
            message: format!("session {} not found", req.session_id),
            data: None,
        })?;

        // Update cwd
        {
            let mut guard = session.lock();
            guard.cwd = req.cwd.clone();
        }

        tracing::info!("ACP load_session: {}", req.session_id);

        serde_json::to_value(LoadSessionResponse {}).map_err(|e| JsonRpcError {
            code: INTERNAL_ERROR,
            message: format!("failed to serialize load_session response: {e}"),
            data: None,
        })
    }

    /// Handle `session/resume` — resume a previous session.
    fn handle_resume_session(&self, params: Option<Value>) -> Result<Value, JsonRpcError> {
        let req: ResumeSessionRequest = params
            .map(|v| serde_json::from_value(v).map_err(|e| JsonRpcError {
                code: INVALID_PARAMS,
                message: format!("invalid resume_session params: {e}"),
                data: None,
            }))
            .transpose()?
            .ok_or_else(|| JsonRpcError {
                code: INVALID_PARAMS,
                message: "missing resume_session params".into(),
                data: None,
            })?;

        // If session doesn't exist, create a new one (matching Python behavior)
        let session = match self.session_manager.get_session(&req.session_id) {
            Some(s) => s,
            None => self.session_manager.create_session(req.cwd.clone()),
        };

        {
            let mut guard = session.lock();
            guard.cwd = req.cwd.clone();
        }

        tracing::info!("ACP resume_session: {}", req.session_id);

        serde_json::to_value(ResumeSessionResponse {}).map_err(|e| JsonRpcError {
            code: INTERNAL_ERROR,
            message: format!("failed to serialize resume_session response: {e}"),
            data: None,
        })
    }

    /// Handle `session/list` — list all sessions.
    fn handle_list_sessions(&self, _params: Option<Value>) -> Result<Value, JsonRpcError> {
        let sessions = self.session_manager.list_sessions();
        tracing::info!("ACP list_sessions: {} sessions", sessions.len());

        let resp = ListSessionsResponse { sessions };
        serde_json::to_value(resp).map_err(|e| JsonRpcError {
            code: INTERNAL_ERROR,
            message: format!("failed to serialize list_sessions response: {e}"),
            data: None,
        })
    }

    /// Handle `session/get` — get session details.
    fn handle_get_session(&self, params: Option<Value>) -> Result<Value, JsonRpcError> {
        let session_id: String = params
            .and_then(|v| v.get("session_id").and_then(|v| v.as_str()).map(String::from))
            .ok_or_else(|| JsonRpcError {
                code: INVALID_PARAMS,
                message: "missing session_id".into(),
                data: None,
            })?;

        let session = self.session_manager.get_session(&session_id).ok_or_else(|| JsonRpcError {
            code: INVALID_PARAMS,
            message: format!("session {} not found", session_id),
            data: None,
        })?;

        let guard = session.lock();
        let info = SessionInfo {
            session_id: guard.session_id.clone(),
            cwd: Some(guard.cwd.clone()),
        };

        serde_json::to_value(info).map_err(|e| JsonRpcError {
            code: INTERNAL_ERROR,
            message: format!("failed to serialize session info: {e}"),
            data: None,
        })
    }

    /// Handle `session/fork` — fork a session.
    fn handle_fork_session(&self, params: Option<Value>) -> Result<Value, JsonRpcError> {
        let req: ForkSessionRequest = params
            .map(|v| serde_json::from_value(v).map_err(|e| JsonRpcError {
                code: INVALID_PARAMS,
                message: format!("invalid fork_session params: {e}"),
                data: None,
            }))
            .transpose()?
            .ok_or_else(|| JsonRpcError {
                code: INVALID_PARAMS,
                message: "missing fork_session params".into(),
                data: None,
            })?;

        let new_session = self.session_manager.fork_session(&req.session_id, req.cwd.clone())
            .ok_or_else(|| JsonRpcError {
                code: INVALID_PARAMS,
                message: format!("session {} not found for fork", req.session_id),
                data: None,
            })?;

        let new_id = new_session.lock().session_id.clone();
        tracing::info!("ACP fork_session: {} -> {}", req.session_id, new_id);

        let resp = ForkSessionResponse { session_id: new_id };
        serde_json::to_value(resp).map_err(|e| JsonRpcError {
            code: INTERNAL_ERROR,
            message: format!("failed to serialize fork_session response: {e}"),
            data: None,
        })
    }

    /// Handle `prompt` — send a message to the agent.
    ///
    /// This is the core interaction. In the Python reference, this calls
    /// `agent.run_conversation()` and streams events back. Here we provide
    /// the protocol skeleton; the actual LLM call should be wired up by
    /// the embedding application.
    fn handle_prompt(&self, params: Option<Value>) -> Result<Value, JsonRpcError> {
        let req: PromptRequest = params
            .map(|v| serde_json::from_value(v).map_err(|e| JsonRpcError {
                code: INVALID_PARAMS,
                message: format!("invalid prompt params: {e}"),
                data: None,
            }))
            .transpose()?
            .ok_or_else(|| JsonRpcError {
                code: INVALID_PARAMS,
                message: "missing prompt params".into(),
                data: None,
            })?;

        let session = self.session_manager.get_session(&req.session_id).ok_or_else(|| JsonRpcError {
            code: INVALID_PARAMS,
            message: format!("session {} not found", req.session_id),
            data: None,
        })?;

        // Extract text from the prompt content blocks.
        let user_text: Vec<String> = req
            .prompt
            .iter()
            .map(|b| b.as_text())
            .filter(|t| !t.is_empty())
            .collect();
        let user_text = user_text.join("\n");

        if user_text.is_empty() {
            let resp = PromptResponse {
                stop_reason: Some("end_turn".into()),
                usage: None,
            };
            return serde_json::to_value(resp).map_err(|e| JsonRpcError {
                code: INTERNAL_ERROR,
                message: format!("failed to serialize prompt response: {e}"),
                data: None,
            });
        }

        // Intercept slash commands — handle locally without calling the LLM.
        if user_text.starts_with('/') {
            let response_text = self.handle_slash_command(&user_text, &session);
            let resp = PromptResponse {
                stop_reason: Some("end_turn".into()),
                usage: None,
            };
            // In a full implementation, we'd also emit a session_update
            // notification with the response text before returning.
            tracing::info!("ACP slash command: {}", user_text);
            let _ = response_text; // TODO: stream response_text via session_update notification
            return serde_json::to_value(resp).map_err(|e| JsonRpcError {
                code: INTERNAL_ERROR,
                message: format!("failed to serialize prompt response: {e}"),
                data: None,
            });
        }

        // Set up cancel channel for this request.
        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        {
            let mut guard = session.lock();
            guard.cancel_tx = Some(cancel_tx);
        }

        tracing::info!("ACP prompt on session {}: {:?}", req.session_id, &user_text.chars().take(100).collect::<String>());

        // In a real implementation, this is where you would:
        // 1. Build the conversation history
        // 2. Call the LLM (hermes-llm / hermes-agent-engine)
        // 3. Stream tool calls and thinking events via session_update notifications
        // 4. Collect the final response
        //
        // For now, we return a placeholder response.
        // The `cancel_rx` can be monitored to abort mid-generation.

        // Wait for the LLM response or cancellation.
        // In a real implementation you'd spawn the LLM call and monitor cancel_rx.
        let _ = &mut cancel_rx;

        let resp = PromptResponse {
            stop_reason: Some("end_turn".into()),
            usage: None,
        };

        serde_json::to_value(resp).map_err(|e| JsonRpcError {
            code: INTERNAL_ERROR,
            message: format!("failed to serialize prompt response: {e}"),
            data: None,
        })
    }

    /// Handle `session/cancel` — cancel an in-progress request.
    fn handle_cancel(&self, params: Option<Value>) -> Result<Value, JsonRpcError> {
        let req: CancelRequest = params
            .map(|v| serde_json::from_value(v).map_err(|e| JsonRpcError {
                code: INVALID_PARAMS,
                message: format!("invalid cancel params: {e}"),
                data: None,
            }))
            .transpose()?
            .ok_or_else(|| JsonRpcError {
                code: INVALID_PARAMS,
                message: "missing cancel params".into(),
                data: None,
            })?;

        self.session_manager.cancel_session(&req.session_id);
        tracing::info!("ACP cancel: session {}", req.session_id);

        Ok(Value::Null)
    }

    /// Handle `tools/list` — list available tools.
    fn handle_tools_list(&self, _params: Option<Value>) -> Result<Value, JsonRpcError> {
        let tools = self.tools.lock().clone();
        let resp = ToolsListResponse { tools };
        serde_json::to_value(resp).map_err(|e| JsonRpcError {
            code: INTERNAL_ERROR,
            message: format!("failed to serialize tools response: {e}"),
            data: None,
        })
    }

    /// Handle `set_session_model` — switch model for a session.
    fn handle_set_session_model(&self, params: Option<Value>) -> Result<Value, JsonRpcError> {
        let req: SetSessionModelRequest = params
            .map(|v| serde_json::from_value(v).map_err(|e| JsonRpcError {
                code: INVALID_PARAMS,
                message: format!("invalid set_session_model params: {e}"),
                data: None,
            }))
            .transpose()?
            .ok_or_else(|| JsonRpcError {
                code: INVALID_PARAMS,
                message: "missing set_session_model params".into(),
                data: None,
            })?;

        if let Some(session) = self.session_manager.get_session(&req.session_id) {
            session.lock().model = Some(req.model_id.clone());
            tracing::info!("ACP set_session_model: {} -> {}", req.session_id, req.model_id);
        }

        serde_json::to_value(SetSessionModelResponse {}).map_err(|e| JsonRpcError {
            code: INTERNAL_ERROR,
            message: format!("failed to serialize set_session_model response: {e}"),
            data: None,
        })
    }

    /// Handle `set_session_mode` — switch mode for a session.
    fn handle_set_session_mode(&self, params: Option<Value>) -> Result<Value, JsonRpcError> {
        let req: SetSessionModeRequest = params
            .map(|v| serde_json::from_value(v).map_err(|e| JsonRpcError {
                code: INVALID_PARAMS,
                message: format!("invalid set_session_mode params: {e}"),
                data: None,
            }))
            .transpose()?
            .ok_or_else(|| JsonRpcError {
                code: INVALID_PARAMS,
                message: "missing set_session_mode params".into(),
                data: None,
            })?;

        if let Some(session) = self.session_manager.get_session(&req.session_id) {
            session.lock().mode = Some(req.mode_id.clone());
            tracing::info!("ACP set_session_mode: {} -> {}", req.session_id, req.mode_id);
        }

        serde_json::to_value(SetSessionModeResponse {}).map_err(|e| JsonRpcError {
            code: INTERNAL_ERROR,
            message: format!("failed to serialize set_session_mode response: {e}"),
            data: None,
        })
    }

    /// Handle `set_config_option` — update a session config option.
    fn handle_set_config_option(&self, params: Option<Value>) -> Result<Value, JsonRpcError> {
        let req: SetConfigOptionRequest = params
            .map(|v| serde_json::from_value(v).map_err(|e| JsonRpcError {
                code: INVALID_PARAMS,
                message: format!("invalid set_config_option params: {e}"),
                data: None,
            }))
            .transpose()?
            .ok_or_else(|| JsonRpcError {
                code: INVALID_PARAMS,
                message: "missing set_config_option params".into(),
                data: None,
            })?;

        if let Some(session) = self.session_manager.get_session(&req.session_id) {
            session.lock().config_options.insert(req.config_id.clone(), req.value.clone());
            tracing::info!("ACP set_config_option: {} = {}", req.config_id, req.value);
        }

        serde_json::to_value(SetConfigOptionResponse {}).map_err(|e| JsonRpcError {
            code: INTERNAL_ERROR,
            message: format!("failed to serialize set_config_option response: {e}"),
            data: None,
        })
    }

    // --- Slash command handling (mirrors Python _handle_slash_command) ------

    fn handle_slash_command(&self, text: &str, session: &Arc<Mutex<Session>>) -> String {
        let parts: Vec<&str> = text.splitn(2, char::is_whitespace).collect();
        let cmd = parts[0].trim_start_matches('/').to_lowercase();
        let args = parts.get(1).map(|s| s.trim()).unwrap_or("");

        match cmd.as_str() {
            "help" => self.cmd_help(),
            "model" => self.cmd_model(args, session),
            "tools" => self.cmd_tools(),
            "reset" => self.cmd_reset(session),
            "compact" => "Context compression not yet implemented in Rust.".into(),
            "version" => format!("Hermes Agent v{AGENT_VERSION} (Rust)"),
            _ => String::new(), // Unknown command — let LLM handle it
        }
    }

    fn cmd_help(&self) -> String {
        let commands = self.slash_commands.lock();
        let mut lines = vec!["Available commands:".to_string(), String::new()];
        for cmd in commands.iter() {
            let desc = cmd.description.as_deref().unwrap_or("");
            lines.push(format!("  /{:<10}  {}", cmd.name, desc));
        }
        lines.push(String::new());
        lines.push("Unrecognized /commands are sent to the model as normal messages.".into());
        lines.join("\n")
    }

    fn cmd_model(&self, args: &str, session: &Arc<Mutex<Session>>) -> String {
        if args.is_empty() {
            let guard = session.lock();
            let model = guard.model.as_deref().unwrap_or("not set");
            format!("Current model: {model}")
        } else {
            let mut guard = session.lock();
            guard.model = Some(args.to_string());
            format!("Model switched to: {args}")
        }
    }

    fn cmd_tools(&self) -> String {
        let tools = self.tools.lock();
        if tools.is_empty() {
            return "No tools registered.".into();
        }
        let mut lines = vec![format!("Available tools ({}):", tools.len())];
        for tool in tools.iter() {
            let desc = tool.function.description.as_deref().unwrap_or("");
            lines.push(format!("  {}: {}", tool.function.name, desc));
        }
        lines.join("\n")
    }

    fn cmd_reset(&self, session: &Arc<Mutex<Session>>) -> String {
        session.lock().history.clear();
        "Conversation history cleared.".into()
    }

    // --- Notification helpers ------------------------------------------------

    /// Build a session_update notification with text content.
    /// Mirrors Python's `acp.update_agent_message_text()`.
    pub fn session_update_text(session_id: &str, text: &str) -> JsonRpcNotification {
        JsonRpcNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: Some(serde_json::json!({
                "session_id": session_id,
                "update": {
                    "sessionUpdate": "agent_message",
                    "content": [{
                        "type": "text",
                        "text": text
                    }]
                }
            })),
        }
    }

    /// Build a session_update notification with tool call start.
    /// Mirrors Python's `acp.start_tool_call()`.
    pub fn session_update_tool_start(
        session_id: &str,
        tool_call_id: &str,
        tool_name: &str,
        title: &str,
    ) -> JsonRpcNotification {
        JsonRpcNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: Some(serde_json::json!({
                "session_id": session_id,
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": tool_call_id,
                    "title": title,
                    "tool": tool_name,
                    "status": "running"
                }
            })),
        }
    }

    /// Build a session_update notification with tool call complete.
    pub fn session_update_tool_complete(
        session_id: &str,
        tool_call_id: &str,
        output: &str,
    ) -> JsonRpcNotification {
        JsonRpcNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: Some(serde_json::json!({
                "session_id": session_id,
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": tool_call_id,
                    "status": "completed",
                    "content": [{
                        "type": "text",
                        "text": output
                    }]
                }
            })),
        }
    }

    /// Build a session_update notification for thinking/throttling content.
    pub fn session_update_thinking(session_id: &str, text: &str) -> JsonRpcNotification {
        JsonRpcNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: Some(serde_json::json!({
                "session_id": session_id,
                "update": {
                    "sessionUpdate": "agent_throttle",
                    "content": [{
                        "type": "text",
                        "text": text
                    }]
                }
            })),
        }
    }

    /// Build an available_commands_update notification.
    pub fn available_commands_update(session_id: &str) -> JsonRpcNotification {
        let commands = self::AcpServer::new().slash_commands.lock().clone();
        JsonRpcNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: Some(serde_json::json!({
                "session_id": session_id,
                "update": {
                    "sessionUpdate": "available_commands_update",
                    "availableCommands": commands
                }
            })),
        }
    }

    // --- Main dispatch loop --------------------------------------------------

    /// Dispatch a single JSON-RPC request to the appropriate handler.
    pub fn dispatch(&self, request: &JsonRpcRequest) -> Option<JsonRpcResponse> {
        let result = match request.method.as_str() {
            "initialize" => self.handle_initialize(request.params.clone()),
            "session/new" => self.handle_new_session(request.params.clone()),
            "session/load" => self.handle_load_session(request.params.clone()),
            "session/resume" => self.handle_resume_session(request.params.clone()),
            "session/list" => self.handle_list_sessions(request.params.clone()),
            "session/get" => self.handle_get_session(request.params.clone()),
            "session/fork" => self.handle_fork_session(request.params.clone()),
            "session/cancel" => self.handle_cancel(request.params.clone()),
            "prompt" => self.handle_prompt(request.params.clone()),
            "tools/list" => self.handle_tools_list(request.params.clone()),
            "setSessionModel" | "session/setModel" => self.handle_set_session_model(request.params.clone()),
            "setSessionMode" | "session/setMode" => self.handle_set_session_mode(request.params.clone()),
            "setConfigOption" | "session/setConfigOption" => self.handle_set_config_option(request.params.clone()),
            _ => Err(JsonRpcError {
                code: METHOD_NOT_FOUND,
                message: format!("method not found: {}", request.method),
                data: None,
            }),
        };

        // Notifications (no id) don't get a response.
        let id = request.id.clone()?;

        match result {
            Ok(value) => Some(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                result: Some(value),
                error: None,
                id: Some(id),
            }),
            Err(err) => Some(JsonRpcResponse {
                jsonrpc: "2.0".into(),
                result: None,
                error: Some(err),
                id: Some(id),
            }),
        }
    }

    /// Write a single JSON value followed by a newline to stdout.
    fn write_response(&self, response: &JsonRpcResponse) -> io::Result<()> {
        let stdout = io::stdout();
        let mut lock = stdout.lock();
        serde_json::to_writer(&mut lock, response)?;
        writeln!(lock)?;
        lock.flush()?;
        Ok(())
    }

    /// Write a notification to stdout.
    pub fn write_notification(&self, notification: &JsonRpcNotification) -> io::Result<()> {
        let stdout = io::stdout();
        let mut lock = stdout.lock();
        serde_json::to_writer(&mut lock, notification)?;
        writeln!(lock)?;
        lock.flush()?;
        Ok(())
    }

    /// Run the ACP server, reading JSON-RPC requests from stdin and writing
    /// responses to stdout. This is a synchronous loop suitable for embedding
    /// in a CLI binary.
    ///
    /// Logging should be configured to write to stderr so stdout stays clean
    /// for the ACP JSON-RPC transport (mirrors Python's `_setup_logging()`).
    pub fn run_stdio(&self) -> io::Result<()> {
        let stdin = io::stdin();
        let reader = stdin.lock();

        for line in reader.lines() {
            let line = line?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Parse the JSON-RPC request.
            let request: JsonRpcRequest = match serde_json::from_str(line) {
                Ok(req) => req,
                Err(e) => {
                    let error_resp = JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        result: None,
                        error: Some(JsonRpcError {
                            code: PARSE_ERROR,
                            message: format!("parse error: {e}"),
                            data: None,
                        }),
                        id: None,
                    };
                    self.write_response(&error_resp)?;
                    continue;
                }
            };

            // Validate JSON-RPC version.
            if let Some(ref version) = request.jsonrpc {
                if version != "2.0" {
                    let error_resp = JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        result: None,
                        error: Some(JsonRpcError {
                            code: INVALID_REQUEST,
                            message: format!("invalid jsonrpc version: {version}"),
                            data: None,
                        }),
                        id: request.id.clone(),
                    };
                    self.write_response(&error_resp)?;
                    continue;
                }
            }

            // Dispatch and respond.
            if let Some(response) = self.dispatch(&request) {
                self.write_response(&response)?;
            }
        }

        Ok(())
    }

    /// Run the ACP server using async tokio I/O.
    ///
    /// This is the async version of `run_stdio()`, suitable for use with
    /// hermes-llm async LLM calls and streaming responses.
    pub async fn run_stdio_async(&self) -> io::Result<()> {
        use tokio::io::{stdin, stdout, AsyncBufReadExt, AsyncWriteExt, BufReader};

        let stdin = BufReader::new(stdin());
        let mut stdout = stdout();
        let mut lines = stdin.lines();

        while let Some(line) = lines.next_line().await? {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }

            let request: JsonRpcRequest = match serde_json::from_str(&line) {
                Ok(req) => req,
                Err(e) => {
                    let error_resp = JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        result: None,
                        error: Some(JsonRpcError {
                            code: PARSE_ERROR,
                            message: format!("parse error: {e}"),
                            data: None,
                        }),
                        id: None,
                    };
                    let bytes = serde_json::to_vec(&error_resp).unwrap();
                    stdout.write_all(&bytes).await?;
                    stdout.write_all(b"\n").await?;
                    stdout.flush().await?;
                    continue;
                }
            };

            if let Some(ref version) = request.jsonrpc {
                if version != "2.0" {
                    let error_resp = JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        result: None,
                        error: Some(JsonRpcError {
                            code: INVALID_REQUEST,
                            message: format!("invalid jsonrpc version: {version}"),
                            data: None,
                        }),
                        id: request.id.clone(),
                    };
                    let bytes = serde_json::to_vec(&error_resp).unwrap();
                    stdout.write_all(&bytes).await?;
                    stdout.write_all(b"\n").await?;
                    stdout.flush().await?;
                    continue;
                }
            }

            if let Some(response) = self.dispatch(&request) {
                let bytes = serde_json::to_vec(&response).unwrap();
                stdout.write_all(&bytes).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
        }

        Ok(())
    }

    /// Access the session manager for external use.
    pub fn session_manager(&self) -> &Arc<SessionManager> {
        &self.session_manager
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(method: &str, params: Option<Value>, id: u64) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: Some("2.0".into()),
            method: method.into(),
            params,
            id: Some(Value::Number(id.into())),
        }
    }

    #[test]
    fn test_initialize() {
        let server = AcpServer::new();
        let req = make_request("initialize", Some(serde_json::json!({
            "protocol_version": 1,
            "client_info": { "name": "test-ide", "version": "1.0.0" }
        })), 1);

        let resp = server.dispatch(&req).expect("should respond");
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let init: InitializeResponse = serde_json::from_value(result).unwrap();
        assert_eq!(init.protocol_version, ACP_PROTOCOL_VERSION);
        assert_eq!(init.agent_info.name, "hermes-agent");
        assert!(init.agent_capabilities.is_some());
    }

    #[test]
    fn test_new_session() {
        let server = AcpServer::new();
        let req = make_request("session/new", Some(serde_json::json!({
            "cwd": "/tmp/test"
        })), 2);

        let resp = server.dispatch(&req).expect("should respond");
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let session_resp: NewSessionResponse = serde_json::from_value(result).unwrap();
        assert!(!session_resp.session_id.is_empty());
    }

    #[test]
    fn test_list_sessions() {
        let server = AcpServer::new();
        // Create two sessions first.
        server.session_manager.create_session("/tmp/a".into());
        server.session_manager.create_session("/tmp/b".into());

        let req = make_request("session/list", None, 3);
        let resp = server.dispatch(&req).expect("should respond");
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let list: ListSessionsResponse = serde_json::from_value(result).unwrap();
        assert_eq!(list.sessions.len(), 2);
    }

    #[test]
    fn test_prompt_slash_command_help() {
        let server = AcpServer::new();
        let session = server.session_manager.create_session("/tmp".into());
        let session_id = session.lock().session_id.clone();

        let req = make_request("prompt", Some(serde_json::json!({
            "session_id": session_id,
            "prompt": [{ "type": "text", "text": "/help" }]
        })), 4);

        let resp = server.dispatch(&req).expect("should respond");
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let prompt_resp: PromptResponse = serde_json::from_value(result).unwrap();
        assert_eq!(prompt_resp.stop_reason, Some("end_turn".into()));
    }

    #[test]
    fn test_cancel_session() {
        let server = AcpServer::new();
        let session = server.session_manager.create_session("/tmp".into());
        let session_id = session.lock().session_id.clone();

        let req = make_request("session/cancel", Some(serde_json::json!({
            "session_id": session_id
        })), 5);

        let resp = server.dispatch(&req).expect("should respond");
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_fork_session() {
        let server = AcpServer::new();
        let session = server.session_manager.create_session("/tmp".into());
        let session_id = session.lock().session_id.clone();

        let req = make_request("session/fork", Some(serde_json::json!({
            "session_id": session_id,
            "cwd": "/tmp/fork"
        })), 6);

        let resp = server.dispatch(&req).expect("should respond");
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let fork: ForkSessionResponse = serde_json::from_value(result).unwrap();
        assert!(!fork.session_id.is_empty());
        assert_ne!(fork.session_id, session_id);
    }

    #[test]
    fn test_method_not_found() {
        let server = AcpServer::new();
        let req = make_request("unknown/method", None, 7);
        let resp = server.dispatch(&req).expect("should respond");
        assert!(resp.error.is_some());
        let err = resp.error.unwrap();
        assert_eq!(err.code, METHOD_NOT_FOUND);
    }

    #[test]
    fn test_parse_error() {
        let _server = AcpServer::new();
        let bad_json = "not json at all";
        let request: Result<JsonRpcRequest, _> = serde_json::from_str(bad_json);
        assert!(request.is_err());
    }

    #[test]
    fn test_tools_list_empty() {
        let _server = AcpServer::new();
        let req = make_request("tools/list", None, 8);
        let resp = _server.dispatch(&req).expect("should respond");
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let tools: ToolsListResponse = serde_json::from_value(result).unwrap();
        assert!(tools.tools.is_empty());
    }

    #[test]
    fn test_session_notifications() {
        let notif = AcpServer::session_update_text("sess-1", "Hello from agent");
        assert_eq!(notif.method, "session/update");
        let params = notif.params.unwrap();
        assert_eq!(params["session_id"], "sess-1");
        assert_eq!(params["update"]["sessionUpdate"], "agent_message");
    }

    #[test]
    fn test_prompt_content_block_text_extraction() {
        let block = PromptContentBlock::Text(TextContentBlock {
            content_type: "text".into(),
            text: "user message".into(),
        });
        assert_eq!(block.as_text(), "user message");
    }

    #[test]
    fn test_set_session_model() {
        let server = AcpServer::new();
        let session = server.session_manager.create_session("/tmp".into());
        let session_id = session.lock().session_id.clone();

        let req = make_request("setSessionModel", Some(serde_json::json!({
            "session_id": session_id,
            "model_id": "claude-sonnet-4-6-20250514"
        })), 9);

        let resp = server.dispatch(&req).expect("should respond");
        assert!(resp.error.is_none());

        // Verify model was set.
        let guard = session.lock();
        assert_eq!(guard.model, Some("claude-sonnet-4-6-20250514".into()));
    }
}
