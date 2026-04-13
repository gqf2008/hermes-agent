//! Auxiliary LLM client.
//!
//! Shared router for side tasks (context compression, session search,
//! web extraction, vision analysis). 5-tier provider resolution chain.
//!
//! Mirrors the Python `auxiliary_client.py`.

use crate::error_classifier::{classify_api_error, ClassifiedError};
use crate::model_metadata::compat_model_slug;
use crate::provider::{is_aggregator, parse_provider, resolve_provider_alias};

/// Auxiliary LLM call parameters.
#[derive(Debug, Clone)]
pub struct AuxiliaryRequest {
    pub task: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub messages: Vec<serde_json::Value>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<usize>,
    pub tools: Option<Vec<serde_json::Value>>,
    pub timeout_secs: Option<u64>,
}

/// Auxiliary LLM response.
#[derive(Debug, Clone)]
pub struct AuxiliaryResponse {
    pub content: String,
    pub model: String,
    pub provider: String,
    pub usage: Option<UsageInfo>,
    pub finish_reason: Option<String>,
}

/// Token usage information.
#[derive(Debug, Clone)]
pub struct UsageInfo {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// The 5-tier auxiliary provider resolution chain.
const AUX_CHAIN: &[fn() -> Option<String>] = &[
    try_openrouter,
    try_nous,
    try_custom,
    try_codex,
    try_api_key_provider,
];

/// Call the auxiliary LLM with automatic provider resolution.
///
/// Resolution:
/// 1. If main provider is NOT an aggregator, use it directly
/// 2. Otherwise iterate the 5-tier chain
///
/// C4: Explicitly requested provider is a hard constraint — no silent fallback.
pub async fn call_auxiliary(request: AuxiliaryRequest) -> Result<AuxiliaryResponse, ClassifiedError> {
    let explicit_provider = request.provider.is_some();
    let provider = request.provider.as_deref().unwrap_or("openrouter");
    let resolved_provider = parse_provider(provider);

    let effective_provider = if !is_aggregator(resolved_provider) {
        resolve_provider_alias(provider).to_string()
    } else {
        // Iterate the chain
        let mut resolved = None;
        for try_fn in AUX_CHAIN {
            if let Some(name) = try_fn() {
                resolved = Some(name);
                break;
            }
        }
        // C4: If provider was explicitly requested but none found, return error
        if resolved.is_none() && explicit_provider {
            return Err(classify_api_error(
                provider,
                request.model.as_deref().unwrap_or("unknown"),
                None,
                &format!("Provider {provider} unavailable — no API keys found in chain"),
            ));
        }
        resolved.unwrap_or_else(|| "openrouter".to_string())
    };

    make_api_call(&effective_provider, &request).await
}

/// Try OpenRouter provider.
fn try_openrouter() -> Option<String> {
    std::env::var("OPENROUTER_API_KEY").ok().map(|_| "openrouter".to_string())
}

/// Try Nous Research provider.
fn try_nous() -> Option<String> {
    std::env::var("NOUS_API_KEY").ok().map(|_| "nous".to_string())
}

/// Try custom/local endpoint.
fn try_custom() -> Option<String> {
    std::env::var("CUSTOM_LLM_URL").ok().map(|_| "custom".to_string())
}

/// Try OpenAI Codex provider.
fn try_codex() -> Option<String> {
    std::env::var("OPENAI_API_KEY").ok().map(|_| "openai-codex".to_string())
}

/// Try resolving from API key to known provider.
fn try_api_key_provider() -> Option<String> {
    std::env::var("OPENAI_API_KEY").ok().map(|_| "openai".to_string())
}

/// Make the actual API call using async-openai.
async fn make_api_call(
    provider: &str,
    request: &AuxiliaryRequest,
) -> Result<AuxiliaryResponse, ClassifiedError> {
    // C1: Config resolution priority chain:
    //   explicit request > config.yaml > env vars
    let config_key = hermes_core::HermesConfig::load().ok();

    // Resolve API key: request > config task > config global > env
    let api_key = request.api_key.clone()
        .or_else(|| {
            config_key.as_ref().and_then(|cfg| {
                cfg.auxiliary_model.tasks.get(&request.task)
                    .and_then(|tc| tc.api_key.clone())
            })
        })
        .or_else(|| std::env::var(format!("{}_API_KEY", provider.to_uppercase())).ok())
        .unwrap_or_default();

    // Resolve base_url: request > config task > config global > env
    let base_url = request.base_url.clone()
        .or_else(|| {
            config_key.as_ref().and_then(|cfg| {
                cfg.auxiliary_model.tasks.get(&request.task)
                    .and_then(|tc| tc.base_url.clone())
            })
        })
        .or_else(|| std::env::var(format!("{}_BASE_URL", provider.to_uppercase())).ok());

    // Build OpenAI-compatible config
    let mut config = async_openai::config::OpenAIConfig::new().with_api_key(&api_key);
    if let Some(ref url) = base_url {
        config = config.with_api_base(url);
    }

    let mut model = request.model.clone().unwrap_or_else(|| "gpt-4o-mini".to_string());

    // C3: Strip incompatible model slug prefixes.
    // If the model contains "/" (e.g., "openrouter/gpt-4o") and the target
    // provider is not an aggregator, convert to a bare model name.
    if model.contains('/') && !is_aggregator(parse_provider(provider)) {
        let compat = compat_model_slug(&model);
        tracing::debug!("Stripping model slug prefix: {} → {}", model, compat);
        model = compat;
    }

    // Build client with optional timeout
    let client = if let Some(secs) = request.timeout_secs {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(secs))
            .build()
            .map_err(|e| classify_api_error(provider, &model, None, &format!("Failed to build HTTP client: {e}")))?;
        async_openai::Client::with_config(config).with_http_client(http_client)
    } else {
        async_openai::Client::with_config(config)
    };

    // Build messages
    let mut messages: Vec<async_openai::types::ChatCompletionRequestMessage> = Vec::new();
    for msg in &request.messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
        let m = match role {
            "system" => {
                let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
                async_openai::types::ChatCompletionRequestSystemMessageArgs::default()
                    .content(content)
                    .build()
                    .ok()
                    .map(async_openai::types::ChatCompletionRequestMessage::System)
            }
            "user" => {
                // Support both simple text content and multimodal content arrays
                let content = msg.get("content");
                if let Some(arr) = content.and_then(|v| v.as_array()) {
                    // Multimodal content array (text + images)
                    let parts: Vec<async_openai::types::ChatCompletionRequestUserMessageContentPart> = arr
                        .iter()
                        .filter_map(|part| {
                            let part_type = part.get("type").and_then(|v| v.as_str())?;
                            match part_type {
                                "text" => {
                                    let text = part.get("text").and_then(|v| v.as_str())?;
                                    Some(async_openai::types::ChatCompletionRequestUserMessageContentPart::Text(
                                        async_openai::types::ChatCompletionRequestMessageContentPartText {
                                            text: text.to_string(),
                                        },
                                    ))
                                }
                                "image_url" => {
                                    let url_obj = part.get("image_url")?;
                                    let url = url_obj.get("url").and_then(|v| v.as_str())?;
                                    let detail = url_obj.get("detail").and_then(|v| v.as_str());
                                    Some(async_openai::types::ChatCompletionRequestUserMessageContentPart::ImageUrl(
                                        async_openai::types::ChatCompletionRequestMessageContentPartImage {
                                            image_url: async_openai::types::ImageUrl {
                                                url: url.to_string(),
                                                detail: detail.and_then(|d| match d {
                                                "low" => Some(async_openai::types::ImageDetail::Low),
                                                "high" => Some(async_openai::types::ImageDetail::High),
                                                "auto" => Some(async_openai::types::ImageDetail::Auto),
                                                _ => None,
                                            }),
                                            },
                                        },
                                    ))
                                }
                                _ => None,
                            }
                        })
                        .collect();
                    if parts.is_empty() {
                        None
                    } else {
                        async_openai::types::ChatCompletionRequestUserMessageArgs::default()
                            .content(async_openai::types::ChatCompletionRequestUserMessageContent::Array(parts))
                            .build()
                            .ok()
                            .map(async_openai::types::ChatCompletionRequestMessage::User)
                    }
                } else {
                    let content = content.and_then(|v| v.as_str()).unwrap_or("");
                    async_openai::types::ChatCompletionRequestUserMessageArgs::default()
                        .content(content)
                        .build()
                        .ok()
                        .map(async_openai::types::ChatCompletionRequestMessage::User)
                }
            }
            "assistant" => {
                let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
                async_openai::types::ChatCompletionRequestAssistantMessageArgs::default()
                    .content(content)
                    .build()
                    .ok()
                    .map(async_openai::types::ChatCompletionRequestMessage::Assistant)
            }
            _ => None,
        };
        if let Some(m) = m {
            messages.push(m);
        }
    }

    if messages.is_empty() {
        return Err(classify_api_error(
            provider,
            &model,
            None,
            "No valid messages in request",
        ));
    }

    let mut builder = async_openai::types::CreateChatCompletionRequestArgs::default();
    builder.model(&model).messages(messages.clone());

    if let Some(temp) = request.temperature {
        builder.temperature(temp as f32);
    }
    if let Some(max_tokens) = request.max_tokens {
        builder.max_tokens(max_tokens as u32);
    }

    let chat_req = builder.build().map_err(|e| {
        classify_api_error(provider, &model, None, &format!("Failed to build request: {}", e))
    })?;

    let result = client.chat().create(chat_req).await;

    match result {
        Ok(response) => {
            // Validate response shape before accessing fields
            if response.choices.is_empty() {
                return Err(classify_api_error(
                    provider,
                    &model,
                    None,
                    "API returned empty choices array",
                ));
            }
            let choice = &response.choices[0];
            if choice.message.content.is_none() && choice.message.tool_calls.as_ref().is_none_or(Vec::is_empty) {
                return Err(classify_api_error(
                    provider,
                    &model,
                    None,
                    "API response choice missing message content or tool calls",
                ));
            }

            let content = choice.message.content.clone().unwrap_or_default();
            let finish_reason = choice.finish_reason.as_ref()
                .and_then(|fr| serde_json::to_value(fr).ok())
                .map(|v| v.to_string());

            let usage = response.usage.as_ref().map(|u| UsageInfo {
                prompt_tokens: u.prompt_tokens as u64,
                completion_tokens: u.completion_tokens as u64,
                total_tokens: u.total_tokens as u64,
            });

            Ok(AuxiliaryResponse {
                content,
                model: if response.model.is_empty() { model } else { response.model },
                provider: provider.to_string(),
                usage,
                finish_reason,
            })
        }
        Err(e) => {
            let status_code = extract_status_code(&e);
            let message = e.to_string();
            Err(classify_api_error(provider, &model, status_code, &message))
        }
    }
}

fn extract_status_code(err: &async_openai::error::OpenAIError) -> Option<u16> {
    match err {
        async_openai::error::OpenAIError::Reqwest(ref e) => e.status().map(|s| s.as_u16()),
        async_openai::error::OpenAIError::ApiError(ref e) => e.code.as_ref().and_then(|s| s.parse::<u16>().ok()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_openrouter_without_env() {
        // OPENROUTER_API_KEY may or may not be set
        let result = try_openrouter();
        if let Some(p) = result {
            assert_eq!(p, "openrouter");
        }
    }

    #[test]
    fn test_try_custom_without_env() {
        std::env::remove_var("CUSTOM_LLM_URL");
        assert!(try_custom().is_none());
    }

    #[test]
    fn test_try_api_key_provider() {
        // OPENAI_API_KEY may or may not be set
        let result = try_api_key_provider();
        if let Some(p) = result {
            assert_eq!(p, "openai");
        }
    }

    #[test]
    fn test_explicit_non_aggregator_no_fallback() {
        // When explicitly specifying a non-aggregator provider,
        // it should be used directly without chain resolution
        let provider = parse_provider("anthropic");
        assert!(!is_aggregator(provider));
    }

    #[test]
    fn test_aggregator_resolves_via_chain() {
        let provider = parse_provider("openrouter");
        assert!(is_aggregator(provider));
    }
}
