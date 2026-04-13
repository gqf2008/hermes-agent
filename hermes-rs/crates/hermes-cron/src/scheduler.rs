//! Cron scheduler — tick loop, file locking, job execution.
//!
//! Mirrors the Python `cron/scheduler.py`.

use std::path::Path;
use std::sync::Arc;

use hermes_core::{HermesError, Result};

use crate::jobs::{JobStore, save_job_output};

/// Run the scheduler tick loop.
///
/// This is the main entry point for the cron scheduler.
/// It runs a continuous loop, checking for due jobs and executing them.
///
/// # Arguments
/// * `verbose` — Enable verbose logging
/// * `loop_forever` — If true, run continuously; if false, run once and exit
pub async fn run_scheduler(verbose: bool, loop_forever: bool) -> Result<()> {
    let mut store = JobStore::new()?;

    if verbose {
        tracing::info!(
            "Cron scheduler started ({} jobs loaded)",
            store.list(true).len()
        );
    }

    loop {
        let tick_result = tick(&mut store, verbose).await;

        if verbose && tick_result > 0 {
            tracing::info!("Tick: {tick_result} job(s) executed");
        }

        if !loop_forever {
            break;
        }

        // Sleep for 30 seconds before next tick
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }

    Ok(())
}

/// Execute a single scheduler tick.
///
/// 1. Acquire file-based lock (skip if already running)
/// 2. Find due jobs
/// 3. Execute each due job
/// 4. Save output
/// 5. Mark job as run
async fn tick(store: &mut JobStore, _verbose: bool) -> usize {
    // Acquire lock
    let lock_path = get_lock_path();
    if !acquire_lock(&lock_path) {
        tracing::debug!("Scheduler lock is held, skipping tick");
        return 0;
    }

    let due_jobs = store.get_due_jobs();
    // Collect job data to avoid borrow conflicts
    let job_ids: Vec<String> = due_jobs.iter().map(|j| j.id.clone()).collect();
    let count = job_ids.len();

    for job_id in job_ids {
        // Advance next_run BEFORE execution (crash safety)
        let _ = store.advance_next_run(&job_id);

        // Get job reference for execution
        let job = store.get(&job_id).cloned();
        let Some(job) = job else { continue };

        // Execute the job
        let result = run_job(&job).await;

        // Save output
        let (success, output, error) = match result {
            Ok((ok, out, err)) => (ok, out, err),
            Err(e) => (false, format!("Error: {e}"), Some(e.to_string())),
        };

        // Save job output
        if !output.is_empty() {
            let _ = save_job_output(&job_id, &output);
        }

        // Check for [SILENT] marker
        let delivery_error = if output.contains("[SILENT]") {
            Some("silent".to_string())
        } else {
            None
        };

        // Mark job as run
        let _ = store.mark_run(&job_id, success, error.as_deref(), delivery_error.as_deref());
    }

    // Release lock
    release_lock(&lock_path);

    count
}

/// Execute a single cron job.
///
/// Returns (success, output, error).
async fn run_job(job: &crate::jobs::CronJob) -> Result<(bool, String, Option<String>)> {
    tracing::info!("Running cron job: {} ({})", job.name, job.id);

    let prompt = if let Some(ref script) = job.script {
        // Run pre-script and inject output as context
        match run_job_script(script) {
            Ok(script_output) => {
                format!(
                    "Script output:\n{script_output}\n\n---\n\n{}\n\nRun the task described above and return a complete result.",
                    job.prompt
                )
            }
            Err(e) => {
                return Ok((false, String::new(), Some(format!("Script failed: {e}"))));
            }
        }
    } else {
        job.prompt.clone()
    };

    // Build agent config
    let model = job.model.clone().unwrap_or_else(|| "anthropic/claude-opus-4.6".to_string());
    let config = hermes_agent_engine::agent::AgentConfig {
        model,
        provider: job.provider.clone(),
        base_url: job.base_url.clone(),
        max_iterations: 50,
        skip_context_files: true,
        platform: Some("cron".to_string()),
        ..hermes_agent_engine::agent::AgentConfig::default()
    };

    // Build tool registry
    let mut registry = hermes_tools::registry::ToolRegistry::new();
    hermes_tools::register_all_tools(&mut registry);

    let mut agent = hermes_agent_engine::AIAgent::new(config, Arc::new(registry))
        .map_err(|e| HermesError::new(hermes_core::errors::ErrorCategory::InternalError, e.to_string()))?;

    // Run with timeout
    let turn_result = tokio::time::timeout(
        std::time::Duration::from_secs(600), // 10 min default timeout
        agent.run_conversation(&prompt, None, None),
    )
    .await;

    match turn_result {
        Ok(result) => {
            let output = if result.response.is_empty() {
                // Get last assistant message
                result
                    .messages
                    .iter()
                    .rev()
                    .find_map(|msg| {
                        if msg.get("role").and_then(|v| v.as_str()) == Some("assistant") {
                            msg.get("content").and_then(|v| v.as_str()).map(String::from)
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default()
            } else {
                result.response
            };

            let success = result.exit_reason == "completed";
            let error = if success {
                None
            } else {
                Some(format!("Exit: {}", result.exit_reason))
            };

            Ok((success, output, error))
        }
        Err(_) => Ok((
            false,
            String::new(),
            Some("Job timed out (600s)".to_string()),
        )),
    }
}

/// Run a pre-job script and return its stdout.
fn run_job_script(script_path: &str) -> Result<String> {
    let home = hermes_core::get_hermes_home();
    let scripts_dir = home.join("scripts");

    // Validate script is within scripts directory (path traversal guard)
    let full_path = std::path::Path::new(&script_path);
    let canonical = full_path.canonicalize().map_err(|e| {
        HermesError::new(
            hermes_core::errors::ErrorCategory::InternalError,
            format!("Script not found: {e}"),
        )
    })?;

    if !canonical.starts_with(&scripts_dir) {
        return Err(HermesError::new(
            hermes_core::errors::ErrorCategory::InternalError,
            "Script path traversal detected".to_string(),
        ));
    }

    // Execute the script
    let output = std::process::Command::new("bash")
        .arg(&canonical)
        .output()
        .map_err(|e| {
            HermesError::new(
                hermes_core::errors::ErrorCategory::InternalError,
                format!("Failed to execute script: {e}"),
            )
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Redact sensitive text from stdout
    let sanitized = redact_sensitive_text(&stdout);

    let result = if output.status.success() {
        sanitized
    } else {
        format!("Script failed:\n{sanitized}\n\nStderr:\n{stderr}")
    };

    Ok(result)
}

/// Redact sensitive information from script output.
fn redact_sensitive_text(text: &str) -> String {
    let mut result = text.to_string();

    // Redact API key patterns
    for pattern in &["sk-", "anthropic:", "Bearer "] {
        if let Some(pos) = result.find(pattern) {
            let key_start = pos + pattern.len();
            let key_end = result[key_start..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                .map(|i| key_start + i)
                .unwrap_or(result.len())
                .min(key_start + 50);

            if key_end > key_start {
                result.replace_range(key_start..key_end, "[REDACTED]");
            }
        }
    }

    result
}

/// Get the lock file path.
fn get_lock_path() -> std::path::PathBuf {
    let home = hermes_core::get_hermes_home();
    home.join("cron").join(".tick.lock")
}

/// Acquire a file-based lock.
///
/// Returns false if the lock is already held.
fn acquire_lock(path: &Path) -> bool {
    // Try to create the lock file exclusively
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(file) => {
            // Write PID to lock file
            let pid = std::process::id();
            let _ = std::io::Write::write_all(
                &mut std::io::BufWriter::new(file),
                format!("{pid}").as_bytes(),
            );
            true
        }
        Err(_) => false,
    }
}

/// Release a file-based lock.
fn release_lock(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Trigger a job immediately (one-shot execution).
pub async fn trigger_job(store: &mut JobStore, job_id: &str) -> Result<(bool, String)> {
    let job = store.get(job_id).ok_or_else(|| {
        HermesError::new(
            hermes_core::errors::ErrorCategory::InternalError,
            format!("Job not found: {job_id}"),
        )
    })?;

    let result = run_job(job).await?;
    let (success, output, error) = result;

    // Save output
    if !output.is_empty() {
        let _ = save_job_output(job_id, &output);
    }

    // Mark as run
    let _ = store.mark_run(job_id, success, error.as_deref(), None);

    Ok((success, output))
}

/// List all saved outputs for a job.
pub fn list_job_outputs(job_id: &str) -> Result<Vec<std::path::PathBuf>> {
    let dir = crate::jobs::get_output_dir(job_id);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| {
            HermesError::new(
                hermes_core::errors::ErrorCategory::InternalError,
                format!("Failed to read output directory: {e}"),
            )
        })?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "md"))
        .collect();

    files.sort();
    Ok(files)
}

/// Read a job's output file.
pub fn read_job_output(job_id: &str, filename: &str) -> Result<String> {
    let dir = crate::jobs::get_output_dir(job_id);
    let path = dir.join(filename);

    std::fs::read_to_string(&path).map_err(|e| {
        HermesError::new(
            hermes_core::errors::ErrorCategory::InternalError,
            format!("Failed to read output file: {e}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_sensitive_text() {
        let input = "Here is my key: sk-1234567890abcdef and more text";
        let output = redact_sensitive_text(input);
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains("sk-1234567890abcdef"));
    }

    #[test]
    fn test_redact_bearer_token() {
        let input = "Authorization: Bearer secret_token_123 end";
        let output = redact_sensitive_text(input);
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains("secret_token_123"));
    }

    #[test]
    fn test_acquire_and_release_lock() {
        let dir = std::env::temp_dir();
        let lock_path = dir.join("test_cron.lock");
        let _ = std::fs::remove_file(&lock_path);

        // First acquire should succeed
        assert!(acquire_lock(&lock_path));
        assert!(lock_path.exists());

        // Second acquire should fail
        assert!(!acquire_lock(&lock_path));

        // Release and re-acquire should succeed
        release_lock(&lock_path);
        assert!(acquire_lock(&lock_path));

        // Cleanup
        release_lock(&lock_path);
    }
}
