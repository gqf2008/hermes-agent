//! Multi-format reasoning extraction.
//!
//! Mirrors the Python `_extract_reasoning` method in `run_agent.py:2182`.
//! Extracts reasoning/thinking content from assistant messages in 4 formats:
//! 1. `message.reasoning` — direct field (DeepSeek, Qwen)
//! 2. `message.reasoning_content` — alternative field (Moonshot AI, Novita)
//! 3. `message.reasoning_details` — array of detail objects (OpenRouter unified format)
//! 4. Inline patterns in content (fallback): <think>, <thinking>, <reasoning>, etc.

use regex::Regex;
use serde_json::Value;

/// Extract reasoning/thinking content from an assistant message.
pub fn extract_reasoning(message: &Value) -> String {
    let mut parts = Vec::new();

    // 1. message.reasoning — direct field
    if let Some(reasoning) = message.get("reasoning").and_then(Value::as_str) {
        if !reasoning.is_empty() {
            parts.push(reasoning.to_string());
        }
    }

    // 2. message.reasoning_content — alternative field
    if let Some(reasoning_content) = message.get("reasoning_content").and_then(Value::as_str) {
        if !reasoning_content.is_empty() {
            parts.push(reasoning_content.to_string());
        }
    }

    // 3. message.reasoning_details — array of detail objects
    if let Some(details) = message.get("reasoning_details").and_then(Value::as_array) {
        for detail in details {
            if let Some(obj) = detail.as_object() {
                // Try known keys in priority order
                for key in &["summary", "thinking", "content", "text"] {
                    if let Some(val) = obj.get(*key).and_then(Value::as_str) {
                        if !val.is_empty() {
                            parts.push(val.to_string());
                            break;
                        }
                    }
                }
            }
        }
    }

    // 4. Inline patterns in content (fallback — only if no structured reasoning found)
    if parts.is_empty() {
        if let Some(content) = message.get("content").and_then(Value::as_str) {
            parts.extend(extract_inline_reasoning(content));
        }
    }

    // Join parts with double newline
    parts.join("\n\n")
}

/// Extract inline reasoning patterns from content text.
static REASONING_PATTERNS: &[(&str, &str)] = &[
    ("<think>", "</think>"),
    ("<thinking>", "</thinking>"),
    ("<thought>", "</thought>"),
    ("<reasoning>", "</reasoning>"),
    ("<REASONING_SCRATCHPAD>", "</REASONING_SCRATCHPAD>"),
];

fn extract_inline_reasoning(content: &str) -> Vec<String> {
    let mut results = Vec::new();

    for &(open, close) in REASONING_PATTERNS {
        let pattern = format!("(?s){}(.*?){}", regex::escape(open), regex::escape(close));
        if let Ok(re) = Regex::new(&pattern) {
            for cap in re.captures_iter(content) {
                if let Some(m) = cap.get(1) {
                    let text = m.as_str().trim();
                    if !text.is_empty() {
                        results.push(text.to_string());
                    }
                }
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_direct_reasoning_field() {
        let msg = serde_json::json!({
            "role": "assistant",
            "reasoning": "Let me think about this carefully...",
            "content": "The answer is 42."
        });
        let result = extract_reasoning(&msg);
        assert!(result.contains("Let me think about this carefully"));
    }

    #[test]
    fn test_reasoning_content_field() {
        let msg = serde_json::json!({
            "role": "assistant",
            "reasoning_content": "Internal thinking here",
            "content": "Hello!"
        });
        let result = extract_reasoning(&msg);
        assert!(result.contains("Internal thinking here"));
    }

    #[test]
    fn test_reasoning_details_array() {
        let msg = serde_json::json!({
            "role": "assistant",
            "reasoning_details": [
                {"type": "thought", "summary": "Step 1: analyze input"},
                {"type": "analysis", "thinking": "Step 2: reason through it"}
            ],
            "content": "Done."
        });
        let result = extract_reasoning(&msg);
        assert!(result.contains("Step 1: analyze input"));
        assert!(result.contains("Step 2: reason through it"));
    }

    #[test]
    fn test_inline_think_tags() {
        let msg = serde_json::json!({
            "role": "assistant",
            "content": "<think>I should calculate first</think>The result is 42."
        });
        let result = extract_reasoning(&msg);
        assert!(result.contains("I should calculate first"));
    }

    #[test]
    fn test_inline_thinking_tags() {
        let msg = serde_json::json!({
            "role": "assistant",
            "content": "<thinking>Let me work through this</thinking>\n\nHere's my answer."
        });
        let result = extract_reasoning(&msg);
        assert!(result.contains("Let me work through this"));
    }

    #[test]
    fn test_no_reasoning() {
        let msg = serde_json::json!({
            "role": "assistant",
            "content": "Hello!"
        });
        let result = extract_reasoning(&msg);
        assert_eq!(result, "");
    }

    #[test]
    fn test_structured_takes_precedence_over_inline() {
        // When structured reasoning exists, inline patterns in content
        // should NOT be extracted (fallback only)
        let msg = serde_json::json!({
            "role": "assistant",
            "reasoning": "Structured reasoning",
            "content": "<think>This should not be extracted</think>Hello!"
        });
        let result = extract_reasoning(&msg);
        assert!(result.contains("Structured reasoning"));
        assert!(!result.contains("This should not be extracted"));
    }

    #[test]
    fn test_reasoning_details_multiple_keys() {
        let msg = serde_json::json!({
            "role": "assistant",
            "reasoning_details": [
                {"type": "text", "text": "Text-based reasoning detail"},
                {"type": "thought", "content": "Content-based reasoning"}
            ],
            "content": "Answer"
        });
        let result = extract_reasoning(&msg);
        assert!(result.contains("Text-based reasoning detail"));
        assert!(result.contains("Content-based reasoning"));
    }
}
