//! # Hermes Prompt
//!
//! System prompt construction, context compression, and Anthropic prompt caching.
//! Mirrors the Python `agent/prompt_builder.py`, `agent/context_compressor.py`,
//! and `agent/prompt_caching.py`.

pub mod builder;
pub mod cache_control;
pub mod context_compressor;
pub mod context_references;
pub mod injection_scan;
pub mod skills_prompt;
pub mod manual_compression_feedback;
pub mod soul;
pub mod subdirectory_hints;

// Re-export main public types for convenience.
pub use builder::{
    build_system_prompt, build_context_files_prompt, should_use_developer_role,
    PromptBuilderConfig, PromptBuilderResult, ToolUseEnforcement,
    GOOGLE_MODEL_OPERATIONAL_GUIDANCE, MEMORY_GUIDANCE, OPENAI_MODEL_EXECUTION_GUIDANCE,
    SESSION_SEARCH_GUIDANCE, SKILLS_GUIDANCE, TOOL_USE_ENFORCEMENT_GUIDANCE,
    TOOL_USE_ENFORCEMENT_MODELS, DEFAULT_AGENT_IDENTITY,
};
pub use cache_control::{apply_anthropic_cache_control, CacheTtl};
pub use context_compressor::{CompressorConfig, ContextCompressor};
pub use injection_scan::{sanitize_context_content, scan_context_content};
pub use skills_prompt::build_skills_system_prompt;
pub use soul::{load_soul_md, has_soul_md, CONTEXT_FILE_MAX_CHARS};
