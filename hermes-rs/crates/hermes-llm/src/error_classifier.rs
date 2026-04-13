//! Error classification for API errors.
//!
//! Classifies HTTP and transport errors into actionable categories with
//! hints for retry, fallback, compression, or credential rotation.
//! Mirrors the Python `error_classifier.py` (classify_api_error).

use std::fmt;
use serde::Serialize;

/// Reasons for API failure, mapped to specific actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum FailoverReason {
    Auth,
    AuthPermanent,
    Billing,
    RateLimit,
    Overloaded,
    ServerError,
    Timeout,
    ContextOverflow,
    PayloadTooLarge,
    ModelNotFound,
    FormatError,
    ThinkingSignature,
    LongContextTier,
    Unknown,
}

impl std::fmt::Display for FailoverReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FailoverReason::Auth => write!(f, "auth"),
            FailoverReason::AuthPermanent => write!(f, "auth_permanent"),
            FailoverReason::Billing => write!(f, "billing"),
            FailoverReason::RateLimit => write!(f, "rate_limit"),
            FailoverReason::Overloaded => write!(f, "overloaded"),
            FailoverReason::ServerError => write!(f, "server_error"),
            FailoverReason::Timeout => write!(f, "timeout"),
            FailoverReason::ContextOverflow => write!(f, "context_overflow"),
            FailoverReason::PayloadTooLarge => write!(f, "payload_too_large"),
            FailoverReason::ModelNotFound => write!(f, "model_not_found"),
            FailoverReason::FormatError => write!(f, "format_error"),
            FailoverReason::ThinkingSignature => write!(f, "thinking_signature"),
            FailoverReason::LongContextTier => write!(f, "long_context_tier"),
            FailoverReason::Unknown => write!(f, "unknown"),
        }
    }
}

/// Classified API error with actionable hints.
#[derive(Debug, Clone, Serialize)]
pub struct ClassifiedError {
    pub reason: FailoverReason,
    pub status_code: Option<u16>,
    pub provider: String,
    pub model: String,
    pub message: String,
    pub retryable: bool,
    pub should_compress: bool,
    pub should_rotate_credential: bool,
    pub should_fallback: bool,
}

impl fmt::Display for ClassifiedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {} ({}/{})", self.reason, self.message, self.provider, self.model)
    }
}

/// Classify an API error into an actionable category.
///
/// 5-step classification pipeline:
/// 1. Provider-specific patterns (thinking signature, long-context tier)
/// 2. HTTP status code classification
/// 3. Message pattern matching (billing, rate_limit, context_overflow, auth)
/// 4. Transport/timeout heuristics
/// 5. Fallback: Unknown (retryable with backoff)
pub fn classify_api_error(
    provider: &str,
    model: &str,
    status_code: Option<u16>,
    message: &str,
) -> ClassifiedError {
    let ml = message.to_lowercase();

    // Step 1: Provider-specific patterns
    if provider == "anthropic" && ml.contains("thinking") && ml.contains("signature") {
        return classified(FailoverReason::ThinkingSignature, status_code, provider, model, message);
    }
    if ml.contains("long context") && ml.contains("tier") {
        return classified(FailoverReason::LongContextTier, status_code, provider, model, message);
    }

    // Step 2: HTTP status code
    if let Some(code) = status_code {
        match code {
            400 => {
                if is_context_overflow_message(&ml) {
                    return classified(FailoverReason::ContextOverflow, status_code, provider, model, message);
                }
                if ml.contains("too large") || ml.contains("max") {
                    return classified(FailoverReason::PayloadTooLarge, status_code, provider, model, message);
                }
                return classified(FailoverReason::FormatError, status_code, provider, model, message);
            }
            401 => return classified(FailoverReason::Auth, status_code, provider, model, message),
            402 => {
                if classify_402(&ml) {
                    return classified(FailoverReason::RateLimit, status_code, provider, model, message);
                }
                return classified(FailoverReason::Billing, status_code, provider, model, message);
            }
            403 => return classified(FailoverReason::AuthPermanent, status_code, provider, model, message),
            404 => return classified(FailoverReason::ModelNotFound, status_code, provider, model, message),
            429 => return classified(FailoverReason::RateLimit, status_code, provider, model, message),
            500..=599 => {
                if code == 503 || ml.contains("overload") {
                    return classified(FailoverReason::Overloaded, status_code, provider, model, message);
                }
                return classified(FailoverReason::ServerError, status_code, provider, model, message);
            }
            _ => {}
        }
    }

    // Step 4: Message pattern matching
    if is_auth_message(&ml) {
        return classified(FailoverReason::Auth, status_code, provider, model, message);
    }
    if is_billing_message(&ml) {
        return classified(FailoverReason::Billing, status_code, provider, model, message);
    }
    if is_rate_limit_message(&ml) {
        return classified(FailoverReason::RateLimit, status_code, provider, model, message);
    }
    if is_context_overflow_message(&ml) {
        return classified(FailoverReason::ContextOverflow, status_code, provider, model, message);
    }

    // Step 6: Transport/timeout
    if ml.contains("timeout") || ml.contains("timed out") {
        return classified(FailoverReason::Timeout, status_code, provider, model, message);
    }
    if ml.contains("disconnect") || ml.contains("connection") {
        return classified(FailoverReason::ServerError, status_code, provider, model, message);
    }

    // Step 7: Fallback
    classified(FailoverReason::Unknown, status_code, provider, model, message)
}

fn classified(
    reason: FailoverReason,
    status_code: Option<u16>,
    provider: &str,
    model: &str,
    message: &str,
) -> ClassifiedError {
    let (retryable, should_compress, should_rotate_credential, should_fallback) = action_hints(&reason);
    ClassifiedError {
        reason,
        status_code,
        provider: provider.to_string(),
        model: model.to_string(),
        message: message.to_string(),
        retryable,
        should_compress,
        should_rotate_credential,
        should_fallback,
    }
}

fn action_hints(reason: &FailoverReason) -> (bool, bool, bool, bool) {
    match reason {
        FailoverReason::Auth => (false, false, true, true),
        FailoverReason::AuthPermanent => (false, false, false, true),
        FailoverReason::Billing => (false, true, false, true),
        FailoverReason::RateLimit => (true, false, false, false),
        FailoverReason::Overloaded => (true, false, false, false),
        FailoverReason::ServerError => (true, false, false, false),
        FailoverReason::Timeout => (true, false, false, false),
        FailoverReason::ContextOverflow => (false, true, false, false),
        FailoverReason::PayloadTooLarge => (false, false, false, false),
        FailoverReason::ModelNotFound => (false, false, false, true),
        FailoverReason::FormatError => (false, false, false, false),
        FailoverReason::ThinkingSignature => (false, false, false, true),
        FailoverReason::LongContextTier => (false, false, false, true),
        FailoverReason::Unknown => (true, false, false, false),
    }
}

/// 402 disambiguation: "usage limit" + "try again" -> rate_limit, otherwise billing.
fn classify_402(message: &str) -> bool {
    let has_usage = message.contains("usage") || message.contains("quota") || message.contains("limit");
    let has_retry = message.contains("try again") || message.contains("retry") || message.contains("please wait");
    has_usage && has_retry
}

fn is_auth_message(msg: &str) -> bool {
    msg.contains("invalid api key")
        || msg.contains("incorrect api key")
        || msg.contains("authentication")
        || msg.contains("unauthorized")
        || msg.contains("forbidden")
        || msg.contains("access denied")
        || msg.contains("permission denied")
        || msg.contains("credential")
}

fn is_billing_message(msg: &str) -> bool {
    msg.contains("billing")
        || msg.contains("payment")
        || msg.contains("exceeded")
        || msg.contains("quota exceeded")
        || msg.contains("usage limit")
        || msg.contains("insufficient")
        || msg.contains("upgrade")
        || msg.contains("out of credits")
        || msg.contains("no credits")
}

fn is_rate_limit_message(msg: &str) -> bool {
    msg.contains("rate limit")
        || msg.contains("too many requests")
        || msg.contains("throttl")
        || msg.contains("per minute")
        || msg.contains("per day")
        || msg.contains("tpm limit")
        || msg.contains("rpm limit")
        || msg.contains("rpd limit")
        || msg.contains("try again later")
}

fn is_context_overflow_message(msg: &str) -> bool {
    msg.contains("token limit")
        || msg.contains("maximum context")
        || msg.contains("prompt too long")
        || msg.contains("input length")
        || msg.contains("context length")
        || msg.contains("context window")
        || (msg.contains("context") && (msg.contains("exceeds") || msg.contains("too long") || msg.contains("overflow")))
        || msg.contains("max tokens")
        || msg.contains("超出上下文")
        || msg.contains("token 限制")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_401() {
        let e = classify_api_error("openrouter", "gpt-4", Some(401), "Invalid API key");
        assert_eq!(e.reason, FailoverReason::Auth);
        assert!(e.should_rotate_credential);
        assert!(e.should_fallback);
        assert!(!e.retryable);
    }

    #[test]
    fn test_billing_402() {
        let e = classify_api_error("openrouter", "gpt-4", Some(402), "Billing: insufficient credits");
        assert_eq!(e.reason, FailoverReason::Billing);
        assert!(e.should_compress);
        assert!(e.should_fallback);
    }

    #[test]
    fn test_rate_limit_429() {
        let e = classify_api_error("openai", "gpt-4", Some(429), "Rate limit exceeded");
        assert_eq!(e.reason, FailoverReason::RateLimit);
        assert!(e.retryable);
    }

    #[test]
    fn test_context_overflow() {
        let e = classify_api_error("anthropic", "claude-3", Some(400), "prompt too long, exceeds context length");
        assert_eq!(e.reason, FailoverReason::ContextOverflow);
        assert!(e.should_compress);
    }

    #[test]
    fn test_server_500() {
        let e = classify_api_error("openrouter", "gpt-4", Some(500), "Internal server error");
        assert_eq!(e.reason, FailoverReason::ServerError);
        assert!(e.retryable);
    }

    #[test]
    fn test_timeout() {
        let e = classify_api_error("custom", "llama-3", None, "Request timed out");
        assert_eq!(e.reason, FailoverReason::Timeout);
        assert!(e.retryable);
    }

    #[test]
    fn test_unknown_retryable() {
        let e = classify_api_error("unknown", "model", None, "Something weird happened");
        assert_eq!(e.reason, FailoverReason::Unknown);
        assert!(e.retryable);
    }

    #[test]
    fn test_402_transient() {
        let e = classify_api_error("openrouter", "model", Some(402), "Usage limit exceeded, please try again later");
        assert_eq!(e.reason, FailoverReason::RateLimit);
        assert!(e.retryable);
    }

    #[test]
    fn test_thinking_signature() {
        let e = classify_api_error("anthropic", "claude-3", Some(400), "thinking signature invalid");
        assert_eq!(e.reason, FailoverReason::ThinkingSignature);
        assert!(e.should_fallback);
    }
}
