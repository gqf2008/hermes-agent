//! Hermes CLI Application — main app struct.
//!
//! Interactive CLI with reedline for input, console for output.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use hermes_agent_engine::agent::{AIAgent, AgentConfig};
use hermes_core::{HermesConfig, Result};
use hermes_tools::registry::ToolRegistry;
use hermes_tools::register_all_tools;

/// Main application struct holding configuration and state.
pub struct HermesApp {
    #[allow(dead_code)]
    config: HermesConfig,
}

impl HermesApp {
    pub fn new() -> Result<Self> {
        let config = HermesConfig::load()?;
        Ok(Self { config })
    }

    /// Run the interactive chat loop.
    pub fn run_chat(
        &self,
        model: Option<String>,
        query: Option<String>,
        _image: Option<String>,
        _toolsets: Option<String>,
        _skills: Option<String>,
        _provider: Option<String>,
        _resume: Option<String>,
        _continue_last: Option<Option<String>>,
        _worktree: bool,
        _checkpoints: bool,
        max_turns: Option<u32>,
        _yolo: bool,
        _pass_session_id: bool,
        _source: Option<String>,
        quiet: bool,
        skip_context: bool,
        _skip_memory: bool,
        _voice: bool,
    ) -> Result<()> {
        let model_name = model.unwrap_or_else(|| "anthropic/claude-opus-4.6".to_string());

        // Build tool registry
        let mut registry = ToolRegistry::new();
        register_all_tools(&mut registry);

        if !quiet {
            println!("Hermes Agent — {}", model_name);
            println!("Tools: {} registered", registry.len());
            println!("Type 'quit' or 'exit' to leave, 'clear' to reset context.");
            println!();
        }

        // Resolve provider for default model fallback.
        // When no model is explicitly configured, fall back to the provider's
        // first catalog model so the API call doesn't fail with "model must be non-empty".
        let provider_str = model_name.split('/').next().unwrap_or("").to_lowercase();
        let provider = hermes_llm::provider::parse_provider(&provider_str);
        let final_model = if model_name.is_empty() {
            if let Some(default) = hermes_llm::provider::get_default_model_for_provider(provider.clone()) {
                tracing::info!("No model configured — defaulting to {default} for provider {}", provider);
                default.to_string()
            } else {
                "anthropic/claude-opus-4.6".to_string()
            }
        } else {
            model_name
        };

        // Build agent config
        let max_iterations = max_turns.unwrap_or(90) as usize;
        let config = AgentConfig {
            model: final_model,
            max_iterations,
            skip_context_files: skip_context,
            terminal_cwd: std::env::current_dir().ok(),
            ..AgentConfig::default()
        };

        let interrupt = Arc::new(AtomicBool::new(false));

        // Set up the event runtime for async agent calls
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| hermes_core::HermesError::new(
                hermes_core::ErrorCategory::InternalError,
                format!("Failed to create tokio runtime: {e}"),
            ))?;

        let mut agent = AIAgent::new(config.clone(), Arc::new(registry))?;

        // Single-shot query mode (non-interactive)
        if let Some(ref q) = query {
            let spinner = if !quiet {
                let s = indicatif::ProgressBar::new_spinner();
                s.set_style(
                    indicatif::ProgressStyle::default_spinner()
                        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                        .template("{spinner} {msg}")
                        .unwrap(),
                );
                s.set_message("Thinking...");
                s.enable_steady_tick(std::time::Duration::from_millis(100));
                Some(s)
            } else {
                None
            };

            let turn_result = rt.block_on(async {
                agent.run_conversation(q, None, None).await
            });

            if let Some(s) = spinner {
                s.finish_and_clear();
            }

            if !turn_result.response.is_empty() {
                println!("{}", turn_result.response);
            }

            return Ok(());
        }

        // Set up reedline for input
        let mut line_editor = reedline::Reedline::create();
        let prompt = reedline::DefaultPrompt::default();

        // Main chat loop
        loop {
            // Check for interrupt
            if interrupt.load(std::sync::atomic::Ordering::Relaxed) {
                println!("\nConversation interrupted.");
                break;
            }

            // Read input
            let read_result = line_editor.read_line(&prompt);
            let input = match read_result {
                Ok(reedline::Signal::Success(buffer)) => buffer,
                Ok(reedline::Signal::CtrlD) => {
                    println!();
                    break;
                }
                Ok(reedline::Signal::CtrlC) => {
                    println!("^C");
                    continue;
                }
                Err(e) => {
                    tracing::error!("Input error: {e}");
                    break;
                }
            };

            let trimmed = input.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Handle built-in commands
            match trimmed.to_lowercase().as_str() {
                "quit" | "exit" | ":q" => break,
                "clear" | ":clear" => {
                    agent = AIAgent::new(
                        AgentConfig {
                            model: config.model.clone(),
                            max_iterations: config.max_iterations,
                            skip_context_files: config.skip_context_files,
                            terminal_cwd: config.terminal_cwd.clone(),
                            ..AgentConfig::default()
                        },
                        // Need to re-create registry — just reset by creating new agent
                        Arc::new({
                            let mut r = ToolRegistry::new();
                            register_all_tools(&mut r);
                            r
                        }),
                    )?;
                    println!("Context cleared.");
                    continue;
                }
                _ => {}
            }

            // Show spinner during processing
            let spinner = if !quiet {
                let s = indicatif::ProgressBar::new_spinner();
                s.set_style(
                    indicatif::ProgressStyle::default_spinner()
                        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                        .template("{spinner} {msg}")
                        .unwrap(),
                );
                s.set_message("Thinking...");
                s.enable_steady_tick(std::time::Duration::from_millis(100));
                Some(s)
            } else {
                None
            };

            // Run the agent
            let turn_result = rt.block_on(async {
                agent.run_conversation(trimmed, None, None).await
            });

            // Stop spinner
            if let Some(s) = spinner {
                s.finish_and_clear();
            }

            // Display result
            if !turn_result.response.is_empty() {
                println!("\n{}\n", turn_result.response);
            } else {
                // Show last assistant message from history
                for msg in turn_result.messages.iter().rev() {
                    if let Some(role) = msg.get("role").and_then(|v| v.as_str()) {
                        if role == "assistant" {
                            if let Some(content) = msg.get("content").and_then(|v| v.as_str()) {
                                if !content.is_empty() {
                                    println!("\n{}\n", content);
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            // Show summary in non-quiet mode
            if !quiet {
                println!("[{} API calls, {} messages, {} budget remaining]",
                    turn_result.api_calls,
                    turn_result.messages.len(),
                    agent.budget.remaining(),
                );
                println!();
            }
        }

        if !quiet {
            println!("Goodbye.");
        }

        Ok(())
    }

    pub fn run_setup(&self) -> Result<()> {
        use console::Style;
        use dialoguer::{Confirm, Input};
        use std::fs;

        let green = Style::new().green();
        let yellow = Style::new().yellow();

        let home = hermes_core::get_hermes_home();
        println!("{} Hermes Setup", green.apply_to("Setup"));
        println!("  HERMES_HOME: {}", home.display());
        println!();

        // Ensure directories
        fs::create_dir_all(&home)?;
        fs::create_dir_all(home.join("skills"))?;
        fs::create_dir_all(home.join("bin"))?;
        println!("{} Directories created", green.apply_to("✓"));

        // Check .env file
        let env_path = home.join(".env");
        if env_path.exists() {
            println!("{} .env file exists at {}", green.apply_to("✓"), env_path.display());
        } else {
            println!("{} No .env file found", yellow.apply_to("→"));
            let create = Confirm::new()
                .with_prompt("Create .env file for API keys?")
                .default(true)
                .interact()
                .map_err(|e| hermes_core::HermesError::from(std::io::Error::other(e.to_string())))?;
            if create {
                fs::write(&env_path, "# API Keys — uncomment and fill in:\n# OPENAI_API_KEY=\n# ANTHROPIC_API_KEY=\n# OPENROUTER_API_KEY=\n")?;
                println!("{} Created .env file at {}", green.apply_to("✓"), env_path.display());
            }
        }

        // Check config file
        let config_path = home.join("config.yaml");
        if config_path.exists() {
            println!("{} config.yaml exists at {}", green.apply_to("✓"), config_path.display());
        } else {
            println!("{} No config.yaml found", yellow.apply_to("→"));
            let create = Confirm::new()
                .with_prompt("Create default config.yaml?")
                .default(true)
                .interact()
                .map_err(|e| hermes_core::HermesError::from(std::io::Error::other(e.to_string())))?;
            if create {
                let default_config = serde_yaml::to_string(&serde_yaml::Mapping::new())
                    .map_err(|e| hermes_core::HermesError::from(std::io::Error::other(e.to_string())))?;
                fs::write(&config_path, default_config)?;
                println!("{} Created config.yaml at {}", green.apply_to("✓"), config_path.display());
            }
        }

        // Prompt for primary model
        println!();
        let model: String = Input::new()
            .with_prompt("Primary model (e.g., anthropic/claude-opus-4.6)")
            .default("anthropic/claude-opus-4.6".to_string())
            .interact_text()
            .map_err(|e| hermes_core::HermesError::from(std::io::Error::other(e.to_string())))?;
        println!("{} Model set to: {}", green.apply_to("✓"), model);

        // Prompt for SOUL.md
        let soul_path = home.join("SOUL.md");
        if !soul_path.exists() {
            println!();
            let create_soul = Confirm::new()
                .with_prompt("Create SOUL.md (agent personality/instructions)?")
                .default(true)
                .interact()
                .map_err(|e| hermes_core::HermesError::from(std::io::Error::other(e.to_string())))?;
            if create_soul {
                let prompt_text: String = Input::new()
                    .with_prompt("Agent personality (brief description)")
                    .default("You are a helpful AI assistant.".to_string())
                    .interact_text()
                    .map_err(|e| hermes_core::HermesError::from(std::io::Error::other(e.to_string())))?;
                fs::write(&soul_path, format!("# SOUL.md\n\n{prompt_text}\n"))?;
                println!("{} Created SOUL.md", green.apply_to("✓"));
            }
        }

        println!();
        println!("{} Setup complete!", green.apply_to("Done"));
        Ok(())
    }

    pub fn list_tools(&self) -> Result<()> {
        let mut registry = ToolRegistry::new();
        register_all_tools(&mut registry);

        let tools = registry.list_tools();
        println!("Registered tools: {}", tools.len());
        println!();

        let available = registry.get_available_tools();
        println!("Available tools (prerequisites met): {}", available.len());
        for entry in &available {
            println!("  {}  {}  {}", entry.emoji, entry.name, entry.description);
        }

        let toolsets = registry.list_toolsets();
        println!();
        println!("Toolsets: {:?}", toolsets);

        Ok(())
    }

    pub fn show_tool_info(&self, name: &str) -> Result<()> {
        let mut registry = ToolRegistry::new();
        register_all_tools(&mut registry);

        if let Some(entry) = registry.get(name) {
            println!("Tool: {}", entry.name);
            println!("Toolset: {}", entry.toolset);
            println!("Description: {}", entry.description);
            println!("Emoji: {}", entry.emoji);
            if !entry.requires_env.is_empty() {
                println!("Required env vars: {:?}", entry.requires_env);
            }
            println!();
            println!("Schema:");
            println!("{}", serde_json::to_string_pretty(&entry.schema)?);
        } else {
            println!("Tool '{}' not found.", name);
            let tools = registry.list_tools();
            println!("Available tools: {:?}", tools);
        }

        Ok(())
    }

    pub fn list_tools_for_platform(&self, platform: &str) -> Result<()> {
        use console::Style;
        let dim = Style::new().dim();

        let mut registry = ToolRegistry::new();
        register_all_tools(&mut registry);

        println!("Tools for platform: {}", platform);
        println!();

        let tools = registry.list_tools();
        let mut enabled_count = 0;
        let mut disabled_count = 0;

        // Get disabled tools from config
        let home = if let Ok(h) = std::env::var("HERMES_HOME") {
            std::path::PathBuf::from(h)
        } else if let Some(dir) = dirs::home_dir() {
            dir.join(".hermes")
        } else {
            std::path::PathBuf::from(".hermes")
        };
        let config_path = home.join("config.yaml");
        let disabled_tools: std::collections::HashSet<String> = if config_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&config_path) {
                if let Ok(config) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
                    config.get("tools")
                        .and_then(|t| t.get(platform))
                        .and_then(|p| p.as_sequence())
                        .map(|seq| seq.iter()
                            .filter_map(|v| v.as_str())
                            .map(|s| {
                                if s.starts_with('!') { s[1..].to_string() }
                                else if s.starts_with("mcp:") { s.to_string() }
                                else { s.to_string() }
                            })
                            .collect())
                        .unwrap_or_default()
                } else {
                    Default::default()
                }
            } else {
                Default::default()
            }
        } else {
            Default::default()
        };

        for tool_name in &tools {
            let is_disabled = disabled_tools.contains(tool_name)
                || disabled_tools.iter().any(|d| d.starts_with("mcp:"));
            if is_disabled {
                println!("  {} {}", dim.apply_to("○"), tool_name);
                disabled_count += 1;
            } else {
                println!("  ✓ {}", tool_name);
                enabled_count += 1;
            }
        }

        println!();
        println!("  {} enabled, {} disabled", enabled_count, disabled_count);

        Ok(())
    }

    pub fn list_skills(&self) -> Result<()> {
        use console::Style;
        let green = Style::new().green();
        let yellow = Style::new().yellow();
        let dim = Style::new().dim();

        let result = hermes_tools::skills::handle_skills_list(serde_json::json!({}));
        match result {
            Ok(json_str) => {
                let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
                if json.get("error").is_some() {
                    println!("{} {}", yellow.apply_to("!"), json["error"]);
                    return Ok(());
                }

                let skills = json["skills"].as_array();
                let categories = json["categories"].as_array();
                let count = json["count"].as_u64().unwrap_or(0);

                println!("Installed skills: {}", count);
                if let Some(cats) = categories {
                    println!("Categories: {}", cats.iter().map(|v| v.as_str().unwrap_or("")).collect::<Vec<_>>().join(", "));
                }
                println!();

                if let Some(arr) = skills {
                    if arr.is_empty() {
                        println!("{} No skills found.", dim.apply_to("→"));
                        return Ok(());
                    }
                    for skill in arr {
                        let name = skill.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                        let desc = skill.get("description").and_then(|v| v.as_str()).unwrap_or("");
                        let category = skill.get("category").and_then(|v| v.as_str()).unwrap_or("");
                        let enabled = if skill.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true) {
                            green.apply_to("enabled").to_string()
                        } else {
                            dim.apply_to("disabled").to_string()
                        };
                        println!("  {}  {}  {}  [{}]", name, dim.apply_to(desc), dim.apply_to(category), enabled);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error listing skills: {e}");
            }
        }
        Ok(())
    }

    pub fn show_skill_info(&self, name: &str) -> Result<()> {
        use console::Style;
        let yellow = Style::new().yellow();
        let dim = Style::new().dim();

        let result = hermes_tools::skills::handle_skill_view(serde_json::json!({
            "name": name,
        }));
        match result {
            Ok(json_str) => {
                let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
                if json.get("error").is_some() {
                    println!("{} {}", yellow.apply_to("!"), json["error"]);
                    if let Some(available) = json.get("available_skills") {
                        if let Some(arr) = available.as_array() {
                            println!();
                            println!("{} Available skills:", dim.apply_to("→"));
                            for s in arr {
                                if let Some(sname) = s.as_str() {
                                    println!("    {}", sname);
                                }
                            }
                        }
                    }
                    return Ok(());
                }

                let skill_name = json.get("name").and_then(|v| v.as_str()).unwrap_or(name);
                let desc = json.get("description").and_then(|v| v.as_str()).unwrap_or("");
                let category = json.get("category").and_then(|v| v.as_str()).unwrap_or("");
                let tags = json.get("tags").and_then(|v| v.as_array());
                let enabled = json.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);

                println!("Skill: {}", skill_name);
                println!("Category: {}", category);
                println!("Description: {}", desc);
                println!("Enabled: {}", enabled);
                if let Some(tags) = tags {
                    println!("Tags: {}", tags.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "));
                }
                if let Some(content) = json.get("content").and_then(|v| v.as_str()) {
                    println!();
                    println!("--- SKILL.md content ---");
                    println!("{content}");
                    println!("--- end ---");
                }
            }
            Err(e) => {
                eprintln!("Error viewing skill: {e}");
            }
        }
        Ok(())
    }

    pub fn enable_skill(&self, name: &str, platform: Option<&str>) -> Result<()> {
        use console::Style;
        let green = Style::new().green();
        let yellow = Style::new().yellow();

        let mut config = HermesConfig::load().unwrap_or_default();

        if let Some(p) = platform {
            let list = config
                .skills
                .platform_disabled
                .entry(p.to_string())
                .or_default();
            if list.contains(&name.to_string()) {
                list.retain(|s| s != name);
                println!("  {} Skill '{name}' enabled for platform '{p}'", green.apply_to("✓"));
            } else {
                println!("  {} Skill '{name}' was already enabled for platform '{p}'", yellow.apply_to("→"));
            }
        } else if config.skills.disabled.contains(&name.to_string()) {
            config.skills.disabled.retain(|s| s != name);
            println!("  {} Skill '{name}' enabled", green.apply_to("✓"));
        } else {
            println!("  {} Skill '{name}' was already enabled", yellow.apply_to("→"));
        }

        config.save()?;
        Ok(())
    }

    pub fn disable_skill(&self, name: &str, platform: Option<&str>) -> Result<()> {
        use console::Style;
        let green = Style::new().green();
        let yellow = Style::new().yellow();

        let mut config = HermesConfig::load().unwrap_or_default();

        if let Some(p) = platform {
            let list = config
                .skills
                .platform_disabled
                .entry(p.to_string())
                .or_default();
            if !list.contains(&name.to_string()) {
                list.push(name.to_string());
                println!("  {} Skill '{name}' disabled for platform '{p}'", green.apply_to("✓"));
            } else {
                println!("  {} Skill '{name}' was already disabled for platform '{p}'", yellow.apply_to("→"));
            }
        } else if !config.skills.disabled.contains(&name.to_string()) {
            config.skills.disabled.push(name.to_string());
            println!("  {} Skill '{name}' disabled", green.apply_to("✓"));
        } else {
            println!("  {} Skill '{name}' was already disabled", yellow.apply_to("→"));
        }

        config.save()?;
        Ok(())
    }

    pub fn list_skill_commands(&self) -> Result<()> {
        use console::Style;
        let cyan = Style::new().cyan();
        let dim = Style::new().dim();

        let commands = hermes_tools::skills::scan_skill_commands();

        println!();
        println!("{}", cyan.apply_to("◆ Skill Commands"));
        println!();

        if commands.is_empty() {
            println!("  {}", dim.apply_to("No skill commands found."));
            println!("  Install skills with: hermes skills install <name>");
            println!();
            return Ok(());
        }

        for (cmd, info) in &commands {
            println!("  {cmd:<20} {}", dim.apply_to(&info.name));
            println!("  {:<20} {}", "", dim.apply_to(&info.description));
        }
        println!();
        println!("  Total: {} command(s)", commands.len());
        println!();

        Ok(())
    }

    pub fn run_gateway(&self) -> Result<()> {
        use console::Style;
        use hermes_gateway::runner::{GatewayRunner, load_gateway_config, GatewayConfig};
        use hermes_gateway::config::Platform;

        let green = Style::new().green();
        let cyan = Style::new().cyan();
        let dim = Style::new().dim();

        println!("{} Hermes Gateway", cyan.apply_to("Gateway"));
        println!();

        // Load config
        let gateway_config = load_gateway_config();
        let platform_count = gateway_config.platforms.iter().filter(|p| p.enabled).count();
        println!("  {} {} platform(s) configured", green.apply_to("✓"), platform_count);

        if platform_count == 0 {
            println!("  No platforms configured. Set FEISHU_APP_ID/SECRET or WEIXIN_SESSION_KEY,");
            println!("  or add platforms to ~/.hermes/config.yaml under gateway.platforms");
            return Ok(());
        }

        // Create and initialize runner
        let mut runner = GatewayRunner::new(GatewayConfig {
            platforms: gateway_config.platforms,
            default_model: gateway_config.default_model.clone(),
        });
        runner.initialize();

        let status = runner.status();
        println!("  Feishu: {}", if status.feishu_configured { green.apply_to("configured").to_string() } else { dim.apply_to("not configured").to_string() });
        println!("  Weixin: {}", if status.weixin_configured { green.apply_to("configured").to_string() } else { dim.apply_to("not configured").to_string() });
        println!();

        // Set up message handler that routes to the agent engine
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| hermes_core::HermesError::new(hermes_core::ErrorCategory::InternalError, format!("Failed to create tokio runtime: {e}")))?;

        // Build agent for gateway use
        let model_name = gateway_config.default_model.clone();
        let mut agent_registry = ToolRegistry::new();
        register_all_tools(&mut agent_registry);

        // Provider default model fallback for gateway
        let provider_str = model_name.split('/').next().unwrap_or("openrouter").to_lowercase();
        let provider = hermes_llm::provider::parse_provider(&provider_str);
        let final_model = if model_name.is_empty() {
            if let Some(default) = hermes_llm::provider::get_default_model_for_provider(provider.clone()) {
                tracing::info!("No model configured — defaulting to {default} for provider {}", provider);
                default.to_string()
            } else {
                "anthropic/claude-opus-4.6".to_string()
            }
        } else {
            model_name
        };

        let agent_config = AgentConfig {
            model: final_model.clone(),
            max_iterations: 90,
            skip_context_files: false,
            terminal_cwd: std::env::current_dir().ok(),
            ..AgentConfig::default()
        };

        let agent = AIAgent::new(agent_config, Arc::new(agent_registry))
            .map_err(|e| hermes_core::HermesError::new(hermes_core::ErrorCategory::InternalError, format!("Failed to create agent: {e}")))?;

        tracing::info!("Gateway started with {} platform(s) using model: {}", platform_count, final_model);
        println!("  Gateway running (Ctrl+C to stop)");

        // Create agent-based message handler
        struct AgentHandler {
            agent: tokio::sync::Mutex<AIAgent>,
        }

        #[async_trait::async_trait]
        impl hermes_gateway::runner::MessageHandler for AgentHandler {
            async fn handle_message(
                &self,
                _platform: Platform,
                chat_id: &str,
                content: &str,
            ) -> std::result::Result<hermes_gateway::runner::HandlerResult, String> {
                tracing::info!("Gateway received from {chat_id}: {}", content.chars().take(50).collect::<String>());

                let mut agent = self.agent.lock().await;
                let turn_result = agent.run_conversation(content, None, None).await;
                if turn_result.response.is_empty() {
                    Err("Agent returned no response".to_string())
                } else {
                    Ok(hermes_gateway::runner::HandlerResult {
                        response: turn_result.response.clone(),
                        messages: turn_result.messages.clone(),
                        compression_exhausted: turn_result.compression_exhausted,
                    })
                }
            }

            fn interrupt(&self, _chat_id: &str, _new_message: &str) {
                // Signal the agent to stop the current turn immediately.
                // The new message will be queued and processed after this
                // turn completes. Mirrors Python PR a8b7db35.
                let agent = self.agent.try_lock();
                if let Ok(mut a) = agent {
                    a.close();
                } else {
                    tracing::debug!("Agent handler locked during interrupt — flag already set");
                }
            }
        }

        let _ = rt.block_on(async {
            let handler = std::sync::Arc::new(AgentHandler {
                agent: tokio::sync::Mutex::new(agent),
            });
            runner.set_message_handler(handler).await;
            runner.run().await
                .map_err(|e| hermes_core::HermesError::new(hermes_core::ErrorCategory::InternalError, e))
        });

        Ok(())
    }

    pub fn run_doctor(&self) -> Result<()> {
        use console::Style;

        let green = Style::new().green();
        let yellow = Style::new().yellow();
        let red = Style::new().red();
        let cyan = Style::new().cyan();
        let dim = Style::new().dim();

        let mut issues = Vec::new();

        println!();
        println!("{}", cyan.apply_to("┌─────────────────────────────────────────────────────────┐"));
        println!("{}", cyan.apply_to("│                 Hermes Doctor                          │"));
        println!("{}", cyan.apply_to("└─────────────────────────────────────────────────────────┘"));

        // ── Configuration ──────────────────────────────────────────────
        println!();
        println!("{}", cyan.apply_to("◆ Configuration"));

        let hermes_home = hermes_core::hermes_home::get_hermes_home();
        if hermes_home.exists() {
            println!("  {} HERMES_HOME exists", green.apply_to("✓"));
        } else {
            println!("  {} HERMES_HOME not found", yellow.apply_to("⚠"));
            println!("    {}", dim.apply_to("(will be created on first use)"));
        }

        // Config file
        let config_path = hermes_home.join("config.yaml");
        if config_path.exists() {
            println!("  {} config.yaml exists", green.apply_to("✓"));
        } else {
            println!("  {} config.yaml not found", yellow.apply_to("⚠"));
            println!("    {}", dim.apply_to("(using defaults)"));
        }

        // .env file
        let env_path = hermes_home.join(".env");
        if env_path.exists() {
            println!("  {} .env file exists", green.apply_to("✓"));
            // Check for API keys
            let content = std::fs::read_to_string(&env_path).unwrap_or_default();
            let has_key = content.contains("OPENROUTER_API_KEY")
                || content.contains("OPENAI_API_KEY")
                || content.contains("ANTHROPIC_API_KEY")
                || content.contains("NOUS_API_KEY")
                || content.contains("OPENAI_BASE_URL");
            if has_key {
                println!("  {} API key configured", green.apply_to("✓"));
            } else {
                println!("  {} No API key found in .env", yellow.apply_to("⚠"));
                issues.push("Run 'hermes setup' to configure API keys".to_string());
            }
        } else {
            println!("  {} .env file missing", red.apply_to("✗"));
            issues.push("Run 'hermes setup' to create .env".to_string());
        }

        // ── Model ──────────────────────────────────────────────────────
        println!();
        println!("{}", cyan.apply_to("◆ Model"));
        let model = &self.config.model.name.as_deref().unwrap_or("anthropic/claude-opus-4.6");
        println!("  {} Primary model: {}", green.apply_to("✓"), model);

        // Check provider env hints
        let provider = model.split('/').next().unwrap_or("");
        match provider {
            "anthropic" if std::env::var("ANTHROPIC_API_KEY").is_err() && std::env::var("OPENROUTER_API_KEY").is_err() => {
                println!("  {} ANTHROPIC_API_KEY not set", yellow.apply_to("⚠"));
            }
            "openai" if std::env::var("OPENAI_API_KEY").is_err() => {
                println!("  {} OPENAI_API_KEY not set", yellow.apply_to("⚠"));
            }
            "openrouter" if std::env::var("OPENROUTER_API_KEY").is_err() => {
                println!("  {} OPENROUTER_API_KEY not set", yellow.apply_to("⚠"));
            }
            _ => println!("  {} Provider: {}", green.apply_to("✓"), provider),
        }

        // ── Directory Structure ────────────────────────────────────────
        println!();
        println!("{}", cyan.apply_to("◆ Directory Structure"));

        let expected_subdirs = ["cron", "sessions", "logs", "skills", "memories"];
        for subdir_name in &expected_subdirs {
            let subdir_path = hermes_home.join(subdir_name);
            if subdir_path.exists() {
                println!("  {} {}/ exists", green.apply_to("✓"), subdir_name);
            } else {
                println!("  {} {}/ not found", yellow.apply_to("⚠"), subdir_name);
                println!("    {}", dim.apply_to("(will be created on first use)"));
            }
        }

        // SOUL.md
        let soul_path = hermes_home.join("SOUL.md");
        if soul_path.exists() {
            let content = std::fs::read_to_string(&soul_path).unwrap_or_default();
            let has_content = content.lines().any(|l| {
                let trimmed = l.trim();
                !trimmed.is_empty()
                    && !trimmed.starts_with("<!--")
                    && !trimmed.starts_with("-->")
                    && !trimmed.starts_with("#")
            });
            if has_content {
                println!("  {} SOUL.md exists (persona configured)", green.apply_to("✓"));
            } else {
                println!("  {} SOUL.md exists but empty", yellow.apply_to("⚠"));
                println!("    {}", dim.apply_to("(edit it to customize personality)"));
            }
        } else {
            println!("  {} SOUL.md not found", yellow.apply_to("⚠"));
            println!("    {}", dim.apply_to("(create it to give Hermes a custom personality)"));
        }

        // ── Session Database ───────────────────────────────────────────
        println!();
        println!("{}", cyan.apply_to("◆ Session Database"));
        let db_path = hermes_home.join("sessions.db");
        if db_path.exists() {
            println!("  {} sessions.db exists", green.apply_to("✓"));
            // Try to open and check
            match hermes_state::SessionDB::open(&db_path) {
                Ok(db) => {
                    if let Ok(count) = db.session_count(None) {
                        println!("  {} {} session(s) recorded", green.apply_to("✓"), count);
                    }
                    if let Ok(fts_count) = db.search_messages("test", None, None, None, 1, 0) {
                        if !fts_count.is_empty() || fts_count.is_empty() {
                            // FTS5 table exists if no error
                            println!("  {} FTS5 search available", green.apply_to("✓"));
                        }
                    }
                }
                Err(e) => {
                    println!("  {} Failed to open: {e}", red.apply_to("✗"));
                    issues.push(format!("Session database error: {e}"));
                }
            }
        } else {
            println!("  {} sessions.db not found", yellow.apply_to("⚠"));
            println!("    {}", dim.apply_to("(will be created on first conversation)"));
        }

        // ── Tools ──────────────────────────────────────────────────────
        println!();
        println!("{}", cyan.apply_to("◆ Tools"));
        let mut registry = hermes_tools::registry::ToolRegistry::new();
        hermes_tools::register_all_tools(&mut registry);
        let total = registry.len();
        let available = registry.get_available_tools();
        println!("  {} {} tools registered", green.apply_to("✓"), total);
        println!("  {} {} available (prerequisites met)", green.apply_to("✓"), available.len());

        // Check for common external tools
        let external_checks = [
            ("docker", "Docker (terminal backend)"),
            ("bash", "Bash shell"),
        ];
        for (cmd, desc) in &external_checks {
            if which::which(cmd).is_ok() {
                println!("  {} {cmd} ({desc})", green.apply_to("✓"));
            } else {
                println!("  {} {cmd} not found", yellow.apply_to("⚠"));
                println!("    {}", dim.apply_to(desc));
            }
        }

        // ── Summary ────────────────────────────────────────────────────
        println!();
        if issues.is_empty() {
            println!("  {} No issues found!", green.apply_to("✓"));
        } else {
            println!("{}", cyan.apply_to("◆ Issues Found"));
            for (i, issue) in issues.iter().enumerate() {
                println!("  {}. {issue}", i + 1);
            }
        }
        println!();

        Ok(())
    }

    pub fn list_models(&self) -> Result<()> {
        use console::Style;

        let green = Style::new().green();
        let yellow = Style::new().yellow();
        let cyan = Style::new().cyan();
        let dim = Style::new().dim();

        println!();
        println!("{}", cyan.apply_to("◆ Available Providers"));
        println!();

        let providers = [
            ("openrouter", "https://openrouter.ai/api/v1", "OPENROUTER_API_KEY", true),
            ("nous", "https://api.nousresearch.com/v1", "NOUS_API_KEY", false),
            ("anthropic", "https://api.anthropic.com", "ANTHROPIC_API_KEY", false),
            ("openai", "https://api.openai.com/v1", "OPENAI_API_KEY", false),
            ("gemini", "https://generativelanguage.googleapis.com/...", "GOOGLE_API_KEY", false),
            ("zai", "https://api.z.ai/api/paas/v4/", "ZAI_API_KEY", false),
            ("kimi", "https://api.moonshot.cn/v1", "KIMI_API_KEY", false),
            ("minimax", "https://api.minimax.io/v1", "MINIMAX_API_KEY", false),
            ("codex", "https://api.openai.com/v1", "OPENAI_API_KEY", false),
        ];

        println!("{:<14} {:<50} {:<22} {:<10}", "Provider", "Base URL", "Env Var", "Status");
        println!("{}", "-".repeat(100));

        for (name, url, env_var, is_aggregator) in &providers {
            let has_key = std::env::var(env_var).is_ok();
            let status = if has_key {
                green.apply_to("✓ configured").to_string()
            } else {
                yellow.apply_to("⚠ not set").to_string()
            };
            let label = if *is_aggregator {
                format!("{name} (agg)")
            } else {
                name.to_string()
            };
            println!("{:<14} {:<50} {:<22} {}", label, url, env_var, status);
        }

        println!();
        println!("  {}", dim.apply_to("Fallback chain: openrouter → nous → codex → gemini → zai → kimi → minimax → anthropic"));
        println!();

        // Current model
        let model = &self.config.model.name.as_deref().unwrap_or("anthropic/claude-opus-4.6");
        println!("  {} Current model: {}", green.apply_to("→"), model);

        // Custom base URL
        if let Some(base_url) = &self.config.model.base_url {
            println!("  {} Custom base URL: {}", green.apply_to("→"), base_url);
        }

        println!();

        Ok(())
    }

    pub fn list_profiles(&self) -> Result<()> {
        use console::Style;
        let cyan = Style::new().cyan();
        let dim = Style::new().dim();

        println!();
        println!("{}", cyan.apply_to("◆ Profiles"));
        println!();

        let hermes_home = hermes_core::hermes_home::get_hermes_home();
        println!("  HERMES_HOME: {}", hermes_home.display());
        println!();

        // Check for profiles directory
        let profiles_dir = hermes_home.parent().map(|p| p.join("profiles")).filter(|p| p.exists());
        if let Some(dir) = profiles_dir {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                let profiles: Vec<_> = entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_dir())
                    .collect();
                if profiles.is_empty() {
                    println!("  No profiles found.");
                } else {
                    println!("  {} profile(s):", profiles.len());
                    for entry in &profiles {
                        println!("    - {}", entry.file_name().to_string_lossy());
                    }
                }
            }
        } else {
            println!("  {}", dim.apply_to("No profiles directory found. Profiles are stored under ~/.hermes/profiles/"));
        }
        println!();

        Ok(())
    }

    pub fn create_profile(&self, name: &str) -> Result<()> {
        use console::Style;
        let green = Style::new().green();
        let cyan = Style::new().cyan();

        let hermes_home = hermes_core::hermes_home::get_hermes_home();
        let profiles_dir = hermes_home.parent().map(|p| p.join("profiles")).unwrap_or_else(|| hermes_home.join("profiles"));

        let profile_dir = profiles_dir.join(name);
        if profile_dir.exists() {
            println!("  {} Profile '{name}' already exists at: {}", yellow_style().apply_to("⚠"), profile_dir.display());
            return Ok(());
        }

        std::fs::create_dir_all(&profile_dir)
            .map_err(|e| hermes_core::HermesError::new(hermes_core::ErrorCategory::InternalError, e.to_string()))?;

        // Create basic .env and config.yaml
        let env_path = profile_dir.join(".env");
        std::fs::write(&env_path, "# API keys for this profile\n")
            .map_err(|e| hermes_core::HermesError::new(hermes_core::ErrorCategory::InternalError, e.to_string()))?;

        let config_path = profile_dir.join("config.yaml");
        std::fs::write(&config_path, format!("# Hermes profile: {}\nmodel:\n  name: anthropic/claude-opus-4.6\n", name))
            .map_err(|e| hermes_core::HermesError::new(hermes_core::ErrorCategory::InternalError, e.to_string()))?;

        println!("  {} Profile '{name}' created at: {}", green.apply_to("✓"), profile_dir.display());
        println!("  {}", cyan.apply_to("Set HERMES_HOME to switch profiles:"));
        println!("    {}", cyan.apply_to(format!("  HERMES_HOME={} hermes", profile_dir.display())));
        println!();

        Ok(())
    }

    pub fn use_profile(&self, name: &str) -> Result<()> {
        use console::Style;
        let green = Style::new().green();
        let yellow = Style::new().yellow();

        let hermes_home = hermes_core::hermes_home::get_hermes_home();
        let profiles_dir = hermes_home.parent().map(|p| p.join("profiles")).unwrap_or_else(|| hermes_home.join("profiles"));
        let profile_dir = profiles_dir.join(name);

        if !profile_dir.exists() {
            println!("  {} Profile '{name}' not found. Create it first with 'hermes profile create {name}'", yellow.apply_to("✗"));
            return Ok(());
        }

        println!("  {} To use profile '{name}', set:", yellow.apply_to("→"));
        println!("    {}", green.apply_to(format!("HERMES_HOME={}", profile_dir.display())));
        println!();
        println!("  On Unix: export HERMES_HOME={}", profile_dir.display());
        println!("  On Windows: set HERMES_HOME={}", profile_dir.display());

        Ok(())
    }
}

/// Delete a profile.
pub fn cmd_profile_delete(name: &str, force: bool) -> anyhow::Result<()> {
    use console::Style;
    let yellow = Style::new().yellow();
    let green = Style::new().green();

    let profiles_dir = get_profiles_dir();
    let profile_dir = profiles_dir.join(name);

    if !profile_dir.exists() {
        println!("  {} Profile '{name}' not found.", yellow.apply_to("✗"));
        return Ok(());
    }

    if !force {
        println!("  This will delete profile '{}' and all its data.", name);
        print!("  Continue? [y/N]: ");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);
        if !input.trim().eq_ignore_ascii_case("y") && !input.trim().eq_ignore_ascii_case("yes") {
            println!("  {}", Style::new().dim().apply_to("Delete cancelled."));
            return Ok(());
        }
    }

    let _ = std::fs::remove_dir_all(&profile_dir);
    println!("  {} Profile '{name}' deleted.", green.apply_to("✓"));
    Ok(())
}

/// Show profile details.
pub fn cmd_profile_show(name: &str) -> anyhow::Result<()> {
    use console::Style;
    let cyan = Style::new().cyan();
    let yellow = Style::new().yellow();

    let profiles_dir = get_profiles_dir();
    let profile_dir = profiles_dir.join(name);

    if !profile_dir.exists() {
        println!("  {} Profile '{name}' not found.", yellow.apply_to("✗"));
        return Ok(());
    }

    println!();
    println!("{}", cyan.apply_to("◆ Profile: {name}"));
    println!("  Path: {}", profile_dir.display());
    println!();

    if profile_dir.join("config.yaml").exists() {
        println!("  Config: present");
    }
    if profile_dir.join(".env").exists() {
        println!("  Env: present");
    }
    println!();

    Ok(())
}

/// Manage profile wrapper scripts.
pub fn cmd_profile_alias(name: &str) -> anyhow::Result<()> {
    use console::Style;
    let cyan = Style::new().cyan();
    let dim = Style::new().dim();

    let profiles_dir = get_profiles_dir();
    let profile_dir = profiles_dir.join(name);

    if !profile_dir.exists() {
        println!("  {} Profile '{name}' not found.", Style::new().yellow().apply_to("✗"));
        return Ok(());
    }

    println!();
    println!("{}", cyan.apply_to("◆ Profile Alias: {name}"));
    println!();
    println!("  {}", dim.apply_to("Create a shell alias to quickly switch to this profile:"));
    println!();
    println!("  bash/zsh: alias hermes-{name}='HERMES_HOME={} hermes'", profile_dir.display());
    println!("  fish:     alias hermes-{name}='env HERMES_HOME={} hermes'", profile_dir.display());
    println!();

    Ok(())
}

/// Rename a profile.
pub fn cmd_profile_rename(old_name: &str, new_name: &str) -> anyhow::Result<()> {
    use console::Style;
    let green = Style::new().green();
    let yellow = Style::new().yellow();

    let profiles_dir = get_profiles_dir();
    let old_dir = profiles_dir.join(old_name);
    let new_dir = profiles_dir.join(new_name);

    if !old_dir.exists() {
        println!("  {} Profile '{old_name}' not found.", yellow.apply_to("✗"));
        return Ok(());
    }
    if new_dir.exists() {
        println!("  {} Profile '{new_name}' already exists.", yellow.apply_to("✗"));
        return Ok(());
    }

    std::fs::rename(&old_dir, &new_dir)?;
    println!("  {} Profile renamed: {old_name} → {new_name}", green.apply_to("✓"));
    Ok(())
}

/// Export a profile to archive.
pub fn cmd_profile_export(name: &str, output: Option<&str>) -> anyhow::Result<()> {
    use console::Style;
    let green = Style::new().green();
    let yellow = Style::new().yellow();

    let profiles_dir = get_profiles_dir();
    let profile_dir = profiles_dir.join(name);

    if !profile_dir.exists() {
        println!("  {} Profile '{name}' not found.", yellow.apply_to("✗"));
        return Ok(());
    }

    let default_out = format!("{name}.tar.gz");
    let out_path = output.unwrap_or(&default_out);
    println!("  Exporting profile '{name}' to {out_path}...");

    // Use tar on Unix, zip fallback on Windows
    let result = if cfg!(unix) {
        std::process::Command::new("tar")
            .args(["-czf", out_path, "-C", &profiles_dir.to_string_lossy(), name])
            .output()
    } else {
        std::process::Command::new("tar")
            .args(["-cf", out_path, "-C", &profiles_dir.to_string_lossy(), name])
            .output()
    };

    match result {
        Ok(out) if out.status.success() => {
            println!("  {} Exported to: {out_path}", green.apply_to("✓"));
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            println!("  {} Export failed: {}", yellow.apply_to("⚠"), err.trim());
        }
        Err(e) => {
            println!("  {} Failed: {e}", yellow.apply_to("⚠"));
        }
    }
    Ok(())
}

/// Import a profile from archive.
pub fn cmd_profile_import(path: &str) -> anyhow::Result<()> {
    use console::Style;
    let green = Style::new().green();
    let yellow = Style::new().yellow();

    let archive = std::path::Path::new(path);
    if !archive.exists() {
        println!("  {} Archive not found: {path}", yellow.apply_to("✗"));
        return Ok(());
    }

    let profiles_dir = get_profiles_dir();
    std::fs::create_dir_all(&profiles_dir)?;

    let output = std::process::Command::new("tar")
        .args(["-xf", path, "-C", &profiles_dir.to_string_lossy()])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            println!("  {} Profile imported to: {}", green.apply_to("✓"), profiles_dir.display());
        }
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr);
            println!("  {} Import failed: {}", yellow.apply_to("⚠"), err.trim());
        }
        Err(e) => {
            println!("  {} Failed: {e}", yellow.apply_to("⚠"));
        }
    }
    Ok(())
}

fn get_profiles_dir() -> std::path::PathBuf {
    let hermes_home = hermes_core::get_hermes_home();
    hermes_home.join("profiles")
}

fn yellow_style() -> console::Style {
    console::Style::new().yellow()
}
