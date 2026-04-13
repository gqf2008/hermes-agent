//! # Hermes Core
//!
//! Shared types, constants, error definitions, and configuration for the Hermes Agent system.
//!
//! This crate provides:
//! - `HermesError` — unified error type with rich context
//! - `get_hermes_home()` — profile-aware path resolution
//! - `HermesConfig` — typed configuration from YAML + env vars
//! - Logging setup via `tracing`
//! - Time utilities

pub mod config;
pub mod constants;
pub mod env_loader;
pub mod errors;
pub mod hermes_home;
pub mod logging;
pub mod redact;
pub mod time;

pub use config::HermesConfig;
pub use env_loader::{load_dotenv_override, load_hermes_dotenv};
pub use errors::{ApiErrorDetails, ErrorCategory, HermesError, Result};
pub use hermes_home::{display_hermes_home, get_hermes_home, get_hermes_dir, get_default_hermes_root};
pub use redact::redact_sensitive_text;
