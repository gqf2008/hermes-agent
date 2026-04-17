//! # Hermes LLM
//!
//! LLM client library with multi-provider routing, credential pooling,
//! and a 7-tier fallback chain.

pub mod anthropic;
pub mod auxiliary_client;
pub mod bedrock;
pub mod client;
pub mod codex;
pub mod credential_pool;
pub mod error_classifier;
pub mod model_metadata;
pub mod model_normalize;
pub mod models_dev;
pub mod pricing;
pub mod provider;
pub mod rate_limit;
pub mod reasoning;
pub mod retry;
pub mod runtime_provider;
pub mod token_estimate;
pub mod tool_call;

// Re-export key types for convenience.
pub use models_dev::{
    fetch_models_dev, get_model_capabilities, get_model_info, get_provider_info,
    list_agentic_models, list_provider_models, lookup_context, search_models_dev,
    ModelCapabilities, ModelInfo, ModelSearchResult, ProviderInfo,
};
