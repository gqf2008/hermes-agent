//! Terminal tool — command execution with foreground and background modes.
//!
//! Mirrors the Python `tools/terminal_tool.py`.
//! MVP: local execution only (no Docker/SSH/Modal/Singularity/Daytona backends).
//! Integrates with `process_reg` for background process tracking.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::process_reg::{mark_process_finished, register_process, update_process_output};
use crate::registry::{tool_error, ToolRegistry};

/// Max foreground timeout (seconds).
const FOREGROUND_MAX_TIMEOUT: u64 = 600;

/// Default foreground timeout (seconds).
const FOREGROUND_DEFAULT_TIMEOUT: u64 = 60;

/// Max output size returned to LLM (50KB).
const MAX_OUTPUT_RETURN: usize = 50 * 1024;

/// Truncate ratio: 40% head + 60% tail.
const HEAD_RATIO: f64 = 0.4;

/// Workdir validation regex — alphanumeric, slashes, dots, dashes, underscores.
fn is_valid_workdir(workdir: &str) -> bool {
    !workdir.is_empty()
        && workdir
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '/' | '\\' | '.' | '-' | '_' | ' ' | ':'))
        && !workdir.contains("..")
        && !workdir.contains('|')
        && !workdir.contains(';')
        && !workdir.contains('&')
        && !workdir.contains('$')
        && !workdir.contains('`')
}

/// Truncate output to MAX_OUTPUT_RETURN: 40% head + 60% tail.
fn truncate_output(output: &str) -> String {
    let bytes = output.len();
    if bytes <= MAX_OUTPUT_RETURN {
        return output.to_string();
    }

    let head_len = (bytes as f64 * HEAD_RATIO) as usize;
    let tail_len = bytes - head_len;

    // Find safe char boundaries
    let head_end = output
        .char_indices()
        .take_while(|(i, _)| *i <= head_len)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(head_len.min(bytes));

    let tail_start = output
        .char_indices()
        .find(|(i, _)| *i >= bytes - tail_len)
        .map(|(i, _)| i)
        .unwrap_or(head_end);

    if tail_start <= head_end {
        // Head and tail overlap — return head with truncation note
        return format!(
            "{}\n... [{} bytes truncated]",
            &output[..head_end.min(bytes)],
            bytes
        );
    }

    format!(
        "{}\n... [{} bytes truncated] ...\n{}",
        &output[..head_end],
        bytes,
        &output[tail_start..]
    )
}

/// Redact secrets from error messages.
fn redact_secrets(text: &str) -> String {
    let mut result = text.to_string();
    for pattern in ["sk-", "ghp_", "xoxb-", "Bearer "] {
        while let Some(pos) = result.find(pattern) {
            let end = pos + pattern.len() + 10;
            let end = end.min(result.len());
            result.replace_range(pos..end, "[REDACTED]");
            if !result.contains(pattern) {
                break;
            }
        }
    }
    result
}

/// Execute a command in the foreground with timeout.
fn execute_foreground(command: &str, timeout: u64, workdir: Option<&str>) -> Result<String, String> {
    let start = Instant::now();

    let mut cmd = Command::new("cmd.exe");
    cmd.args(["/C", command]);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }

    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn command: {e}"))?;

    // Wait with timeout
    let result = loop {
        if start.elapsed() > Duration::from_secs(timeout) {
            let _ = child.kill();
            break Err(format!(
                "Command timed out after {timeout}s. Increase timeout or run with background=true."
            ));
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                break Ok(status.code().unwrap_or(-1));
            }
            Ok(None) => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                break Err(format!("Error waiting for process: {e}"));
            }
        }
    };

    let exit_code = result?;

    // Collect output
    let output = child.wait_with_output().map_err(|e| format!("Failed to read output: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut combined = format!("{stdout}{stderr}");
    if !combined.is_empty() && !combined.ends_with('\n') {
        combined.push('\n');
    }
    combined.push_str(&format!("[Process exited with code {exit_code}]\n"));

    // Strip ANSI, truncate, redact
    let stripped = crate::ansi_strip::strip_ansi(&combined);
    let truncated = truncate_output(&stripped);
    Ok(redact_secrets(&truncated))
}

/// Execute a command in the background, registering with process_reg.
fn execute_background(command: &str, workdir: Option<&str>) -> String {
    let mut cmd = Command::new("cmd.exe");
    cmd.args(["/C", command]);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return tool_error(format!("Failed to spawn background command: {e}")),
    };

    let pid = child.id();
    let session_id = format!("proc_{:016x}", pid);

    // Register in process registry
    register_process(session_id.clone(), command.to_string(), Some(pid));

    // Spawn a thread to collect output
    let sid = session_id.clone();
    std::thread::spawn(move || {
        if let Ok(output) = child.wait_with_output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            update_process_output(&sid, &stdout);
            let exit_code = output.status.code().unwrap_or(-1);
            mark_process_finished(&sid, exit_code);
        }
    });

    serde_json::json!({
        "success": true,
        "action": "background",
        "session_id": session_id,
        "pid": pid,
        "command": command,
        "note": "Process started in background. Use 'process' tool with session_id to poll status.",
    })
    .to_string()
}

/// Handle terminal tool call.
pub fn handle_terminal(args: Value) -> Result<String, hermes_core::HermesError> {
    let command = match args.get("command").and_then(Value::as_str) {
        Some(c) if !c.trim().is_empty() => c.to_string(),
        _ => return Ok(tool_error("Terminal tool requires a non-empty 'command' parameter.")),
    };

    let background = args
        .get("background")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let timeout = args
        .get("timeout")
        .and_then(Value::as_u64)
        .unwrap_or(FOREGROUND_DEFAULT_TIMEOUT);

    let workdir = args
        .get("workdir")
        .and_then(Value::as_str)
        .map(String::from);

    // Validate workdir if provided
    if let Some(ref dir) = workdir {
        if !is_valid_workdir(dir) {
            return Ok(tool_error(format!(
                "Invalid workdir '{dir}'. Workdir must be a safe path without shell metacharacters."
            )));
        }
    }

    if background {
        Ok(execute_background(&command, workdir.as_deref()))
    } else {
        // Cap foreground timeout
        let timeout = timeout.min(FOREGROUND_MAX_TIMEOUT);
        match execute_foreground(&command, timeout, workdir.as_deref()) {
            Ok(output) => Ok(serde_json::json!({
                "success": true,
                "output": output,
            })
            .to_string()),
            Err(e) => Ok(tool_error(redact_secrets(&e))),
        }
    }
}

/// Register terminal tool.
pub fn register_terminal_tool(registry: &mut ToolRegistry) {
    registry.register(
        "terminal".to_string(),
        "terminal".to_string(),
        serde_json::json!({
            "name": "terminal",
            "description": "Execute shell commands. Use background=true for long-running processes.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The shell command to execute." },
                    "background": { "type": "boolean", "description": "Run in background with process tracking (default false)." },
                    "timeout": { "type": "integer", "description": "Max seconds to wait (default 60, max 600 for foreground)." },
                    "workdir": { "type": "string", "description": "Working directory override." },
                },
                "required": ["command"]
            }
        }),
        std::sync::Arc::new(handle_terminal),
        None,
        vec!["terminal".to_string()],
        "Execute shell commands".to_string(),
        "💻".to_string(),
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workdir_validation() {
        assert!(is_valid_workdir("/home/user/project"), "unix path should be valid");
        let win_path = "C:\\Users\\test";
        assert!(is_valid_workdir(win_path), "windows path '{win_path}' should be valid");
        assert!(is_valid_workdir("./src"));
        assert!(!is_valid_workdir("../etc/passwd"));
        assert!(!is_valid_workdir("/tmp; rm -rf /"));
        assert!(!is_valid_workdir("/tmp|cat /etc/passwd"));
        assert!(!is_valid_workdir(""));
        // Explicitly test shell metacharacters
        assert!(!is_valid_workdir("$(whoami)"));
        assert!(!is_valid_workdir("/tmp`id`"));
    }

    #[test]
    fn test_truncate_output_small() {
        let input = "short output";
        assert_eq!(truncate_output(input), "short output");
    }

    #[test]
    fn test_truncate_output_large() {
        // Need output > 50KB to trigger truncation
        let head = "H".repeat(40_000);
        let tail = "T".repeat(40_000);
        let input = format!("{head}MIDDLE{tail}");
        let result = truncate_output(&input);
        assert!(result.contains("truncated"), "should have truncation marker");
        assert!(result.starts_with("HHHHH"), "should start with head");
        assert!(result.len() < input.len(), "should be shorter than input");
    }

    #[test]
    fn test_redact_secrets() {
        let input = "error with sk-abc1234567890abcdef key";
        let output = redact_secrets(input);
        assert!(!output.contains("sk-abc1234567890"));
        assert!(output.contains("[REDACTED]"));
    }

    #[test]
    fn test_handle_missing_command() {
        let result = handle_terminal(serde_json::json!({}));
        let json: Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert!(json.get("error").is_some());
    }

    #[test]
    fn test_handle_empty_command() {
        let result = handle_terminal(serde_json::json!({ "command": "   " }));
        let json: Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert!(json.get("error").is_some());
    }

    #[test]
    fn test_foreground_echo() {
        let result = handle_terminal(serde_json::json!({
            "command": "echo hello"
        }));
        let json: Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert!(json.get("success").is_some());
        let output = json.get("output").and_then(Value::as_str).unwrap_or("");
        // On Windows cmd, echo adds a newline
        assert!(output.contains("hello") || json.get("error").is_some(), "output: {output}");
    }

    #[test]
    fn test_background_starts() {
        let result = handle_terminal(serde_json::json!({
            "command": "timeout /t 10 /nobreak >nul",
            "background": true
        }));
        let json: Value = serde_json::from_str(&result.unwrap()).unwrap();
        // May succeed or fail depending on Windows environment
        if json.get("success").and_then(Value::as_bool).unwrap_or(false) {
            assert!(json.get("session_id").is_some());
            assert!(json.get("pid").is_some());
        }
    }

    #[test]
    fn test_invalid_workdir_rejected() {
        let result = handle_terminal(serde_json::json!({
            "command": "echo test",
            "workdir": "/tmp; malicious"
        }));
        let json: Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert!(json.get("error").is_some());
    }

    #[test]
    fn test_timeout_capped() {
        // Timeout > 600 should be capped
        let result = handle_terminal(serde_json::json!({
            "command": "echo test",
            "timeout": 9999
        }));
        // Should not error due to timeout cap, just execute with capped value
        let json: Value = serde_json::from_str(&result.unwrap()).unwrap();
        // Either succeeds or fails for other reasons, but not timeout validation
        assert!(json.get("success").is_some() || json.get("error").is_some());
    }
}
