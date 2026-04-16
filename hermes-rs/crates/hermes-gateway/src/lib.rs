//! # Hermes Gateway
//!
//! Gateway session management, platform configuration, and messaging adapters.
//! Mirrors the Python `gateway/` directory.

pub mod config;
pub mod dedup;
pub mod runner;
pub mod session;
pub mod platforms;
pub mod stream_consumer;
pub mod mcp_config;
