//! .env file loader.
//!
//! Loads `.env` files with the same precedence logic as the Python version:
//! - `~/.hermes/.env` overrides stale shell-exported values
//! - project `.env` acts as a dev fallback (only fills missing values)

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::hermes_home::get_hermes_home;

/// Parse a .env file and return key-value pairs.
fn parse_env_file(path: &Path) -> HashMap<String, String> {
    let mut result = HashMap::new();
    if let Ok(content) = std::fs::read_to_string(path) {
        for line in content.lines() {
            let line = line.trim();
            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Parse KEY=VALUE
            if let Some(eq_idx) = line.find('=') {
                let key = line[..eq_idx].trim().to_string();
                let mut value = line[eq_idx + 1..].trim().to_string();
                // Strip surrounding quotes
                if (value.starts_with('"') && value.ends_with('"'))
                    || (value.starts_with('\'') && value.ends_with('\''))
                {
                    value = value[1..value.len() - 1].to_string();
                }
                if !key.is_empty() {
                    result.insert(key, value);
                }
            }
        }
    }
    result
}

/// Load the Hermes .env files.
///
/// Returns the list of loaded file paths.
///
/// Behavior:
/// - `HERMES_HOME/.env` overrides existing env vars
/// - project `.env` only fills missing vars (when user .env exists)
///   or overrides all (when no user .env exists)
pub fn load_hermes_dotenv(project_env: Option<&Path>) -> Vec<PathBuf> {
    let mut loaded = Vec::new();

    let hermes_home = get_hermes_home();
    let user_env = hermes_home.join(".env");

    let has_user_env = user_env.exists();

    if has_user_env {
        let vars = parse_env_file(&user_env);
        for (key, value) in vars {
            std::env::set_var(&key, &value);
        }
        loaded.push(user_env);
    }

    if let Some(project_path) = project_env {
        if project_path.exists() {
            let vars = parse_env_file(project_path);
            for (key, value) in vars {
                // Only set if not already set (user env takes precedence)
                if !has_user_env || std::env::var(&key).is_err() {
                    std::env::set_var(&key, &value);
                }
            }
            loaded.push(project_path.to_path_buf());
        }
    }

    loaded
}

/// Load a single .env file and set all variables (override mode).
pub fn load_dotenv_override(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    let vars = parse_env_file(path);
    for (key, value) in vars {
        std::env::set_var(&key, &value);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_parse_env_file() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_env_loader.env");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "# comment").unwrap();
        writeln!(file, "KEY1=value1").unwrap();
        writeln!(file, "KEY2=\"quoted value\"").unwrap();
        writeln!(file, "KEY3='single quoted'").unwrap();
        drop(file);

        let vars = parse_env_file(&path);
        assert_eq!(vars.get("KEY1"), Some(&"value1".to_string()));
        assert_eq!(vars.get("KEY2"), Some(&"quoted value".to_string()));
        assert_eq!(vars.get("KEY3"), Some(&"single quoted".to_string()));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_parse_skips_comments() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_env_comments.env");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "# this is a comment").unwrap();
        writeln!(file, "").unwrap();
        writeln!(file, "VALID=yes").unwrap();
        drop(file);

        let vars = parse_env_file(&path);
        assert_eq!(vars.len(), 1);
        assert_eq!(vars.get("VALID"), Some(&"yes".to_string()));

        let _ = std::fs::remove_file(&path);
    }
}
