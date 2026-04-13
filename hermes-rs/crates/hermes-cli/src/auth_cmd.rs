//! Authentication subcommands.
//!
//! Mirrors Python: hermes auth add/list/remove/reset, hermes login/logout

use std::path::PathBuf;

use console::Style;

/// Add a pooled credential.
pub fn cmd_auth_add(provider: &str, api_key: &str, label: Option<&str>) -> anyhow::Result<()> {
    let green = Style::new().green();
    let cyan = Style::new().cyan();

    let cred_path = credential_store_path();
    let mut creds = load_credentials(&cred_path).unwrap_or_default();

    let label_str = label.unwrap_or(provider).to_string();
    creds.push(CredentialEntry {
        provider: provider.to_string(),
        api_key: api_key.to_string(),
        label: label_str.clone(),
        exhausted: false,
    });

    save_credentials(&cred_path, &creds)?;

    println!();
    println!("{}", cyan.apply_to("◆ Credential Added"));
    println!("  {} Provider: {provider}", green.apply_to("✓"));
    println!("  Label:    {label_str}");
    println!();

    Ok(())
}

/// List pooled credentials.
pub fn cmd_auth_list() -> anyhow::Result<()> {
    let cyan = Style::new().cyan();
    let green = Style::new().green();
    let yellow = Style::new().yellow();
    let dim = Style::new().dim();

    let cred_path = credential_store_path();
    let creds = load_credentials(&cred_path).unwrap_or_default();

    println!();
    println!("{}", cyan.apply_to("◆ Pooled Credentials"));
    println!();

    if creds.is_empty() {
        println!("  {}", dim.apply_to("No credentials configured."));
        println!("  Add one with: hermes auth add <provider> --key <api_key>");
        println!();
        return Ok(());
    }

    for (i, cred) in creds.iter().enumerate() {
        let masked = mask_api_key(&cred.api_key);
        let status = if cred.exhausted {
            yellow.apply_to("exhausted").to_string()
        } else {
            green.apply_to("active").to_string()
        };
        println!("  {i:>2}. {}  [{}]  {masked}  {}", cred.provider, cred.label, status);
    }
    println!();
    println!("  Total: {} credential(s)", creds.len());
    println!();

    Ok(())
}

/// Remove a credential by index.
pub fn cmd_auth_remove(index: usize) -> anyhow::Result<()> {
    let green = Style::new().green();
    let yellow = Style::new().yellow();

    let cred_path = credential_store_path();
    let mut creds = load_credentials(&cred_path).unwrap_or_default();

    if index >= creds.len() {
        println!("  {} Index {index} out of range ({} credentials).", yellow.apply_to("✗"), creds.len());
    } else {
        let removed = creds.remove(index);
        save_credentials(&cred_path, &creds)?;
        println!("  {} Removed credential: {} ({})", green.apply_to("✓"), removed.provider, removed.label);
    }
    println!();

    Ok(())
}

/// Reset exhaustion status for a provider.
pub fn cmd_auth_reset(provider: &str) -> anyhow::Result<()> {
    let green = Style::new().green();
    let yellow = Style::new().yellow();

    let cred_path = credential_store_path();
    let mut creds = load_credentials(&cred_path).unwrap_or_default();

    let mut reset_count = 0;
    for cred in &mut creds {
        if cred.provider == provider && cred.exhausted {
            cred.exhausted = false;
            reset_count += 1;
        }
    }

    if reset_count > 0 {
        save_credentials(&cred_path, &creds)?;
        println!("  {} Reset {reset_count} credential(s) for '{provider}'.", green.apply_to("✓"));
    } else {
        println!("  {} No exhausted credentials found for '{provider}'.", yellow.apply_to("→"));
    }
    println!();

    Ok(())
}

/// Logout — clear stored credentials.
pub fn cmd_logout() -> anyhow::Result<()> {
    let green = Style::new().green();
    let yellow = Style::new().yellow();

    let cred_path = credential_store_path();
    if cred_path.exists() {
        std::fs::remove_file(&cred_path)?;
        println!("  {} Logged out — credentials cleared.", green.apply_to("✓"));
    } else {
        println!("  {} No credentials found.", yellow.apply_to("→"));
    }
    println!();

    Ok(())
}

/// Show auth status.
pub fn cmd_auth_status() -> anyhow::Result<()> {
    let cyan = Style::new().cyan();
    let green = Style::new().green();
    let yellow = Style::new().yellow();

    let cred_path = credential_store_path();
    let creds = load_credentials(&cred_path).unwrap_or_default();

    println!();
    println!("{}", cyan.apply_to("◆ Auth Status"));
    println!();

    if creds.is_empty() {
        println!("  {}", yellow.apply_to("No credentials configured."));
    } else {
        let active = creds.iter().filter(|c| !c.exhausted).count();
        let exhausted = creds.iter().filter(|c| c.exhausted).count();
        println!("  {} {} active, {} exhausted", green.apply_to("✓"), active, exhausted);
    }

    // Check env-based auth
    let env_providers = [
        ("OPENAI", "OPENAI_API_KEY"),
        ("ANTHROPIC", "ANTHROPIC_API_KEY"),
        ("OPENROUTER", "OPENROUTER_API_KEY"),
        ("GOOGLE", "GOOGLE_API_KEY"),
        ("NOUS", "NOUS_API_KEY"),
    ];

    println!();
    println!("  {}", cyan.apply_to("Environment Credentials:"));
    for (name, env_var) in &env_providers {
        if std::env::var(env_var).is_ok() {
            println!("    {} {name} ({env_var} set)", green.apply_to("✓"));
        }
    }
    println!();

    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CredentialEntry {
    provider: String,
    api_key: String,
    label: String,
    exhausted: bool,
}

fn credential_store_path() -> PathBuf {
    let hermes_home = hermes_core::get_hermes_home();
    hermes_home.join("credentials.json")
}

fn load_credentials(path: &PathBuf) -> Option<Vec<CredentialEntry>> {
    if !path.exists() {
        return None;
    }
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn save_credentials(path: &PathBuf, creds: &[CredentialEntry]) -> anyhow::Result<()> {
    let data = serde_json::to_string_pretty(creds)?;
    std::fs::write(path, data)?;
    Ok(())
}

fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        "****".to_string()
    } else {
        format!("{}****{}", &key[..4], &key[key.len() - 4..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_api_key() {
        assert_eq!(mask_api_key("sk-abc"), "****");
        assert_eq!(mask_api_key("sk-1234567890abcdef"), "sk-1****cdef");
    }

    #[test]
    fn test_credential_store_path() {
        let path = credential_store_path();
        assert!(path.to_string_lossy().contains("credentials.json"));
    }

    #[test]
    fn test_load_nonexistent() {
        assert!(load_credentials(&PathBuf::from("/nonexistent")).is_none());
    }
}
