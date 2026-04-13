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
