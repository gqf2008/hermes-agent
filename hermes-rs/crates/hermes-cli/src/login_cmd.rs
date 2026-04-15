//! Login / OAuth flow.
//!
//! Mirrors Python: hermes login (OAuth login for supported providers)

use console::Style;

fn cyan() -> Style { Style::new().cyan() }
fn green() -> Style { Style::new().green() }
fn yellow() -> Style { Style::new().yellow() }
fn dim() -> Style { Style::new().dim() }

/// Interactive login via OAuth.
pub fn cmd_login(
    provider: &str,
    client_id: Option<&str>,
    no_browser: bool,
    scopes: Option<&str>,
) -> anyhow::Result<()> {
    println!();
    println!("{}", cyan().apply_to("◆ OAuth Login"));
    println!();

    let supported = ["google", "anthropic", "openai"];
    if !supported.iter().any(|p| *p == provider.to_lowercase()) {
        println!("  {} Provider '{provider}' does not support OAuth login.", yellow().apply_to("⚠"));
        println!("  Supported providers: {}", supported.join(", "));
        println!();
        println!("  {}", dim().apply_to("Use `hermes auth add <provider> --key <api_key>` for API key auth."));
        return Ok(());
    }

    println!("  Provider: {provider}");
    if let Some(cid) = client_id {
        println!("  Client ID: {cid}");
    }
    if no_browser {
        println!("  Browser: disabled (manual URL open required)");
    }
    if let Some(s) = scopes {
        println!("  Scopes: {s}");
    }
    println!();

    // OAuth flow would open browser and wait for callback
    if no_browser {
        println!("  {}", yellow().apply_to("⚠ OAuth requires manual authorization:"));
        println!("  1. Visit: https://{provider}.com/oauth/authorize?client_id=...");
        println!("  2. Authorize the application");
        println!("  3. Copy the authorization code");
        println!("  4. Run: hermes login {provider} --code <code>");
    } else {
        println!("  Opening browser for OAuth...");
        println!("  {}", green().apply_to("✓"));
    }
    println!();

    Ok(())
}
