//! Configuration management commands for the Hermes CLI.
//!
//! Mirrors the Python `hermes config` subcommand.

use console::style;
use std::path::PathBuf;

/// Show current configuration.
pub fn cmd_config_show(verbose: bool) -> anyhow::Result<()> {
    // Load config from YAML
    let config_path = get_config_path();
    let env_path = get_env_path();

    println!("{}", style("Hermes Configuration").bold());
    println!("{}", "-".repeat(50));

    println!(
        "Config file: {}",
        style(config_path.display()).cyan()
    );
    println!(
        "Env file:    {}",
        style(env_path.display()).cyan()
    );
    println!(
        "HERMES_HOME: {}",
        style(get_hermes_home().display()).cyan()
    );
    println!();

    // Show loaded config values
    if let Ok(config) = load_config_yaml(&config_path) {
        print_config_section("Agent", &config, &["agent", "model", "provider", "quiet", "toolsets"]);
        print_config_section("Compression", &config, &["compression", "enabled", "target_tokens"]);
        print_config_section("Terminal", &config, &["terminal", "backend", "docker_image"]);
        if verbose {
            println!("\n{}", style("Full config (YAML):").bold());
            println!("{}", serde_yaml::to_string(&config).unwrap_or_default());
        }
    } else {
        println!("{}", style("No config file found. Run `hermes setup` to create one.").dim());
    }

    // Show environment variable status
    println!("\n{}", style("Environment Keys:").bold());
    show_env_status();

    Ok(())
}

/// Edit configuration file in default editor.
pub fn cmd_config_edit() -> anyhow::Result<()> {
    let config_path = get_config_path();

    if !config_path.exists() {
        println!(
            "{} No config file at {}. Run `hermes setup` first.",
            style("[!]").yellow(),
            style(config_path.display()).cyan()
        );
        return Ok(());
    }

    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| if cfg!(windows) { "notepad" } else { "vi" }.to_string());

    println!(
        "Opening {} with {}...",
        style(&config_path.display()).cyan(),
        style(&editor).cyan()
    );

    let status = std::process::Command::new(&editor)
        .arg(&config_path)
        .status();

    match status {
        Ok(s) if s.success() => println!("{} Config saved.", style("[OK]").green()),
        Ok(_) => println!("{}", style("Editor exited without saving.").yellow()),
        Err(e) => println!(
            "{} Failed to open editor: {}",
            style("[!]").red(),
            e
        ),
    }

    Ok(())
}

/// Get the configuration file path.
fn get_config_path() -> PathBuf {
    let home = get_hermes_home();
    home.join("config.yaml")
}

/// Get the environment file path.
fn get_env_path() -> PathBuf {
    let home = get_hermes_home();
    home.join(".env")
}

/// Get the Hermes home directory.
fn get_hermes_home() -> PathBuf {
    if let Ok(home) = std::env::var("HERMES_HOME") {
        PathBuf::from(home)
    } else if let Some(dir) = dirs::home_dir() {
        dir.join(".hermes")
    } else {
        PathBuf::from(".hermes")
    }
}

/// Load config YAML, returning empty map on failure.
fn load_config_yaml(path: &PathBuf) -> Result<serde_yaml::Value, ()> {
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(value) = serde_yaml::from_str(&content) {
                return Ok(value);
            }
        }
    }
    Err(())
}

/// Print a section of config values.
fn print_config_section(
    header: &str,
    config: &serde_yaml::Value,
    keys: &[&str],
) {
    if keys.is_empty() {
        return;
    }

    // Support nested paths like ["compression", "enabled"]
    // For simplicity, we just look up top-level keys here
    let mut found = false;
    let mut lines = Vec::new();

    for &key in keys {
        if let Some(value) = config.get(key) {
            if let Some(s) = value.as_str() {
                lines.push(format!("  {:20} {}", style(key).dim(), style(s).cyan()));
                found = true;
            } else if let Some(b) = value.as_bool() {
                lines.push(format!("  {:20} {}", style(key).dim(), style(b).cyan()));
                found = true;
            } else if let Some(n) = value.as_i64() {
                lines.push(format!("  {:20} {}", style(key).dim(), style(n).cyan()));
                found = true;
            } else {
                lines.push(format!("  {:20} {}", style(key).dim(), style(format!("{value:?}")).cyan()));
                found = true;
            }
        }
    }

    if found {
        println!("{}", style(header).bold());
        for line in lines {
            println!("{line}");
        }
        println!();
    }
}

/// Show status of known environment variables.
fn show_env_status() {
    let known_keys = [
        "OPENROUTER_API_KEY",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "GOOGLE_API_KEY",
        "DEEPSEEK_API_KEY",
        "NOUS_API_KEY",
        "FIRECRAWL_API_KEY",
        "EXA_API_KEY",
    ];

    for key in &known_keys {
        let status = if std::env::var(key).is_ok() {
            style("set").green()
        } else {
            style("not set").dim()
        };
        println!("  {:30} {}", style(key).dim(), status);
    }
}

/// Set a configuration value in the YAML file.
pub fn cmd_config_set(key: &str, value: &str) -> anyhow::Result<()> {
    let config_path = get_config_path();

    let mut config: serde_yaml::Value = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        serde_yaml::from_str(&content).unwrap_or(serde_yaml::Value::Mapping(Default::default()))
    } else {
        serde_yaml::Value::Mapping(Default::default())
    };

    if let Some(map) = config.as_mapping_mut() {
        // Support dotted key paths: agent.model
        let parts: Vec<&str> = key.split('.').collect();
        if parts.len() == 1 {
            map.insert(
                serde_yaml::Value::String(parts[0].to_string()),
                parse_yaml_value(value),
            );
        } else {
            // Navigate nested keys
            let mut current = map;
            for part in &parts[..parts.len() - 1] {
                let key_val = serde_yaml::Value::String(part.to_string());
                let entry = current
                    .entry(key_val)
                    .or_insert(serde_yaml::Value::Mapping(Default::default()));
                if let Some(mapping) = entry.as_mapping_mut() {
                    current = mapping;
                } else {
                    println!(
                        "{} Key path '{}' has a non-mapping intermediate value.",
                        style("[!]").red(),
                        key
                    );
                    return Ok(());
                }
            }
            let last_key = serde_yaml::Value::String(parts.last().unwrap().to_string());
            current.insert(last_key, parse_yaml_value(value));
        }
    }

    let yaml = serde_yaml::to_string(&config)?;
    std::fs::write(&config_path, yaml)?;
    println!(
        "{} Set {} = {}",
        style("[OK]").green(),
        style(key).cyan(),
        style(value).cyan()
    );
    Ok(())
}

/// Parse a string value into a YAML value.
fn parse_yaml_value(s: &str) -> serde_yaml::Value {
    // Try bool
    if s.eq_ignore_ascii_case("true") {
        return serde_yaml::Value::Bool(true);
    }
    if s.eq_ignore_ascii_case("false") {
        return serde_yaml::Value::Bool(false);
    }
    // Try number
    if let Ok(n) = s.parse::<i64>() {
        return serde_yaml::Value::Number(n.into());
    }
    // String
    serde_yaml::Value::String(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_yaml_value_bool() {
        assert_eq!(parse_yaml_value("true"), serde_yaml::Value::Bool(true));
        assert_eq!(parse_yaml_value("false"), serde_yaml::Value::Bool(false));
        assert_eq!(parse_yaml_value("TRUE"), serde_yaml::Value::Bool(true));
    }

    #[test]
    fn test_parse_yaml_value_number() {
        assert_eq!(parse_yaml_value("42"), serde_yaml::Value::Number(42.into()));
        assert_eq!(parse_yaml_value("0"), serde_yaml::Value::Number(0.into()));
    }

    #[test]
    fn test_parse_yaml_value_string() {
        assert_eq!(
            parse_yaml_value("hello"),
            serde_yaml::Value::String("hello".to_string())
        );
        assert_eq!(
            parse_yaml_value("anthropic/claude-opus-4-6"),
            serde_yaml::Value::String("anthropic/claude-opus-4-6".to_string())
        );
    }

    #[test]
    fn test_get_hermes_home_env() {
        std::env::set_var("HERMES_HOME", "/tmp/test_hermes");
        let path = get_hermes_home();
        assert_eq!(path, PathBuf::from("/tmp/test_hermes"));
        std::env::remove_var("HERMES_HOME");
    }
}
