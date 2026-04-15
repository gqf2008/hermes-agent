//! # Hermes Agent Engine
//!
//! Core agent conversation loop (AIAgent class).
//! Mirrors the Python `run_agent.py`.

pub mod agent;
pub mod budget;
pub mod memory_manager;
pub mod memory_provider;
pub mod message_loop;
pub mod review_agent;
pub mod self_evolution;
pub mod skill_commands;
pub mod smart_model_routing;
pub mod subagent;
pub mod title_generator;
pub mod trajectory;

pub use agent::AIAgent;
pub use memory_manager::{build_memory_context_block, sanitize_context as sanitize_memory_context, MemoryManager};
pub use memory_provider::MemoryProvider;
pub use message_loop::{MessageLoop, MessageResult, PlatformMessage};
pub use smart_model_routing::{
    choose_cheap_model_route, parse_routing_config, resolve_turn_route, RoutingConfig, TurnRoute,
};
pub use title_generator::{generate_title, maybe_auto_title, SessionTitleStore};
pub use trajectory::{
    has_incomplete_scratchpad, messages_to_conversation, save_trajectory,
    ConversationTurn, TrajectoryEntry,
};
pub use skill_commands::{
    build_plan_path, build_skill_invocation_message, get_skill_commands, load_skill_payload,
    resolve_skill_command_key, scan_skill_commands, SkillCommand, SkillPayload,
};
