//! Model management TUI.
//!
//! Mirrors Python: hermes model (interactive TUI for selecting/viewing models)

use console::Style;

fn cyan() -> Style { Style::new().cyan() }
fn dim() -> Style { Style::new().dim() }
fn green() -> Style { Style::new().green() }

/// Interactive model selection and management.
pub fn cmd_model() -> anyhow::Result<()> {
    println!();
    println!("{}", cyan().apply_to("◆ Model Management"));
    println!();
    println!("  {}", dim().apply_to("Available providers and models:"));
    println!();

    // List known providers
    let providers = [
        ("OpenRouter", "openrouter/", "router.openai.com"),
        ("OpenAI", "openai/", "api.openai.com"),
        ("Anthropic", "anthropic/", "api.anthropic.com"),
        ("Google", "google/", "generativelanguage.googleapis.com"),
        ("DeepSeek", "deepseek/", "api.deepseek.com"),
        ("Nous", "nous/", "api.nousresearch.com"),
    ];

    println!("  {}", green().apply_to("Providers:"));
    println!();
    for (name, prefix, host) in &providers {
        println!("    {name:15} {prefix:20} {host}");
    }

    println!();
    println!("  {}", dim().apply_to("Configure with: hermes config set model.provider <provider>"));
    println!("  {}", dim().apply_to("Configure with: hermes config set model.name <model>"));
    println!();

    Ok(())
}

/// Show available models list.
pub fn cmd_model_list() -> anyhow::Result<()> {
    println!();
    println!("{}", cyan().apply_to("◆ Available Models"));
    println!();

    // Common models
    let models = [
        ("anthropic", "claude-sonnet-4-6", "Anthropic Claude Sonnet 4.6"),
        ("anthropic", "claude-opus-4-6", "Anthropic Claude Opus 4.6"),
        ("anthropic", "claude-haiku-4-5", "Anthropic Claude Haiku 4.5"),
        ("openai", "gpt-4o", "OpenAI GPT-4o"),
        ("openai", "gpt-4o-mini", "OpenAI GPT-4o Mini"),
        ("openai", "o1", "OpenAI o1"),
        ("openai", "o3-mini", "OpenAI o3 Mini"),
        ("google", "gemini-2.5-pro", "Google Gemini 2.5 Pro"),
        ("deepseek", "deepseek-chat", "DeepSeek Chat V3"),
        ("nous", "hermes-3", "Nous Hermes 3"),
    ];

    println!("  {:15} {:25} Display Name", "Provider", "Model ID");
    println!("  {}", "-".repeat(60));
    for (provider, model_id, name) in &models {
        println!("  {:15} {:25} {}", provider, model_id, name);
    }
    println!();

    Ok(())
}
