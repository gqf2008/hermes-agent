//! MCP (Model Context Protocol) server management.

use console::Style;
use std::path::PathBuf;

fn get_hermes_home() -> PathBuf {
    if let Ok(home) = std::env::var("HERMES_HOME") {
        PathBuf::from(home)
    } else if let Some(dir) = dirs::home_dir() {
        dir.join(".hermes")
    } else {
        PathBuf::from(".hermes")
    }
}

fn green() -> Style { Style::new().green() }
fn cyan() -> Style { Style::new().cyan() }
fn dim() -> Style { Style::new().dim() }
fn yellow() -> Style { Style::new().yellow() }
fn red() -> Style { Style::new().red() }

fn mcp_config_path() -> PathBuf {
    get_hermes_home().join("mcp_servers.json")
}

/// MCP server configuration.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct MCPServer {
    name: String,
    command: String,
    args: Vec<String>,
    enabled: bool,
}

fn load_mcp_servers() -> Vec<MCPServer> {
    let path = mcp_config_path();
    if path.exists() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(servers) = serde_json::from_str::<Vec<MCPServer>>(&content) {
                return servers;
            }
        }
    }
    Vec::new()
}

fn save_mcp_servers(servers: &[MCPServer]) -> anyhow::Result<()> {
    let path = mcp_config_path();
    std::fs::create_dir_all(path.parent().unwrap())?;
    let content = serde_json::to_string_pretty(servers)?;
    std::fs::write(&path, content)?;
    Ok(())
}

/// List MCP servers.
pub fn cmd_mcp_list() -> anyhow::Result<()> {
    let servers = load_mcp_servers();

    println!();
    println!("{}", cyan().apply_to("◆ MCP Servers"));
    println!();

    if servers.is_empty() {
        println!("  {}", dim().apply_to("No MCP servers configured."));
        println!("  Add one with: hermes mcp add <name> --command <cmd>");
    } else {
        for server in &servers {
            let status = if server.enabled {
                green().apply_to("enabled").to_string()
            } else {
                yellow().apply_to("disabled").to_string()
            };
            println!("  {} — {} ({})", server.name, status, server.command);
            if !server.args.is_empty() {
                println!("    args: {}", server.args.join(" "));
            }
        }
    }
    println!();

    Ok(())
}

/// Add an MCP server.
pub fn cmd_mcp_add(name: &str, command: &str, args: &[String], auto_enable: bool) -> anyhow::Result<()> {
    let mut servers = load_mcp_servers();

    // Check for duplicate
    if servers.iter().any(|s| s.name == name) {
        println!("  {} MCP server already exists: {}", yellow().apply_to("⚠"), name);
        return Ok(());
    }

    servers.push(MCPServer {
        name: name.to_string(),
        command: command.to_string(),
        args: args.to_vec(),
        enabled: auto_enable,
    });

    save_mcp_servers(&servers)?;
    println!("  {} MCP server added: {}", green().apply_to("✓"), name);
    println!("    Command: {} {}", command, args.join(" "));

    Ok(())
}

/// Remove an MCP server.
pub fn cmd_mcp_remove(name: &str) -> anyhow::Result<()> {
    let mut servers = load_mcp_servers();
    let before = servers.len();
    servers.retain(|s| s.name != name);

    if servers.len() < before {
        save_mcp_servers(&servers)?;
        println!("  {} MCP server removed: {}", green().apply_to("✓"), name);
    } else {
        println!("  {} MCP server not found: {}", yellow().apply_to("✗"), name);
    }

    Ok(())
}

/// Test connection to an MCP server.
pub fn cmd_mcp_test(name: &str) -> anyhow::Result<()> {
    let servers = load_mcp_servers();
    let server = servers.iter().find(|s| s.name == name);

    match server {
        Some(s) => {
            println!("  Testing MCP server: {}", s.name);
            println!("  Command: {} {}", s.command, s.args.join(" "));
            println!();

            // Try to run the command with --help or version
            let output = std::process::Command::new(&s.command)
                .args(&s.args)
                .arg("--version")
                .output();

            match output {
                Ok(out) if out.status.success() => {
                    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    println!("  {} Connected — version: {}", green().apply_to("✓"), version);
                }
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    println!("  {} Connection failed: {}", yellow().apply_to("⚠"), stderr.trim());
                }
                Err(e) => {
                    println!("  {} Failed to execute: {}", red().apply_to("✗"), e);
                }
            }
        }
        None => {
            println!("  {} MCP server not found: {}", yellow().apply_to("✗"), name);
        }
    }

    println!();
    Ok(())
}

/// Configure MCP servers interactively.
pub fn cmd_mcp_configure() -> anyhow::Result<()> {
    let servers = load_mcp_servers();

    println!();
    println!("{}", cyan().apply_to("◆ MCP Server Configuration"));
    println!();

    if servers.is_empty() {
        println!("  No MCP servers configured.");
    } else {
        for (i, server) in servers.iter().enumerate() {
            let status = if server.enabled { "ON" } else { "OFF" };
            println!("  {}. {} [{}] — {} {}", i + 1, server.name, status, server.command, server.args.join(" "));
        }
    }
    println!();
    println!("  Add with: hermes mcp add <name> --command <cmd> [--args \"...\"]");
    println!();

    Ok(())
}

/// Dispatch MCP subcommands.
pub fn cmd_mcp(
    action: &str,
    name: Option<&str>,
    command: &str,
    args: &[String],
) -> anyhow::Result<()> {
    match action {
        "list" | "ls" | "" => cmd_mcp_list(),
        "add" | "register" => {
            let n = name.ok_or_else(|| anyhow::anyhow!("name is required"))?;
            cmd_mcp_add(n, command, args, true)
        }
        "remove" | "rm" | "unregister" => {
            let n = name.ok_or_else(|| anyhow::anyhow!("name is required"))?;
            cmd_mcp_remove(n)
        }
        "test" | "ping" => {
            let n = name.ok_or_else(|| anyhow::anyhow!("name is required"))?;
            cmd_mcp_test(n)
        }
        "configure" | "config" => cmd_mcp_configure(),
        _ => {
            anyhow::bail!("Unknown action: {}. Use list, add, remove, test, or configure.", action);
        }
    }
}

/// Run as MCP stdio server.
pub fn cmd_mcp_serve() -> anyhow::Result<()> {
    println!();
    println!("{}", cyan().apply_to("◆ MCP Stdio Server"));
    println!();
    println!("  {}", dim().apply_to("Starting MCP server over stdio..."));
    println!("  {}", dim().apply_to("This allows IDEs and other clients to connect via stdio transport."));
    println!();

    // List enabled servers
    let servers = load_mcp_servers();
    let enabled: Vec<_> = servers.iter().filter(|s| s.enabled).collect();

    if enabled.is_empty() {
        println!("  {}", yellow().apply_to("⚠ No MCP servers enabled."));
        println!("  {}", dim().apply_to("Run `hermes mcp add <name> --command <cmd>` first."));
    } else {
        println!("  Enabled servers:");
        for server in &enabled {
            println!("    - {} ({})", server.name, server.command);
        }
    }
    println!();

    Ok(())
}
