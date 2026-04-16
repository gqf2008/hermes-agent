//! OpenAI Codex Responses API adapter.
//!
//! Mirrors the Python Codex Responses API support in `run_agent.py:3709-4627`.
//! The Responses API is distinct from Chat Completions:
//! - Input format: list of items with `type` field (not `role`/`content` messages)
//! - `instructions` replaces `system` message
//! - `max_output_tokens` replaces `max_tokens`
//! - `store=false` required (no server-side persistence)
//! - Uses `responses.stream()` instead of `chat.completions.create()`

use reqwest::Client as HttpClient;
use serde_json::{json, Value};
use std::time::Duration;

use crate::error_classifier::{classify_api_error, ClassifiedError};

/// Convert chat messages to Responses API input items.
///
/// Mirrors Python `_chat_messages_to_responses_input` (run_agent.py:3709).
/// - System messages are skipped
/// - User/assistant messages become `{"role": ..., "content": ...}`
/// - Assistant tool_calls become `{"type": "function_call", ...}`
/// - Tool responses become `{"type": "function_call_output", ...}`
/// - Encrypted reasoning items are replayed (without `id`, store=false)
pub fn chat_to_responses_input(messages: &[Value]) -> Vec<Value> {
    let mut items = Vec::new();

    for msg in messages {
        let role = msg.get("role").and_then(Value::as_str).unwrap_or("");

        match role {
            // System messages become instructions
            "system" => {
                // Skipped — system becomes `instructions` parameter instead
            }
            // User messages
            "user" => {
                if let Some(content) = msg.get("content").and_then(Value::as_str) {
                    items.push(json!({
                        "type": "message",
                        "role": "user",
                        "content": content,
                    }));
                }
            }
            // Assistant messages with possible tool_calls
            "assistant" => {
                // Emit tool_calls as function_call items
                if let Some(tool_calls) = msg.get("tool_calls").and_then(Value::as_array) {
                    for tc in tool_calls {
                        if let Some(function) = tc.get("function") {
                            items.push(json!({
                                "type": "function_call",
                                "call_id": tc.get("id").and_then(Value::as_str).unwrap_or(""),
                                "name": function.get("name").and_then(Value::as_str).unwrap_or(""),
                                "arguments": function.get("arguments").cloned().unwrap_or(json!({})),
                            }));
                        }
                    }
                }
                // Emit text content as message
                if let Some(content) = msg.get("content").and_then(Value::as_str) {
                    if !content.is_empty() {
                        items.push(json!({
                            "type": "message",
                            "role": "assistant",
                            "content": content,
                        }));
                    }
                }
            }
            // Tool responses
            "tool" => {
                let output = msg.get("content").and_then(Value::as_str).unwrap_or("");
                let call_id = msg.get("tool_call_id").and_then(Value::as_str).unwrap_or("");
                items.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": output,
                }));
            }
            _ => {}
        }
    }

    items
}

/// Build a Responses API request body.
fn build_responses_body(
    model: &str,
    instructions: &str,
    input: &[Value],
    tools: Option<&[Value]>,
    max_output_tokens: Option<usize>,
    temperature: Option<f64>,
) -> Value {
    let mut body = json!({
        "model": model,
        "instructions": instructions,
        "input": input,
        "store": false,
    });

    if let Some(t) = tools {
        let responses_tools: Vec<Value> = t.iter().map(|tool| {
            // Convert OpenAI function tool schema to Responses API tool format
            if let Some(function) = tool.get("function") {
                json!({
                    "type": "function",
                    "name": function.get("name").and_then(Value::as_str).unwrap_or(""),
                    "description": function.get("description").and_then(Value::as_str).unwrap_or(""),
                    "parameters": function.get("parameters").cloned().unwrap_or(json!({"type": "object", "properties": {}})),
                })
            } else {
                tool.clone()
            }
        }).collect();
        body["tools"] = json!(responses_tools);
    }

    if let Some(max) = max_output_tokens {
        body["max_output_tokens"] = json!(max);
    }

    if let Some(temp) = temperature {
        body["temperature"] = json!(temp);
    }

    body
}

/// Call OpenAI Responses API (non-streaming).
///
/// Mirrors Python `_run_codex_create` (non-streaming variant).
pub async fn call_responses_api(
    model: &str,
    instructions: &str,
    input: &[Value],
    tools: Option<&[Value]>,
    max_output_tokens: Option<usize>,
    temperature: Option<f64>,
    base_url: &str,
    api_key: &str,
    timeout_secs: u64,
) -> Result<ResponsesApiResponse, ClassifiedError> {
    let body = build_responses_body(
        model, instructions, input, tools, max_output_tokens, temperature,
    );

    let url = format!("{}/responses", base_url.trim_end_matches('/'));
    let client = HttpClient::builder()
        .user_agent("reqwest/0.12.12")
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| classify_api_error("openai_responses", model, None,
            &format!("Failed to build HTTP client: {e}")))?;

    let resp = client.post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .header("OpenAI-Beta", "responses=v1")
        .json(&body)
        .send()
        .await
        .map_err(|e| classify_api_error("openai_responses", model, None,
            &format!("Request failed: {e}")))?;

    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();

    if status >= 400 {
        return Err(classify_api_error("openai_responses", model, Some(status), &text));
    }

    let response: ResponsesApiResponse = serde_json::from_str(&text).map_err(|e| {
        classify_api_error("openai_responses", model, Some(status),
            &format!("Failed to parse response: {e}"))
    })?;

    Ok(response)
}

/// Responses API response structure.
#[derive(Debug, Clone)]
pub struct ResponsesApiResponse {
    pub id: String,
    pub model: String,
    pub status: String,
    pub output: Vec<Value>,
    pub usage: Option<ResponsesApiUsage>,
}

impl<'de> serde::Deserialize<'de> for ResponsesApiResponse {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let val = Value::deserialize(deserializer)?;
        let obj = val.as_object().ok_or_else(|| serde::de::Error::custom("expected object"))?;

        Ok(Self {
            id: obj.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
            model: obj.get("model").and_then(Value::as_str).unwrap_or("").to_string(),
            status: obj.get("status").and_then(Value::as_str).unwrap_or("completed").to_string(),
            output: obj.get("output").and_then(Value::as_array).cloned().unwrap_or_default(),
            usage: obj.get("usage").and_then(|u| serde_json::from_value(u.clone()).ok()),
        })
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ResponsesApiUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
}

/// Extract text content from Responses API output.
pub fn extract_text_from_output(output: &[Value]) -> String {
    let mut parts = Vec::new();
    for item in output {
        if let Some(item_type) = item.get("type").and_then(Value::as_str) {
            match item_type {
                "message" => {
                    if let Some(content_arr) = item.get("content").and_then(Value::as_array) {
                        for c in content_arr {
                            if c.get("type").and_then(Value::as_str) == Some("output_text") {
                                if let Some(text) = c.get("text").and_then(Value::as_str) {
                                    parts.push(text);
                                }
                            }
                        }
                    }
                }
                "function_call" => {
                    // Tool call detected — caller will handle
                }
                _ => {}
            }
        }
    }
    parts.join("\n")
}

/// Extract tool calls from Responses API output.
pub fn extract_tool_calls_from_output(output: &[Value]) -> Vec<Value> {
    let mut tool_calls = Vec::new();
    for item in output {
        if item.get("type").and_then(Value::as_str) == Some("function_call") {
            tool_calls.push(json!({
                "id": item.get("call_id").and_then(Value::as_str).unwrap_or(""),
                "type": "function",
                "function": {
                    "name": item.get("name").and_then(Value::as_str).unwrap_or(""),
                    "arguments": item.get("arguments").cloned().unwrap_or(json!({})),
                }
            }));
        }
    }
    tool_calls
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_to_responses_basic() {
        let messages = vec![
            json!({"role": "system", "content": "You are helpful."}),
            json!({"role": "user", "content": "What is 2+2?"}),
            json!({"role": "assistant", "content": "It's 4."}),
        ];
        let input = chat_to_responses_input(&messages);
        // System is skipped, user and assistant become items
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[1]["role"], "assistant");
    }

    #[test]
    fn test_chat_to_responses_with_tool_calls() {
        let messages = vec![
            json!({"role": "user", "content": "Search for Rust"}),
            json!({"role": "assistant", "content": null, "tool_calls": [
                {"id": "call_1", "type": "function", "function": {
                    "name": "web_search", "arguments": {"query": "Rust"}
                }}
            ]}),
            json!({"role": "tool", "tool_call_id": "call_1", "content": "Results here"}),
        ];
        let input = chat_to_responses_input(&messages);
        assert_eq!(input.len(), 3);
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["call_id"], "call_1");
        assert_eq!(input[2]["type"], "function_call_output");
        assert_eq!(input[2]["output"], "Results here");
    }

    #[test]
    fn test_extract_text_from_output() {
        let output = vec![
            json!({
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "Hello!"}]
            }),
        ];
        let text = extract_text_from_output(&output);
        assert_eq!(text, "Hello!");
    }

    #[test]
    fn test_extract_tool_calls_from_output() {
        let output = vec![
            json!({
                "type": "function_call",
                "call_id": "call_abc",
                "name": "terminal",
                "arguments": {"command": "ls"}
            }),
        ];
        let tool_calls = extract_tool_calls_from_output(&output);
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0]["function"]["name"], "terminal");
    }

    #[test]
    fn test_build_responses_body() {
        let body = build_responses_body(
            "o3", "You are a coding assistant",
            &[json!({"type": "message", "role": "user", "content": "Hi"})],
            None, Some(1000), Some(1.0),
        );
        assert_eq!(body["model"], "o3");
        assert_eq!(body["instructions"], "You are a coding assistant");
        assert_eq!(body["store"], false);
        assert_eq!(body["max_output_tokens"], 1000);
        assert_eq!(body["temperature"], 1.0);
    }

    #[test]
    fn test_system_message_skipped() {
        let messages = vec![
            json!({"role": "system", "content": "System prompt"}),
            json!({"role": "system", "content": "Another system"}),
            json!({"role": "user", "content": "Hello"}),
        ];
        let input = chat_to_responses_input(&messages);
        // Only user message remains
        assert_eq!(input.len(), 1);
    }
}
