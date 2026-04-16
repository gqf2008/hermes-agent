//! Authentication subcommands.
//!
//! Mirrors Python: hermes auth add/list/remove/reset, hermes login/logout

use std::path::PathBuf;

use console::Style;

/// Add a pooled credential.
pub fn cmd_auth_add(
    provider: &str,
    auth_type: &str,
    key: Option<&str>,
    label: Option<&str>,
    client_id: Option<&str>,
    no_browser: bool,
    _portal_url: Option<&str>,
    _inference_url: Option<&str>,
    _scope: Option<&str>,
    _timeout: Option<f64>,
    _insecure: bool,
    _ca_bundle: Option<&str>,
) -> anyhow::Result<()> {
    let green = Style::new().green();
    let cyan = Style::new().cyan();
    let yellow = Style::new().yellow();

    let cred_path = credential_store_path();
    let mut creds = load_credentials(&cred_path).unwrap_or_default();

    // For OAuth types, we need client_id
    if auth_type == "oauth" && client_id.is_none() {
        println!("  {} OAuth auth type requires --client-id.", yellow.apply_to("⚠"));
        return Ok(());
    }

    // For API key type, require a key
    if auth_type == "api-key" && key.is_none() {
        println!("  {} API key auth type requires --key.", yellow.apply_to("⚠"));
        return Ok(());
    }

    let label_str = label.unwrap_or(provider).to_string();
    let api_key = key.unwrap_or("").to_string();
    creds.push(CredentialEntry {
        provider: provider.to_string(),
        api_key,
        label: label_str.clone(),
        exhausted: false,
    });

    save_credentials(&cred_path, &creds)?;

    println!();
    println!("{}", cyan.apply_to("◆ Credential Added"));
    println!("  {} Provider: {provider}", green.apply_to("✓"));
    println!("  Type:       {auth_type}");
    println!("  Label:      {label_str}");
    if no_browser {
        println!("  Browser:    disabled");
    }
    println!();

    Ok(())
}

/// List pooled credentials.
pub fn cmd_auth_list(provider_filter: Option<&str>) -> anyhow::Result<()> {
    let cyan = Style::new().cyan();
    let green = Style::new().green();
    let yellow = Style::new().yellow();
    let dim = Style::new().dim();

    let cred_path = credential_store_path();
    let creds = load_credentials(&cred_path).unwrap_or_default();

    let filtered: Vec<&CredentialEntry> = match provider_filter {
        Some(p) => creds.iter().filter(|c| c.provider == p).collect(),
        None => creds.iter().collect(),
    };

    println!();
    println!("{}", cyan.apply_to("◆ Pooled Credentials"));
    if let Some(p) = provider_filter {
        println!("  Filter: {p}");
    }
    println!();

    if filtered.is_empty() {
        println!("  {}", dim.apply_to("No credentials configured."));
        if provider_filter.is_some() {
            println!("  Try: hermes auth list (no filter) to see all credentials.");
        } else {
            println!("  Add one with: hermes auth add <provider> --key <api_key>");
        }
        println!();
        return Ok(());
    }

    for (i, cred) in filtered.iter().enumerate() {
        let masked = mask_api_key(&cred.api_key);
        let status = if cred.exhausted {
            yellow.apply_to("exhausted").to_string()
        } else {
            green.apply_to("active").to_string()
        };
        println!("  {i:>2}. {}  [{}]  {masked}  {}", cred.provider, cred.label, status);
    }
    println!();
    println!("  Total: {} credential(s)", filtered.len());
    println!();

    Ok(())
}

/// Remove a credential by provider + target (index, id, or label).
pub fn cmd_auth_remove(provider: &str, target: &str) -> anyhow::Result<()> {
    let green = Style::new().green();
    let yellow = Style::new().yellow();

    let cred_path = credential_store_path();
    let mut creds = load_credentials(&cred_path).unwrap_or_default();

    // Try to parse target as index first
    if let Ok(index) = target.parse::<usize>() {
        // Filter by provider first, then index within that provider
        let provider_creds: Vec<(usize, &CredentialEntry)> = creds.iter()
            .enumerate()
            .filter(|(_, c)| c.provider == provider)
            .collect();

        if index >= provider_creds.len() {
            println!("  {} Index {index} out of range for provider '{provider}' ({} credentials).", yellow.apply_to("✗"), provider_creds.len());
        } else {
            let (original_idx, removed) = provider_creds[index];
            let removed_provider = removed.provider.clone();
            let removed_label = removed.label.clone();
            creds.remove(original_idx);
            save_credentials(&cred_path, &creds)?;
            println!("  {} Removed credential: {} ({})", green.apply_to("✓"), removed_provider, removed_label);
        }
    } else {
        // Try to match by label
        let before = creds.len();
        creds.retain(|c| !(c.provider == provider && (c.label == target || c.api_key == target)));
        if creds.len() < before {
            save_credentials(&cred_path, &creds)?;
            println!("  {} Removed credential: {} ({})", green.apply_to("✓"), provider, target);
        } else {
            println!("  {} No credential matching target '{}' for provider '{}'.", yellow.apply_to("✗"), target, provider);
        }
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
pub fn cmd_logout(provider: Option<&str>) -> anyhow::Result<()> {
    let green = Style::new().green();
    let yellow = Style::new().yellow();

    let cred_path = credential_store_path();

    match provider {
        Some(p) => {
            let mut creds = load_credentials(&cred_path).unwrap_or_default();
            let before = creds.len();
            creds.retain(|c| c.provider != p);
            let removed = before - creds.len();
            if removed > 0 {
                save_credentials(&cred_path, &creds)?;
                println!("  {} Logged out from '{p}' — {removed} credential(s) removed.", green.apply_to("✓"));
            } else {
                println!("  {} No credentials found for '{p}'.", yellow.apply_to("→"));
            }
        }
        None => {
            if cred_path.exists() {
                std::fs::remove_file(&cred_path)?;
                println!("  {} Logged out — all credentials cleared.", green.apply_to("✓"));
            } else {
                println!("  {} No credentials found.", yellow.apply_to("→"));
            }
        }
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
