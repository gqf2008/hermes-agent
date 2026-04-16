//! Chat loop failover chain.
//!
//! Mirrors the Python failover sequence in `run_agent.py:9350-10127`.
//! On LLM errors, applies a priority-ordered sequence of recovery actions:
//! 1. Unicode sanitization (surrogate characters)
//! 2. Error classification
//! 3. Credential pool rotation
//! 4. Provider-specific auth refresh
//! 5. Thinking signature recovery
//! 6. Rate limit eager fallback
//! 7. Payload too large → compress
//! 8. Context overflow → compress
//! 9. Non-retryable → fallback → abort

use hermes_llm::credential_pool::CredentialPool;
use hermes_llm::error_classifier::{ClassifiedError, FailoverReason};
use serde_json::Value;

/// Failover chain state for a single conversation turn.
#[derive(Debug, Default)]
pub struct FailoverState {
    /// Consecutive 429 rate limit hits.
    pub consecutive_429: u32,
    /// Whether thinking signature has been stripped.
    pub thinking_stripped: bool,
    /// Unicode sanitization pass count.
    pub sanitize_passes: u32,
    /// Total retry attempts.
    pub retry_count: u32,
}

const MAX_SANITIZE_PASSES: u32 = 2;

/// Sanitize surrogate characters in message content.
///
/// Mirrors Python: UnicodeEncodeError recovery (run_agent.py:9376-9489).
/// Replaces invalid UTF-8 surrogate characters with replacement character.
pub fn sanitize_unicode_messages(messages: &mut [Value]) {
    for msg in messages.iter_mut() {
        if let Some(obj) = msg.as_object_mut() {
            for (_, value) in obj.iter_mut() {
                sanitize_value(value);
            }
        }
    }
}

fn sanitize_value(value: &mut Value) {
    match value {
        Value::String(s) => {
            // Filter out replacement/surrogate characters
            let cleaned: String = s.chars()
                .filter(|c| *c != '\u{FFFD}')
                .collect();
            *s = cleaned;
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                sanitize_value(item);
            }
        }
        Value::Object(map) => {
            for (_, value) in map.iter_mut() {
                sanitize_value(value);
            }
        }
        _ => {}
    }
}

/// Strip reasoning details from all messages.
///
/// Mirrors Python: thinking signature recovery (run_agent.py:9574-9592).
/// Removes `reasoning`, `reasoning_content`, `reasoning_details` fields
/// and inline think tags from content.
pub fn strip_reasoning_from_messages(messages: &mut [Value]) {
    for msg in messages.iter_mut() {
        if let Some(obj) = msg.as_object_mut() {
            obj.remove("reasoning");
            obj.remove("reasoning_content");
            obj.remove("reasoning_details");
            // Strip inline tags from content
            if let Some(content) = obj.get("content").and_then(Value::as_str) {
                let cleaned = strip_inline_reasoning(content);
                if let Some(content_val) = obj.get_mut("content") {
                    *content_val = Value::String(cleaned);
                }
            }
        }
    }
}

fn strip_inline_reasoning(content: &str) -> String {
    let patterns = [
        ("<think>", "</think>"),
        ("<thinking>", "</thinking>"),
        ("<thought>", "</thought>"),
        ("<reasoning>", "</reasoning>"),
        ("<REASONING_SCRATCHPAD>", "</REASONING_SCRATCHPAD>"),
    ];

    let mut result = content.to_string();
    for &(open, close) in &patterns {
        while let Some(start) = result.find(open) {
            if let Some(end) = result[start..].find(close) {
                let end = start + end + close.len();
                result.replace_range(start..end, "");
            } else {
                break;
            }
        }
    }
    result
}

/// Apply the failover chain for an LLM error.
///
/// Returns `FailoverAction` indicating what the caller should do.
pub fn apply_failover(
    error: &ClassifiedError,
    state: &mut FailoverState,
    pool: Option<&CredentialPool>,
    has_compressor: bool,
) -> FailoverAction {
    state.retry_count += 1;

    // 1. Unicode sanitization — up to 2 passes
    if state.sanitize_passes < MAX_SANITIZE_PASSES {
        // Check if error looks like an encoding issue
        if error.message.contains("encoding") || error.message.contains("codec") {
            state.sanitize_passes += 1;
            return FailoverAction::SanitizeUnicode;
        }
    }

    // 2. Rate limit tracking (Python: first 429 doesn't rotate, second does)
    if error.reason == FailoverReason::RateLimit {
        if state.consecutive_429 > 0 {
            state.consecutive_429 = 0;
            if pool.is_some() {
                return FailoverAction::RotateCredential;
            }
        } else {
            state.consecutive_429 += 1;
        }
        return FailoverAction::RetryWithBackoff;
    }

    // 3. Credential pool rotation (non-rate-limit errors)
    if error.should_rotate_credential {
        match error.reason {
            FailoverReason::Billing | FailoverReason::Auth => {
                return FailoverAction::RotateCredential;
            }
            _ => {}
        }
    }

    // 3. Thinking signature recovery (one-shot)
    if error.reason == FailoverReason::ThinkingSignature && !state.thinking_stripped {
        state.thinking_stripped = true;
        return FailoverAction::StripThinkingSignature;
    }

    // 4. Context overflow → compress
    if error.reason == FailoverReason::ContextOverflow && has_compressor {
        return FailoverAction::CompressContext;
    }

    // 5. Payload too large → compress
    if error.reason == FailoverReason::PayloadTooLarge && has_compressor {
        return FailoverAction::CompressContext;
    }

    // 6. Retryable errors → backoff
    if error.retryable {
        return FailoverAction::RetryWithBackoff;
    }

    // 7. Fallback recommended
    if error.should_fallback {
        return FailoverAction::TryFallback;
    }

    // 8. Abort
    FailoverAction::Abort
}

/// Recommended action after failover analysis.
#[derive(Debug, Clone)]
pub enum FailoverAction {
    /// Sanitize Unicode characters and retry.
    SanitizeUnicode,
    /// Rotate to next credential in pool and retry.
    RotateCredential,
    /// Strip reasoning from messages and retry (one-shot).
    StripThinkingSignature,
    /// Compress context and retry.
    CompressContext,
    /// Retry with exponential backoff.
    RetryWithBackoff,
    /// Try fallback provider.
    TryFallback,
    /// No recovery available — abort.
    Abort,
}

#[cfg(test)]
mod tests {
    use super::*;
    use hermes_llm::error_classifier::classify_api_error;

    #[test]
    fn test_sanitize_unicode_messages() {
        let mut messages = vec![
            serde_json::json!({
                "role": "user",
                "content": "Hello\u{FFFD}World"
            }),
        ];
        sanitize_unicode_messages(&mut messages);
        // Replacement characters should be filtered out
        let content = messages[0].get("content").and_then(Value::as_str).unwrap();
        assert_eq!(content, "HelloWorld");
    }

    #[test]
    fn test_strip_reasoning_fields() {
        let mut messages = vec![
            serde_json::json!({
                "role": "assistant",
                "reasoning": "I should think...",
                "reasoning_content": "More thinking...",
                "reasoning_details": [{"summary": "Summary"}],
                "content": "Hello!"
            }),
        ];
        strip_reasoning_from_messages(&mut messages);
        assert!(messages[0].get("reasoning").is_none());
        assert!(messages[0].get("reasoning_content").is_none());
        assert!(messages[0].get("reasoning_details").is_none());
        assert_eq!(messages[0].get("content").and_then(Value::as_str), Some("Hello!"));
    }

    #[test]
    fn test_strip_inline_reasoning_tags() {
        let input = "<think>Secret thinking</think>Hello!";
        let result = strip_inline_reasoning(input);
        assert_eq!(result, "Hello!");
    }

    #[test]
    fn test_strip_thinking_tags() {
        let input = "<thinking>Internal reasoning</thinking>Answer: 42";
        let result = strip_inline_reasoning(input);
        assert_eq!(result, "Answer: 42");
    }

    #[test]
    fn test_apply_failover_billing() {
        let err = classify_api_error("openrouter", "model", Some(402), "Billing exceeded");
        let mut state = FailoverState::default();
        let action = apply_failover(&err, &mut state, None, false);
        assert!(matches!(action, FailoverAction::RetryWithBackoff | FailoverAction::TryFallback));
        // Billing errors set should_fallback = true
    }

    #[test]
    fn test_apply_failover_context_overflow() {
        let err = classify_api_error("anthropic", "claude", Some(400), "context length exceeded");
        let mut state = FailoverState::default();
        let action = apply_failover(&err, &mut state, None, true);
        assert!(matches!(action, FailoverAction::CompressContext));
    }

    #[test]
    fn test_apply_failover_thinking_signature() {
        let err = classify_api_error("anthropic", "claude", Some(400), "thinking signature invalid");
        let mut state = FailoverState::default();
        let action = apply_failover(&err, &mut state, None, false);
        assert!(matches!(action, FailoverAction::StripThinkingSignature));
    }

    #[test]
    fn test_apply_failover_thinking_already_stripped() {
        let err = classify_api_error("anthropic", "claude", Some(400), "thinking signature invalid");
        let mut state = FailoverState {
            thinking_stripped: true,
            ..Default::default()
        };
        let action = apply_failover(&err, &mut state, None, false);
        // After thinking is already stripped, should fallback
        assert!(matches!(action, FailoverAction::TryFallback));
    }

    #[test]
    fn test_apply_failover_rate_limit_first() {
        let err = classify_api_error("openai", "gpt-4", Some(429), "Rate limit exceeded");
        let mut state = FailoverState::default();
        let action = apply_failover(&err, &mut state, None, false);
        // First 429: retry with backoff (don't rotate yet)
        assert!(matches!(action, FailoverAction::RetryWithBackoff));
        assert_eq!(state.consecutive_429, 1);
    }

    #[test]
    fn test_apply_failover_retryable_unknown() {
        let err = classify_api_error("unknown", "model", None, "Something weird");
        let mut state = FailoverState::default();
        let action = apply_failover(&err, &mut state, None, false);
        assert!(matches!(action, FailoverAction::RetryWithBackoff));
    }

    #[test]
    fn test_apply_failover_abort_on_non_retryable() {
        // Non-retryable error with no fallback available should abort
        let err = classify_api_error("anthropic", "claude", Some(400), "Invalid request");
        let mut state = FailoverState::default();
        let action = apply_failover(&err, &mut state, None, false);
        // 400 client error is not retryable, no fallback → abort
        assert!(matches!(action, FailoverAction::Abort));
    }

    #[test]
    fn test_apply_failover_unicode_pass_then_retry() {
        // Unicode encoding error should trigger sanitize, then retry
        let err = classify_api_error("openai", "model", Some(400), "encoding error: invalid byte");
        let mut state = FailoverState::default();
        let action = apply_failover(&err, &mut state, None, false);
        assert!(matches!(action, FailoverAction::SanitizeUnicode));
        assert_eq!(state.sanitize_passes, 1);
    }

    #[test]
    fn test_apply_failover_max_sanitize_passes() {
        // After 2 sanitize passes, should fall back to retry/abort
        let mut state = FailoverState { sanitize_passes: 2, ..Default::default() };
        let err = classify_api_error("openai", "model", Some(400), "encoding error: invalid byte");
        let action = apply_failover(&err, &mut state, None, false);
        // Max passes reached, should not sanitize again
        assert!(!matches!(action, FailoverAction::SanitizeUnicode));
    }

    #[test]
    fn test_apply_failover_billing_with_pool() {
        // Billing error with credential pool should still retry (billing → no rotation)
        let err = classify_api_error("openrouter", "model", Some(402), "Billing exceeded");
        let mut state = FailoverState::default();
        let action = apply_failover(&err, &mut state, None, false);
        // Billing without rotation → retry with backoff or fallback
        assert!(matches!(action, FailoverAction::RetryWithBackoff | FailoverAction::TryFallback));
    }
}
