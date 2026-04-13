//! # Hermes Compress
//!
//! Trajectory compression with LLM-based summarization.
//! Mirrors the Python `trajectory_compressor.py`.

pub mod compressor;
pub mod summarizer;

pub use compressor::{
    AggregateMetrics, CompressionConfig, ContextEngine, TrajectoryCompressor, TrajectoryMetrics, Turn,
};
pub use summarizer::Summarizer;

// Prompt caching and context compression are provided by `hermes-prompt`.
// Re-export for convenience.
pub use hermes_prompt::{
    apply_anthropic_cache_control, CacheTtl,
    CompressorConfig, ContextCompressor,
};
