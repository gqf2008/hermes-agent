//! # Hermes Tools
//!
//! Tool registry and all ~60 tool implementations.
//! Mirrors the Python `tools/` directory and `model_tools.py`.

pub mod registry;
pub mod tool_result;
pub mod toolsets_def;

// Simple tools
pub mod budget_config;
pub mod interrupt;
pub mod url_safety;
pub mod website_policy;
pub mod ansi_strip;
pub mod binary_extensions;
pub mod debug_helpers;
pub mod fuzzy_match;
pub mod patch_parser;
pub mod osv_check;
pub mod credential_files;
pub mod tool_result_storage;
pub mod openrouter_client;
pub mod transcription;

// Complex tools (stub modules — implementations added progressively)
pub mod approval;
pub mod file_ops;
pub mod terminal;
pub mod process_reg;
pub mod web;
pub mod browser;
pub mod code_exec;
pub mod delegate;
pub mod mcp_client;
pub mod memory;
pub mod todo;
pub mod skills;
pub mod skills_hub;
pub mod skills_sync;
pub mod tts;
pub mod voice;
pub mod vision;
pub mod image_gen;
pub mod clarify;
pub mod session_search;
pub mod homeassistant;
pub mod send_message;
pub mod checkpoint;
pub mod credentials;
pub mod rl_training;
pub mod skills_guard;
pub mod tirith;
pub mod cron_tools;
pub mod moa;

// Backend helpers
pub mod env_passthrough;
pub mod managed_tool_gateway;
pub mod mcp_oauth;
pub mod neutts_synth;
pub mod path_security;
pub mod tool_backend_helpers;

// Environment backends
pub mod environments;

use std::sync::Arc;

/// Register all tools in the given registry.
///
/// This is the single entry point called at startup. Each tool module
/// exposes a `register_*` function that adds its tools to the registry.
pub fn register_all_tools(registry: &mut crate::registry::ToolRegistry) {
    // Core tools
    todo::register(registry);
    clarify::register(registry);
    fuzzy_match::register(registry);
    memory::register_memory_tool(registry);
    approval::register_approval_tool(registry);
    web::register_web_tools(registry);
    vision::register_vision_tool(registry);
    homeassistant::register_ha_tools(registry);
    skills::register_skills_tools(registry);
    skills_hub::register(registry);
    file_ops::register_file_tools(registry);
    image_gen::register_image_tool(registry);
    cron_tools::register_cron_tools(registry);
    session_search::register_session_search_tool(registry);
    send_message::register_send_message_tool(registry);
    tts::register_tts_tool(registry);
    voice::register_voice_tool(registry);
    process_reg::register_process_tool(registry);
    terminal::register_terminal_tool(registry);
    delegate::register_delegate_tool(registry);
    mcp_client::register_mcp_client_tool(registry);
    rl_training::register_rl_tools(registry);
    browser::register_browser_tools(registry);
    // code_exec registered last — it needs a snapshot of the full registry
    // so the Python sandbox can RPC-dispatch to any registered tool.
    let registry_arc = Arc::new(registry.clone());
    code_exec::register_code_exec_tool(registry, registry_arc);
    moa::register_moa_tool(registry);
}
