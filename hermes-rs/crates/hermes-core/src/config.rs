//! Hermes configuration types.
//!
//! Mirrors the Python `hermes_cli/config.py` DEFAULT_CONFIG and OPTIONAL_ENV_VARS.
//! All configuration is loaded from YAML config + environment variables with
//! env vars taking precedence.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};

use crate::errors::{ErrorCategory, HermesError, Result};
use crate::hermes_home::get_hermes_home;

/// Custom deserializer for context_length that accepts both integers and
/// string values like "256K". Emits a warning for non-integer values,
/// mirroring Python PR 93fe4ead.
fn deserialize_context_length<'de, D>(deserializer: D) -> std::result::Result<Option<usize>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_yaml::Value>::deserialize(deserializer)
        .map_err(|e| <D::Error as serde::de::Error>::custom(e.to_string()))?;
    let Some(value) = value else {
        return Ok(None);
    };

    // Try integer first
    if let Some(n) = value.as_u64() {
        return Ok(Some(n as usize));
    }

    // Try string — warn if it looks like a suffixed value
    if let Some(s) = value.as_str() {
        // Try plain numeric string
        if let Ok(n) = s.parse::<usize>() {
            return Ok(Some(n));
        }

        // Looks like "256K", "128k", "1M", etc. — warn and fall through
        tracing::warn!(
            "Invalid model.context_length in config.yaml: {:?} — \
             must be a plain integer (e.g. 256000, not '256K'). \
             Falling back to auto-detection.",
            s
        );
        eprintln!(
            "\n\u{26A0} Invalid model.context_length in config.yaml: {:?}\n \
             Must be a plain integer (e.g. 256000, not '256K').\n \
             Falling back to auto-detected context window.\n",
            s
        );
    }

    // Null or other type — return None (auto-detect)
    Ok(None)
}

/// Main configuration structure.
///
/// Mirrors the `~/.hermes/config.yaml` schema. Fields are optional to support
/// partial configs where missing values fall back to defaults or env vars.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HermesConfig {
    /// LLM model configuration
    pub model: ModelConfig,
    /// Terminal execution configuration
    pub terminal: TerminalConfig,
    /// File operation configuration
    pub file: FileConfig,
    /// Tool approval settings
    pub approvals: ApprovalConfig,
    /// Skills configuration
    pub skills: SkillsConfig,
    /// Memory configuration
    pub memory: MemoryConfig,
    /// Context compression configuration
    pub compression: CompressionConfig,
    /// MCP server configuration
    pub mcp_servers: HashMap<String, McpServerConfig>,
    /// Cron job configuration
    pub cron: CronConfig,
    /// Browser tool configuration
    pub browser: BrowserConfig,
    /// Auxiliary model configuration
    pub auxiliary_model: AuxiliaryModelConfig,
    /// Security settings
    pub security: SecurityConfig,
    /// Skin / theme settings
    pub skin: Option<String>,
    /// Disabled tools (global)
    pub disabled_tools: Vec<String>,
    /// Disabled toolsets (global)
    pub disabled_toolsets: Vec<String>,
    /// Platform-specific disabled skills
    pub skills_platform_disabled: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    /// Primary model name (e.g., "anthropic/claude-opus-4-6")
    pub name: Option<String>,
    /// Provider override (e.g., "openrouter", "anthropic", "openai")
    pub provider: Option<String>,
    /// Base URL for custom endpoints
    pub base_url: Option<String>,
    /// API key (also read from env: OPENAI_API_KEY, ANTHROPIC_API_KEY, etc.)
    pub api_key: Option<String>,
    /// API mode: "openai", "anthropic_messages", "codex_responses"
    pub api_mode: Option<String>,
    /// Context length override
    #[serde(deserialize_with = "deserialize_context_length")]
    pub context_length: Option<usize>,
    /// Temperature
    pub temperature: Option<f64>,
    /// Max tokens
    pub max_tokens: Option<usize>,
    /// Reasoning effort: "low", "medium", "high"
    pub reasoning_effort: Option<String>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            name: Some("anthropic/claude-opus-4-6".to_string()),
            provider: None,
            base_url: None,
            api_key: None,
            api_mode: Some("anthropic_messages".to_string()),
            context_length: None,
            temperature: Some(0.7),
            max_tokens: None,
            reasoning_effort: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    /// Terminal backend: "local", "docker", "ssh", "modal", "singularity", "daytona"
    pub backend: String,
    /// Working directory for terminal sessions
    pub cwd: Option<PathBuf>,
    /// Sudo password for sudo -S (also from HERMES_SUDO_PASSWORD env)
    pub sudo_password: Option<String>,
    /// Max output size in characters
    pub max_output_size: usize,
    /// Sandbox lifetime in seconds
    pub lifetime_seconds: u64,
    /// Docker image to use
    pub docker_image: Option<String>,
    /// SSH host (for ssh backend)
    pub ssh_host: Option<String>,
    /// SSH user (for ssh backend)
    pub ssh_user: Option<String>,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            backend: "local".to_string(),
            cwd: None,
            sudo_password: None,
            max_output_size: 100_000,
            lifetime_seconds: 3600,
            docker_image: None,
            ssh_host: None,
            ssh_user: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileConfig {
    /// Max read size in characters
    pub max_read_size: usize,
    /// Consecutive re-read limit before hard block
    pub max_consecutive_reads: usize,
    /// Sensitive paths that should be protected
    pub sensitive_paths: Vec<String>,
}

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            max_read_size: 100_000,
            max_consecutive_reads: 4,
            sensitive_paths: vec![
                "/etc/".to_string(),
                "/boot/".to_string(),
                "/var/run/docker.sock".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ApprovalConfig {
    /// Mode: "off", "smart", "strict"
    pub mode: String,
    /// Permanent allowlist of commands
    pub permanent_allowlist: Vec<String>,
}

impl Default for ApprovalConfig {
    fn default() -> Self {
        Self {
            mode: "smart".to_string(),
            permanent_allowlist: vec![],
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillsConfig {
    /// Globally disabled skills
    pub disabled: Vec<String>,
    /// Platform-specific disabled skills (legacy, per-platform key)
    pub platform_disabled: HashMap<String, Vec<String>>,
    /// External skill directories
    pub external_dirs: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    /// Memory backend: "honcho", "holographic", "mem0", "retaindb", etc.
    pub backend: Option<String>,
    /// Whether memory is enabled
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CompressionConfig {
    /// Whether context compression is enabled
    pub enabled: bool,
    /// Target token count for compression
    pub target_tokens: Option<usize>,
    /// Summarization model override
    pub model: Option<String>,
    /// Number of first messages to protect
    pub protect_first_n: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// For stdio transport
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    /// For HTTP/StreamableHTTP transport
    pub url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    /// Timeout in seconds
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CronConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserConfig {
    pub provider: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AuxiliaryModelConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    /// Per-task auxiliary model overrides (e.g., "summarize", "vision", "search")
    pub tasks: HashMap<String, AuxiliaryTaskConfig>,
}

/// Per-task auxiliary model configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AuxiliaryTaskConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SecurityConfig {
    /// Whether to enable OSV vulnerability checking
    pub osv_check: bool,
    /// Website access policy rules path
    pub website_policy_rules: Option<PathBuf>,
}

impl HermesConfig {
    /// Load configuration from the default config file.
    ///
    /// Reads from `~/.hermes/config.yaml` or `./cli-config.yaml` (local override).
    /// Falls back to defaults if the file doesn't exist.
    pub fn load() -> Result<Self> {
        let hermes_home = get_hermes_home();
        let config_path = hermes_home.join("config.yaml");

        // Check for local override
        let local_path = std::env::current_dir()?.join("cli-config.yaml");

        let path = if local_path.exists() {
            &local_path
        } else if config_path.exists() {
            &config_path
        } else {
            return Ok(Self::default());
        };

        let content = std::fs::read_to_string(path)
            .map_err(|e| HermesError::with_source(
                ErrorCategory::ConfigError,
                format!("Failed to read config: {}", path.display()),
                e.into(),
            ))?;

        let config: HermesConfig = serde_yaml::from_str(&content)
            .map_err(|e| HermesError::with_source(
                ErrorCategory::ConfigError,
                format!("Failed to parse config: {}", path.display()),
                e.into(),
            ))?;

        Ok(config)
    }

    /// Save configuration to the default config file.
    pub fn save(&self) -> Result<()> {
        let hermes_home = get_hermes_home();
        std::fs::create_dir_all(&hermes_home)?;
        let config_path = hermes_home.join("config.yaml");

        let content = serde_yaml::to_string(self)
            .map_err(|e| HermesError::with_source(
                ErrorCategory::ConfigError,
                "Failed to serialize config",
                e.into(),
            ))?;

        std::fs::write(&config_path, content)
            .map_err(|e| HermesError::with_source(
                ErrorCategory::ConfigError,
                format!("Failed to write config: {}", config_path.display()),
                e.into(),
            ))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = HermesConfig::default();
        assert_eq!(config.model.name, Some("anthropic/claude-opus-4-6".to_string()));
        assert_eq!(config.terminal.backend, "local");
        assert_eq!(config.approvals.mode, "smart");
    }

    #[test]
    fn test_config_roundtrip() {
        let config = HermesConfig {
            model: ModelConfig {
                name: Some("openai/gpt-4o".to_string()),
                provider: Some("openai".to_string()),
                ..Default::default()
            },
            terminal: TerminalConfig {
                backend: "docker".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        let loaded: HermesConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(loaded.model.name, Some("openai/gpt-4o".to_string()));
        assert_eq!(loaded.terminal.backend, "docker");
    }
}
