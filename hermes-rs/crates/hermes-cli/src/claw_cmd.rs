//! Claw migration command.
//!
//! Mirrors Python: hermes claw (migrate from Claude Code or other agents)

use console::Style;
use std::path::PathBuf;

fn cyan() -> Style { Style::new().cyan() }
fn green() -> Style { Style::new().green() }
fn yellow() -> Style { Style::new().yellow() }
fn dim() -> Style { Style::new().dim() }

fn get_hermes_home() -> PathBuf {
    if let Ok(home) = std::env::var("HERMES_HOME") {
        PathBuf::from(home)
    } else if let Some(dir) = dirs::home_dir() {
        dir.join(".hermes")
    } else {
        PathBuf::from(".hermes")
    }
}

/// Migrate from another agent system.
pub fn cmd_claw(
    action: &str,
    source: Option<&str>,
    force: bool,
    _dry_run: bool,
    _preset: &str,
    _overwrite: bool,
    _migrate_secrets: bool,
    _yes: bool,
    _workspace_target: Option<&str>,
    _skill_conflict: &str,
) -> anyhow::Result<()> {
    println!();
    println!("{}", cyan().apply_to("◆ Claw Migration"));
    println!();

    match action {
        "migrate" => {
            match source {
                Some("claude-code" | "claude") => {
                    migrate_claude_code(force)?;
                }
                Some("chatgpt" | "openai") => {
                    println!("  {}", dim().apply_to("OpenAI ChatGPT migration not yet implemented."));
                }
                Some(src) => {
                    println!("  {} Unknown source: {src}", yellow().apply_to("⚠"));
                    println!("  Supported: claude-code, chatgpt");
                }
                None => {
                    println!("  {} Please specify --source (e.g. claude-code)", yellow().apply_to("⚠"));
                }
            }
        }
        "cleanup" => {
            println!("  {}", dim().apply_to("Cleaning up migration artifacts..."));
            // Remove temp files
            let temp_dir = get_hermes_home().join(".migration");
            if temp_dir.exists() {
                let _ = std::fs::remove_dir_all(&temp_dir);
                println!("  {} Migration temp files removed.", green().apply_to("✓"));
            }
            println!();
        }
        _ => {
            println!("  {}", dim().apply_to("Usage: hermes claw <migrate|cleanup> --source <source>"));
        }
    }

    Ok(())
}

fn migrate_claude_code(_force: bool) -> anyhow::Result<()> {
    println!("  Migrating from Claude Code...");
    println!();

    // Find Claude Code config
    let claude_home = match dirs::home_dir() {
        Some(h) => h.join(".claude"),
        None => {
            println!("  {} Could not find home directory.", yellow().apply_to("⚠"));
            return Ok(());
        }
    };

    if !claude_home.exists() {
        println!("  {} Claude Code config not found at ~/.claude", yellow().apply_to("⚠"));
        println!("  {}", dim().apply_to("Make sure Claude Code is installed and configured."));
        return Ok(());
    }

    println!("  Found Claude Code config at: {}", claude_home.display());
    println!();

    let hermes_home = get_hermes_home();
    std::fs::create_dir_all(&hermes_home)?;

    // Migrate settings
    let claude_settings = claude_home.join("settings.json");
    if claude_settings.exists() {
        println!("  Migrating settings...");
        if let Ok(content) = std::fs::read_to_string(&claude_settings) {
            if let Ok(settings) = serde_json::from_str::<serde_json::Value>(&content) {
                let mut config: serde_yaml::Value = serde_yaml::Value::Mapping(Default::default());

                // Map model
                if let Some(model) = settings.get("model").and_then(|v| v.as_str()) {
                    if let Some(map) = config.as_mapping_mut() {
                        map.insert(
                            serde_yaml::Value::String("model".to_string()),
                            serde_yaml::Value::String(model.to_string()),
                        );
                    }
                }

                let yaml = serde_yaml::to_string(&config)?;
                std::fs::write(hermes_home.join("config.yaml"), yaml)?;
                println!("  {} Settings migrated.", green().apply_to("✓"));
            }
        }
    }

    // Migrate memories
    let claude_memories = claude_home.join("projects");
    if claude_memories.exists() {
        println!("  Migrating memories...");
        let hermes_memories = hermes_home.join("memories");
        std::fs::create_dir_all(&hermes_memories)?;

        let mut migrated = 0;
        let mut failed = 0;
        match std::fs::read_dir(&claude_memories) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(e) => {
                            let src = e.path();
                            let dst = hermes_memories.join(e.file_name());
                            if src.is_file() {
                                match std::fs::copy(&src, &dst) {
                                    Ok(_) => migrated += 1,
                                    Err(e) => {
                                        println!("  {} Failed to copy: {}", yellow().apply_to("⚠"), e);
                                        failed += 1;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            println!("  {} Directory entry error: {}", yellow().apply_to("⚠"), e);
                            failed += 1;
                        }
                    }
                }
            }
            Err(e) => {
                println!("  {} Failed to read projects dir: {}", yellow().apply_to("⚠"), e);
            }
        }
        println!("  {} Memories migrated: {migrated} succeeded, {failed} failed.", green().apply_to("✓"));
    }

    println!();
    println!("  {} Migration complete.", green().apply_to("✓"));
    println!("  {}", dim().apply_to("Run `hermes claw cleanup` to remove migration artifacts."));
    println!();

    Ok(())
}
