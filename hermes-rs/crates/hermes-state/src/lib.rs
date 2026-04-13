//! # Hermes State
//!
//! SQLite session store with WAL mode and FTS5 full-text search.
//! Mirrors the Python `hermes_state.py` SessionDB class.

pub mod insights;
pub mod models;
pub mod schema;
pub mod session_db;

pub use insights::InsightsEngine;
pub use session_db::{SessionDB, StateError};
