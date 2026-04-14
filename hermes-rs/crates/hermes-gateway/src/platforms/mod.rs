//! Platform-specific messaging adapters.
//!
//! Mirrors the Python `gateway/platforms/` directory.
//! Each adapter handles send/receive for its platform.

pub mod api_server;
pub mod dingtalk;
pub mod feishu;
pub mod wecom;
pub mod weixin;
