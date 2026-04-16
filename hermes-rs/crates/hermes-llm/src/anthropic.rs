//! Anthropic Messages API adapter.
//!
//! Mirrors Python `agent/anthropic_adapter.py`: Auth routing, extended thinking,
//! model output limits, beta headers, message/tool format conversion.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};

// ── Model output limits ────────────────────────────────────────────────────

/// Max output token limits per Anthropic model.
/// Source: Anthropic docs + Cline model catalog.
static ANTHROPIC_OUTPUT_LIMITS: &[(&str, usize)] = &[
    // Claude 4.6
    ("claude-opus-4-6", 128_000),
    ("claude-sonnet-4-6", 64_000),
    // Claude 4.5
    ("claude-opus-4-5", 64_000),
    ("claude-sonnet-4-5", 64_000),
    ("claude-haiku-4-5", 64_000),
    // Claude 4
    ("claude-opus-4", 32_000),
    ("claude-sonnet-4", 64_000),
    // Claude 3.7
    ("claude-3-7-sonnet", 128_000),
    // Claude 3.5
    ("claude-3-5-sonnet", 8_192),
    ("claude-3-5-haiku", 8_192),
    // Claude 3
    ("claude-3-opus", 4_096),
    ("claude-3-sonnet", 4_096),
    ("claude-3-haiku", 4_096),
    // Third-party Anthropic-compatible providers
    ("minimax", 131_072),
];

const ANTHROPIC_DEFAULT_OUTPUT_LIMIT: usize = 128_000;

/// Look up the max output token limit for an Anthropic model.
///
/// Uses substring matching so date-stamped model IDs
/// (claude-sonnet-4-5-20250929) and variant suffixes (:1m, :fast)
/// resolve correctly. Longest-prefix match wins.
pub fn get_anthropic_max_output(model: &str) -> usize {
    let m = model.to_lowercase().replace('.', "-");
    let mut best_key = "";
    let mut best_val = ANTHROPIC_DEFAULT_OUTPUT_LIMIT;
    for (key, val) in ANTHROPIC_OUTPUT_LIMITS {
        if m.contains(key) && key.len() > best_key.len() {
            best_key = key;
            best_val = *val;
        }
    }
    best_val
}

// ── Model name normalization ───────────────────────────────────────────────

/// Normalize a model name for the Anthropic API.
///
/// - Strips 'anthropic/' prefix (OpenRouter format, case-insensitive)
/// - Converts dots to hyphens in version numbers (OpenRouter uses dots,
///   Anthropic uses hyphens: claude-opus-4.6 → claude-opus-4-6)
pub fn normalize_model_name(model: &str) -> String {
    let mut result = model.to_string();
    let lower = result.to_lowercase();
    if lower.starts_with("anthropic/") {
        result = result["anthropic/".len()..].to_string();
    }
    result.replace('.', "-").to_lowercase()
}

// ── Thinking support ───────────────────────────────────────────────────────

/// Thinking budget levels mapped to token counts.
static THINKING_BUDGET: &[(&str, u32)] = &[
    ("xhigh", 32_000),
    ("high", 16_000),
    ("medium", 8_000),
    ("low", 4_000),
];

/// Adaptive effort levels for Claude 4.6.
static ADAPTIVE_EFFORT_MAP: &[(&str, &str)] = &[
    ("xhigh", "max"),
    ("high", "high"),
    ("medium", "medium"),
    ("low", "low"),
    ("minimal", "low"),
];

/// Check if a model supports adaptive thinking (Claude 4.6+).
pub fn supports_adaptive_thinking(model: &str) -> bool {
    model.contains("4-6") || model.contains("4.6")
}

/// Get the thinking budget for an effort level.
pub fn thinking_budget_for_level(level: &str) -> u32 {
    THINKING_BUDGET
        .iter()
        .find(|(l, _)| *l == level.to_lowercase())
        .map(|(_, budget)| *budget)
        .unwrap_or(8_000) // default: medium
}

/// Map an effort level to the adaptive effort string.
pub fn effort_to_adaptive(effort: &str) -> &str {
    ADAPTIVE_EFFORT_MAP
        .iter()
        .find(|(l, _)| *l == effort.to_lowercase())
        .map(|(_, v)| *v)
        .unwrap_or("medium")
}

// ── Beta headers ───────────────────────────────────────────────────────────

/// Common beta headers sent with all requests.
const COMMON_BETAS: &[&str] = &[
    "interleaved-thinking-2025-05-14",
    "fine-grained-tool-streaming-2025-05-14",
];

/// Fast mode beta — enables speed: "fast" for ~2.5x output throughput on Opus 4.6.
const FAST_MODE_BETA: &str = "fast-mode-2026-02-01";

/// OAuth-only beta headers.
const OAUTH_ONLY_BETAS: &[&str] = &[
    "claude-code-20250219",
    "oauth-2025-04-20",
];

/// MiniMax's Anthropic-compatible endpoints reject tool-use requests when
/// fine-grained-tool-streaming beta is present.
const TOOL_STREAMING_BETA: &str = "fine-grained-tool-streaming-2025-05-14";

/// Get common betas safe for the configured endpoint.
pub fn common_betas_for_base_url(base_url: Option<&str>) -> Vec<String> {
    if requires_bearer_auth(base_url) {
        COMMON_BETAS
            .iter()
            .filter(|&&b| b != TOOL_STREAMING_BETA)
            .map(|&s| s.to_string())
            .collect()
    } else {
        COMMON_BETAS.iter().map(|&s| s.to_string()).collect()
    }
}

/// Get all betas including OAuth-only ones.
pub fn all_betas(base_url: Option<&str>) -> Vec<String> {
    let mut betas = common_betas_for_base_url(base_url);
    betas.extend(OAUTH_ONLY_BETAS.iter().map(|&s| s.to_string()));
    betas
}

/// Build the anthropic-beta header value from a list of beta names.
pub fn beta_header_value(betas: &[String]) -> String {
    betas.join(",")
}

// ── Auth type detection ────────────────────────────────────────────────────

/// Check if the key is an Anthropic OAuth/setup token.
///
/// - `sk-ant-api*` → Regular API keys, never OAuth
/// - `sk-ant-*` (but not `sk-ant-api*`) → setup tokens, managed keys
/// - `eyJ*` → JWTs from Anthropic OAuth flow
pub fn is_oauth_token(key: &str) -> bool {
    if key.is_empty() {
        return false;
    }
    if key.starts_with("sk-ant-api") {
        return false;
    }
    if key.starts_with("sk-ant-") {
        return true;
    }
    if key.starts_with("eyJ") {
        return true;
    }
    false
}

/// Return true for non-Anthropic endpoints using the Anthropic Messages API.
pub fn is_third_party_endpoint(base_url: Option<&str>) -> bool {
    let Some(url) = base_url else { return false };
    let normalized = url.trim().trim_end_matches('/').to_lowercase();
    if normalized.is_empty() {
        return false;
    }
    !normalized.contains("anthropic.com")
}

/// Return true for Anthropic-compatible providers that require Bearer auth.
/// MiniMax endpoints use Bearer auth instead of x-api-key.
pub fn requires_bearer_auth(base_url: Option<&str>) -> bool {
    let Some(url) = base_url else { return false };
    let normalized = url.trim().trim_end_matches('/').to_lowercase();
    normalized.starts_with("https://api.minimax.io/anthropic")
        || normalized.starts_with("https://api.minimaxi.com/anthropic")
}

/// Determine the auth type for the given API key and base URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthType {
    /// Regular API key → x-api-key header
    ApiKey,
    /// OAuth/setup token → Bearer auth + OAuth betas + Claude Code identity
    OAuth,
    /// Third-party proxy → x-api-key (skip OAuth detection)
    ThirdParty,
    /// Bearer auth for providers like MiniMax
    Bearer,
}

pub fn detect_auth_type(api_key: &str, base_url: Option<&str>) -> AuthType {
    if requires_bearer_auth(base_url) {
        AuthType::Bearer
    } else if is_third_party_endpoint(base_url) {
        AuthType::ThirdParty
    } else if is_oauth_token(api_key) {
        AuthType::OAuth
    } else {
        AuthType::ApiKey
    }
}

// ── Claude Code credential loading ─────────────────────────────────────────

/// Read Anthropic OAuth credentials from ~/.claude/.credentials.json.
pub fn read_claude_code_credentials() -> Option<ClaudeCodeCredentials> {
    let home = dirs::home_dir()?;
    let cred_path = home.join(".claude").join(".credentials.json");
    if !cred_path.exists() {
        return None;
    }
    let data = std::fs::read_to_string(&cred_path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&data).ok()?;
    let oauth_data = parsed.get("claudeAiOauth")?;
    let obj = oauth_data.as_object()?;
    let access_token = obj.get("accessToken")?.as_str()?;
    if access_token.is_empty() {
        return None;
    }
    Some(ClaudeCodeCredentials {
        access_token: access_token.to_string(),
        refresh_token: obj.get("refreshToken").and_then(|v| v.as_str()).map(String::from),
        expires_at: obj.get("expiresAt").and_then(|v| v.as_u64()),
    })
}

/// Read Claude's native managed key from ~/.claude.json (diagnostics only).
pub fn read_claude_managed_key() -> Option<String> {
    let home = dirs::home_dir()?;
    let claude_json = home.join(".claude.json");
    if !claude_json.exists() {
        return None;
    }
    let data = std::fs::read_to_string(&claude_json).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&data).ok()?;
    let primary_key = parsed.get("primaryApiKey")?.as_str()?;
    let trimmed = primary_key.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

/// Claude Code OAuth credentials.
#[derive(Debug, Clone)]
pub struct ClaudeCodeCredentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>, // milliseconds since epoch
}

impl ClaudeCodeCredentials {
    /// Check if the access token is still valid (with 60s buffer).
    pub fn is_valid(&self) -> bool {
        match self.expires_at {
            Some(expires_ms) => {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                now_ms < expires_ms.saturating_sub(60_000)
            }
            None => true, // No expiry — valid if token present
        }
    }
}

/// Resolve an Anthropic token from all available sources.
///
/// Priority:
///   1. ANTHROPIC_TOKEN env var
///   2. CLAUDE_CODE_OAUTH_TOKEN env var
///   3. Claude Code credentials (~/.claude/.credentials.json)
///   4. ANTHROPIC_API_KEY env var (regular API key)
///
/// Returns (token, is_oauth).
pub fn resolve_anthropic_token() -> Option<(String, bool)> {
    // 1. ANTHROPIC_TOKEN
    if let Ok(token) = std::env::var("ANTHROPIC_TOKEN") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            let is_oauth = is_oauth_token(&token);
            return Some((token, is_oauth));
        }
    }

    // 2. CLAUDE_CODE_OAUTH_TOKEN
    if let Ok(token) = std::env::var("CLAUDE_CODE_OAUTH_TOKEN") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            let is_oauth = is_oauth_token(&token);
            return Some((token, is_oauth));
        }
    }

    // 3. Claude Code credentials
    if let Some(creds) = read_claude_code_credentials() {
        return Some((creds.access_token, true));
    }

    // 4. ANTHROPIC_API_KEY
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        let key = key.trim().to_string();
        if !key.is_empty() {
            let is_oauth = is_oauth_token(&key);
            return Some((key, is_oauth));
        }
    }

    None
}

// ── Tool conversion ────────────────────────────────────────────────────────

/// Sanitize a tool call ID for the Anthropic API.
/// Anthropic requires IDs matching [a-zA-Z0-9_-].
pub fn sanitize_tool_id(tool_id: &str) -> String {
    static INVALID_CHARS: Lazy<Regex> = Lazy::new(|| Regex::new(r"[^a-zA-Z0-9_-]").unwrap());
    if tool_id.is_empty() {
        return "tool_0".to_string();
    }
    let sanitized = INVALID_CHARS.replace_all(tool_id, "_").to_string();
    if sanitized.is_empty() {
        "tool_0".to_string()
    } else {
        sanitized
    }
}

/// Convert OpenAI tool definitions to Anthropic format.
pub fn convert_tools_to_anthropic(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            let fn_def = t.get("function").unwrap_or(t);
            json!({
                "name": fn_def.get("name").and_then(Value::as_str).unwrap_or(""),
                "description": fn_def.get("description").and_then(Value::as_str).unwrap_or(""),
                "input_schema": fn_def.get("parameters").cloned().unwrap_or_else(|| json!({
                    "type": "object", "properties": {}
                })),
            })
        })
        .collect()
}

// ── Image content handling ─────────────────────────────────────────────────

/// Convert an OpenAI-style image URL/data URL to an Anthropic image source.
fn image_source_from_openai_url(url: &str) -> Value {
    let url = url.trim();
    if url.starts_with("data:") {
        let media_type = if let Some(comma_pos) = url.find(',') {
            let header = &url[..comma_pos];
            let mime_part = header
                .strip_prefix("data:")
                .and_then(|s| s.split(';').next())
                .unwrap_or("image/jpeg");
            if mime_part.starts_with("image/") {
                mime_part.to_string()
            } else {
                "image/jpeg".to_string()
            }
        } else {
            "image/jpeg".to_string()
        };
        let data = url.find(',').map(|i| &url[i + 1..]).unwrap_or("");
        json!({
            "type": "base64",
            "media_type": media_type,
            "data": data,
        })
    } else {
        json!({
            "type": "url",
            "url": url,
        })
    }
}

/// Convert a single OpenAI-style content part to Anthropic format.
fn convert_content_part(part: &Value) -> Option<Value> {
    if let Some(text) = part.as_str() {
        return Some(json!({"type": "text", "text": text}));
    }
    if !part.is_object() {
        return Some(json!({"type": "text", "text": part.to_string()}));
    }
    let obj = part.as_object().unwrap();
    match obj.get("type").and_then(Value::as_str) {
        Some("input_text") => {
            Some(json!({"type": "text", "text": obj.get("text").and_then(Value::as_str).unwrap_or("")}))
        }
        Some("image_url") | Some("input_image") => {
            let image_value = obj.get("image_url").unwrap_or(part);
            let url = if let Some(img_obj) = image_value.as_object() {
                img_obj.get("url").and_then(Value::as_str).unwrap_or("").to_string()
            } else {
                image_value.as_str().unwrap_or("").to_string()
            };
            Some(json!({
                "type": "image",
                "source": image_source_from_openai_url(&url),
            }))
        }
        _ => Some(part.clone()),
    }
}

/// Convert OpenAI-style multimodal content array to Anthropic blocks.
pub fn convert_content_to_anthropic(content: &Value) -> Value {
    if let Some(arr) = content.as_array() {
        let converted: Vec<Value> = arr
            .iter()
            .filter_map(convert_content_part)
            .collect();
        Value::Array(converted)
    } else {
        content.clone()
    }
}

// ── Message conversion ─────────────────────────────────────────────────────

/// Extract preserved thinking blocks from an assistant message.
fn extract_thinking_blocks(message: &Value) -> Vec<Value> {
    let mut blocks = Vec::new();
    if let Some(raw_details) = message.get("reasoning_details").and_then(|v| v.as_array()) {
        for detail in raw_details {
            if let Some(obj) = detail.as_object() {
                let block_type = obj
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_lowercase();
                if block_type == "thinking" || block_type == "redacted_thinking" {
                    blocks.push(detail.clone());
                }
            }
        }
    }
    blocks
}

/// Convert OpenAI-format messages to Anthropic format.
///
/// Returns (system_prompt, anthropic_messages).
/// System messages are extracted since Anthropic takes them as a separate param.
pub fn convert_messages(messages: &[Value], strip_signatures: bool) -> (Option<Value>, Vec<Value>) {
    let mut system: Option<Value> = None;
    let mut result = Vec::new();

    for m in messages {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("user");
        let content = m.get("content").cloned().unwrap_or(Value::Null);

        match role {
            "system" => {
                if let Some(arr) = content.as_array() {
                    // Check for cache_control markers
                    let has_cache = arr.iter().any(|p| {
                        p.get("cache_control").is_some()
                    });
                    if has_cache {
                        system = Some(Value::Array(
                            arr.iter().filter(|p| p.is_object()).cloned().collect(),
                        ));
                    } else {
                        let texts: Vec<String> = arr
                            .iter()
                            .filter_map(|p| {
                                p.get("type")
                                    .and_then(|t| t.as_str())
                                    .filter(|&t| t == "text")
                                    .and_then(|_| p.get("text").and_then(Value::as_str))
                                    .map(String::from)
                            })
                            .collect();
                        system = Some(Value::String(texts.join("\n")));
                    }
                } else {
                    system = Some(content);
                }
            }
            "assistant" => {
                let mut blocks = extract_thinking_blocks(m);

                if strip_signatures {
                    blocks = blocks.into_iter().map(|mut b| {
                        if let Some(obj) = b.as_object_mut() {
                            obj.remove("signature");
                        }
                        b
                    }).collect();
                }

                // Add text content
                if let Some(arr) = content.as_array() {
                    let converted = convert_content_to_anthropic(&Value::Array(arr.clone()));
                    if let Some(arr) = converted.as_array() {
                        blocks.extend(arr.clone());
                    }
                } else if let Some(text) = content.as_str() {
                    if !text.is_empty() {
                        blocks.push(json!({"type": "text", "text": text}));
                    }
                }

                // Add tool calls
                if let Some(tool_calls) = m.get("tool_calls").and_then(Value::as_array) {
                    for tc in tool_calls {
                        if let Some(fn_def) = tc.get("function") {
                            let args = fn_def.get("arguments")
                                .and_then(Value::as_str)
                                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                                .unwrap_or_else(|| json!({}));
                            blocks.push(json!({
                                "type": "tool_use",
                                "id": sanitize_tool_id(tc.get("id").and_then(Value::as_str).unwrap_or("")),
                                "name": fn_def.get("name").and_then(Value::as_str).unwrap_or(""),
                                "input": args,
                            }));
                        }
                    }
                }

                // Anthropic rejects empty assistant content
                if blocks.is_empty() {
                    blocks.push(json!({"type": "text", "text": "(empty)"}));
                }
                result.push(json!({
                    "role": "assistant",
                    "content": Value::Array(blocks),
                }));
            }
            "tool" => {
                let content_str = content.as_str()
                    .map(String::from)
                    .unwrap_or_else(|| {
                        content.as_str()
                            .map(String::from)
                            .unwrap_or_else(|| "(no output)".to_string())
                    });
                let content_str = if content_str.is_empty() {
                    "(no output)".to_string()
                } else {
                    content_str
                };
                let mut tool_result = json!({
                    "type": "tool_result",
                    "tool_use_id": sanitize_tool_id(
                        m.get("tool_call_id").and_then(Value::as_str).unwrap_or("")
                    ),
                    "content": content_str,
                });
                if let Some(cache_ctrl) = m.get("cache_control") {
                    if let Some(obj) = tool_result.as_object_mut() {
                        obj.insert("cache_control".to_string(), cache_ctrl.clone());
                    }
                }

                // Merge consecutive tool results into one user message
                if let Some(last) = result.last_mut() {
                    if last.get("role").and_then(Value::as_str) == Some("user") {
                        if let Some(arr) = last.get_mut("content").and_then(Value::as_array_mut) {
                            arr.push(tool_result);
                            continue;
                        }
                    }
                }
                result.push(json!({
                    "role": "user",
                    "content": Value::Array(vec![tool_result]),
                }));
            }
            _ => {
                // user or unknown — treat as user
                let converted = if content.is_array() {
                    convert_content_to_anthropic(&content)
                } else if let Some(text) = content.as_str() {
                    Value::Array(vec![json!({"type": "text", "text": text})])
                } else {
                    Value::Array(vec![json!({"type": "text", "text": content.to_string()})])
                };
                result.push(json!({
                    "role": "user",
                    "content": converted,
                }));
            }
        }
    }

    (system, result)
}

// ── Request builder ────────────────────────────────────────────────────────

/// Build the Anthropic API request body as JSON.
pub struct AnthropicRequestBuilder {
    pub model: String,
    pub messages: Vec<Value>,
    pub system_prompt: Option<Value>,
    pub max_tokens: usize,
    pub temperature: Option<f64>,
    pub tools: Option<Vec<Value>>,
    pub api_key: String,
    pub base_url: Option<String>,
    pub thinking_enabled: bool,
    pub thinking_effort: Option<String>, // "low", "medium", "high", "xhigh"
    pub fast_mode: bool,
}

impl AnthropicRequestBuilder {
    /// Build the request body, headers, and URL.
    pub fn build(&self) -> (String, HashMap<String, String>, String) {
        let model = normalize_model_name(&self.model);
        let base_url = self.base_url.as_deref();

        // Determine auth
        let auth_type = detect_auth_type(&self.api_key, base_url);
        let betas = match auth_type {
            AuthType::OAuth => {
                let mut all = common_betas_for_base_url(base_url);
                all.extend(OAUTH_ONLY_BETAS.iter().map(|&s| s.to_string()));
                all
            }
            _ => common_betas_for_base_url(base_url),
        };

        // Build body
        let mut body = json!({
            "model": model,
            "messages": self.messages,
            "max_tokens": self.max_tokens,
        });

        // System prompt (can be string or array of content blocks for cache_control)
        if let Some(ref sys) = self.system_prompt {
            body["system"] = sys.clone();
        }

        if let Some(t) = self.temperature {
            body["temperature"] = json!(t);
        }

        // Tool definitions
        if let Some(ref tools) = self.tools {
            if !tools.is_empty() {
                body["tools"] = Value::Array(convert_tools_to_anthropic(tools));
            }
        }

        // Extended thinking (Claude 4.5+)
        if self.thinking_enabled {
            let budget = if let Some(ref effort) = self.thinking_effort {
                thinking_budget_for_level(effort)
            } else {
                8_000 // default medium
            };
            body["thinking"] = json!({
                "type": "enabled",
                "budget_tokens": budget,
            });
            // Adaptive effort for 4.6 models
            if supports_adaptive_thinking(&model) {
                if let Some(ref effort) = self.thinking_effort {
                    body["effort"] = json!(effort_to_adaptive(effort));
                }
            }
        }

        // Fast mode (Claude 4.6+ Opus/Sonnet)
        if self.fast_mode && supports_adaptive_thinking(&model) {
            body["speed"] = json!("fast");
            // Add fast mode beta header
        }

        let body_str = serde_json::to_string(&body).unwrap_or_default();

        // Build headers
        let mut headers = HashMap::new();
        headers.insert("anthropic-version".to_string(), "2023-06-01".to_string());
        headers.insert("content-type".to_string(), "application/json".to_string());

        // Beta header
        let mut final_betas = betas;
        if self.fast_mode {
            final_betas.push(FAST_MODE_BETA.to_string());
        }
        if !final_betas.is_empty() {
            headers.insert("anthropic-beta".to_string(), beta_header_value(&final_betas));
        }

        // Auth header
        match auth_type {
            AuthType::ApiKey | AuthType::ThirdParty => {
                if !self.api_key.is_empty() {
                    headers.insert("x-api-key".to_string(), self.api_key.clone());
                }
            }
            AuthType::OAuth => {
                headers.insert("authorization".to_string(), format!("Bearer {}", self.api_key));
                // Claude Code identity headers (required for OAuth routing)
                let version = detect_claude_code_version();
                headers.insert(
                    "user-agent".to_string(),
                    format!("claude-cli/{} (external, cli)", version),
                );
                headers.insert("x-app".to_string(), "cli".to_string());
            }
            AuthType::Bearer => {
                headers.insert("authorization".to_string(), format!("Bearer {}", self.api_key));
            }
        }

        // Build URL
        let base = self.base_url
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("https://api.anthropic.com");
        let url = format!("{}/v1/messages", base.trim_end_matches('/'));

        (body_str, headers, url)
    }
}

// ── Claude Code version detection ──────────────────────────────────────────

static CLAUDE_CODE_VERSION_FALLBACK: &str = "2.1.74";
static CACHED_CLAUDE_VERSION: Lazy<String> = Lazy::new(detect_claude_code_version_impl);

fn detect_claude_code_version_impl() -> String {
    for cmd in &["claude", "claude-code"] {
        if let Ok(output) = std::process::Command::new(cmd)
            .arg("--version")
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let version = stdout.split_whitespace().next().unwrap_or("");
                if !version.is_empty() && version.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    return version.to_string();
                }
            }
        }
    }
    CLAUDE_CODE_VERSION_FALLBACK.to_string()
}

/// Get the detected Claude Code version.
pub fn detect_claude_code_version() -> String {
    CACHED_CLAUDE_VERSION.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_model_name() {
        assert_eq!(normalize_model_name("anthropic/claude-opus-4.6"), "claude-opus-4-6");
        assert_eq!(normalize_model_name("claude-sonnet-4.5"), "claude-sonnet-4-5");
        assert_eq!(normalize_model_name("Anthropic/Claude-3-Opus"), "claude-3-opus");
    }

    #[test]
    fn test_output_limits() {
        assert_eq!(get_anthropic_max_output("claude-opus-4-6"), 128_000);
        assert_eq!(get_anthropic_max_output("claude-sonnet-4-5-20250929"), 64_000);
        assert_eq!(get_anthropic_max_output("claude-3-opus"), 4_096);
        assert_eq!(get_anthropic_max_output("unknown-model"), ANTHROPIC_DEFAULT_OUTPUT_LIMIT);
    }

    #[test]
    fn test_oauth_token_detection() {
        assert!(!is_oauth_token("sk-ant-api03-xxx"));
        assert!(is_oauth_token("sk-ant-oat01-xxx"));
        assert!(is_oauth_token("sk-ant-something"));
        assert!(is_oauth_token("eyJhbGciOi..."));
        assert!(!is_oauth_token(""));
        assert!(!is_oauth_token("some-other-key"));
    }

    #[test]
    fn test_third_party_endpoint_detection() {
        assert!(!is_third_party_endpoint(None));
        assert!(!is_third_party_endpoint(Some("https://api.anthropic.com")));
        assert!(is_third_party_endpoint(Some("https://ai.azure.com/anthropic")));
        assert!(is_third_party_endpoint(Some("https://api.minimax.io/anthropic")));
    }

    #[test]
    fn test_bearer_auth_detection() {
        assert!(requires_bearer_auth(Some("https://api.minimax.io/anthropic")));
        assert!(requires_bearer_auth(Some("https://api.minimaxi.com/anthropic")));
        assert!(!requires_bearer_auth(None));
        assert!(!requires_bearer_auth(Some("https://api.anthropic.com")));
    }

    #[test]
    fn test_thinking_budget() {
        assert_eq!(thinking_budget_for_level("xhigh"), 32_000);
        assert_eq!(thinking_budget_for_level("high"), 16_000);
        assert_eq!(thinking_budget_for_level("medium"), 8_000);
        assert_eq!(thinking_budget_for_level("low"), 4_000);
        assert_eq!(thinking_budget_for_level("unknown"), 8_000);
    }

    #[test]
    fn test_adaptive_thinking() {
        assert!(supports_adaptive_thinking("claude-opus-4-6"));
        assert!(supports_adaptive_thinking("claude-sonnet-4.6"));
        assert!(!supports_adaptive_thinking("claude-sonnet-4-5"));
    }

    #[test]
    fn test_sanitize_tool_id() {
        assert_eq!(sanitize_tool_id("tool_123"), "tool_123");
        assert_eq!(sanitize_tool_id("tool@#$%"), "tool____");
        assert_eq!(sanitize_tool_id(""), "tool_0");
    }

    #[test]
    fn test_convert_tools() {
        let tools = vec![json!({
            "function": {
                "name": "read_file",
                "description": "Read a file",
                "parameters": {"type": "object", "properties": {"path": {"type": "string"}}}
            }
        })];
        let converted = convert_tools_to_anthropic(&tools);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0]["name"], "read_file");
        assert_eq!(converted[0]["input_schema"]["properties"]["path"]["type"], "string");
    }

    #[test]
    fn test_message_conversion_system_extraction() {
        let messages = vec![
            json!({"role": "system", "content": "You are a helpful assistant."}),
            json!({"role": "user", "content": "Hello"}),
        ];
        let (system, msgs) = convert_messages(&messages, false);
        assert_eq!(system.as_ref().and_then(Value::as_str), Some("You are a helpful assistant."));
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn test_message_conversion_assistant_tool_use() {
        let messages = vec![
            json!({"role": "assistant", "content": "", "tool_calls": [{
                "id": "call_123",
                "type": "function",
                "function": {
                    "name": "read_file",
                    "arguments": "{\"path\": \"test.txt\"}"
                }
            }]}),
        ];
        let (_, msgs) = convert_messages(&messages, false);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "assistant");
        let content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "tool_use");
        assert_eq!(content[0]["name"], "read_file");
        assert_eq!(content[0]["input"]["path"], "test.txt");
    }

    #[test]
    fn test_detect_auth_type_api_key() {
        let auth = detect_auth_type("sk-ant-api03-xxx", None);
        assert!(matches!(auth, AuthType::ApiKey));
    }

    #[test]
    fn test_detect_auth_type_oauth() {
        let auth = detect_auth_type("sk-ant-oat01-xxx", None);
        assert!(matches!(auth, AuthType::OAuth));
    }

    #[test]
    fn test_detect_auth_type_bearer() {
        let auth = detect_auth_type("any-key", Some("https://api.minimax.io/anthropic"));
        assert!(matches!(auth, AuthType::Bearer));
    }

    #[test]
    fn test_image_source_data_url() {
        let source = image_source_from_openai_url("data:image/png;base64,ABC123");
        assert_eq!(source["type"], "base64");
        assert_eq!(source["media_type"], "image/png");
        assert_eq!(source["data"], "ABC123");
    }

    #[test]
    fn test_image_source_regular_url() {
        let source = image_source_from_openai_url("https://example.com/image.jpg");
        assert_eq!(source["type"], "url");
        assert_eq!(source["url"], "https://example.com/image.jpg");
    }
}
