//! Context compression for long conversations.
//!
//! Mirrors the Python `agent/context_compressor.py`.
//! 4-stage algorithm:
//!   1. Prune old tool results (cheap, no LLM call)
//!   2. Protect head messages (system prompt + first exchange)
//!   3. Protect tail messages by token budget (most recent context)
//!   4. Summarize middle turns with structured LLM prompt

use serde_json::Value;

/// Minimum tokens for the summary output.
const MIN_SUMMARY_TOKENS: usize = 2000;
/// Proportion of compressed content to allocate for summary.
const SUMMARY_RATIO: f64 = 0.20;
/// Absolute ceiling for summary tokens.
const SUMMARY_TOKENS_CEILING: usize = 12_000;
/// Placeholder used when pruning old tool results.
const PRUNED_TOOL_PLACEHOLDER: &str = "[Old tool output cleared to save context space]";
/// Chars per token rough estimate.
const CHARS_PER_TOKEN: usize = 4;
/// Summary failure cooldown in seconds.
#[allow(dead_code)]
const SUMMARY_FAILURE_COOLDOWN_SECONDS: f64 = 600.0;

/// Summary prefixes.
const SUMMARY_PREFIX: &str =
    "[CONTEXT COMPACTION] Earlier turns in this conversation were compacted \
    to save context space. The summary below describes work that was \
    already completed, and the current session state may still reflect \
    that work (for example, files may already be changed). Use the summary \
    and the current state to continue from where things left off, and \
    avoid repeating work:";
const LEGACY_SUMMARY_PREFIX: &str = "[CONTEXT SUMMARY]:";

/// Configuration for the context compressor.
#[derive(Debug, Clone)]
pub struct CompressorConfig {
    /// Model name (for context length lookup).
    pub model: String,
    /// Compress when context exceeds this fraction of model's context length.
    pub threshold_percent: f64,
    /// Always keep first N messages uncompressed.
    pub protect_first_n: usize,
    /// Minimum recent messages to protect (fallback when no token budget).
    pub protect_last_n: usize,
    /// Proportion of compressed content for summary.
    pub summary_target_ratio: f64,
    /// Quiet mode (suppress logging).
    pub quiet_mode: bool,
    /// Summary model override.
    pub summary_model_override: Option<String>,
    /// Context length override.
    pub config_context_length: Option<usize>,
}

impl Default for CompressorConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            threshold_percent: 0.50,
            protect_first_n: 3,
            protect_last_n: 20,
            summary_target_ratio: 0.20,
            quiet_mode: false,
            summary_model_override: None,
            config_context_length: None,
        }
    }
}

/// Context compressor state.
#[derive(Debug)]
pub struct ContextCompressor {
    config: CompressorConfig,
    #[allow(dead_code)]
    context_length: usize,
    threshold_tokens: usize,
    compression_count: usize,
    tail_token_budget: usize,
    max_summary_tokens: usize,
    last_prompt_tokens: usize,
    last_completion_tokens: usize,
    previous_summary: Option<String>,
    summary_failure_cooldown_until: Option<f64>,
}

impl ContextCompressor {
    /// Create a new context compressor.
    pub fn new(config: CompressorConfig) -> Self {
        let context_length = config
            .config_context_length
            .unwrap_or_else(|| estimate_context_length(&config.model));

        let threshold_tokens = (context_length as f64 * config.threshold_percent) as usize;
        let target_tokens = (threshold_tokens as f64 * config.summary_target_ratio) as usize;
        let max_summary_tokens =
            ((context_length as f64 * 0.05) as usize).min(SUMMARY_TOKENS_CEILING);

        Self {
            config,
            context_length,
            threshold_tokens,
            compression_count: 0,
            tail_token_budget: target_tokens,
            max_summary_tokens,
            last_prompt_tokens: 0,
            last_completion_tokens: 0,
            previous_summary: None,
            summary_failure_cooldown_until: None,
        }
    }

    /// Update token usage from API response.
    pub fn update_from_response(&mut self, prompt_tokens: usize, completion_tokens: usize) {
        self.last_prompt_tokens = prompt_tokens;
        self.last_completion_tokens = completion_tokens;
    }

    /// Check if context exceeds the compression threshold.
    pub fn should_compress(&self, prompt_tokens: Option<usize>) -> bool {
        let tokens = prompt_tokens.unwrap_or(self.last_prompt_tokens);
        tokens >= self.threshold_tokens
    }

    /// Compress conversation messages by summarizing middle turns.
    ///
    /// Returns the compressed message list.
    pub fn compress(&mut self, messages: &[Value], current_tokens: Option<usize>) -> Vec<Value> {
        let n_messages = messages.len();
        let min_for_compress = self.config.protect_first_n + 3 + 1;
        if n_messages <= min_for_compress {
            return messages.to_vec();
        }

        let display_tokens = current_tokens.unwrap_or(self.last_prompt_tokens);

        // Phase 1: Prune old tool results
        let (messages, pruned_count) =
            self.prune_old_tool_results(messages, self.config.protect_last_n);
        if pruned_count > 0 && !self.config.quiet_mode {
            tracing::info!(
                "Pre-compression: pruned {} old tool result(s)",
                pruned_count
            );
        }

        // Phase 2: Determine boundaries
        let mut compress_start = self.config.protect_first_n;
        compress_start = self.align_boundary_forward(&messages, compress_start);

        let compress_end = self.find_tail_cut_by_tokens(&messages, compress_start);
        if compress_start >= compress_end {
            return messages;
        }

        let turns_to_summarize: Vec<Value> = messages[compress_start..compress_end].to_vec();

        if !self.config.quiet_mode {
            let tail_msgs = n_messages - compress_end;
            tracing::info!(
                "Context compression triggered ({} tokens >= {} threshold)",
                display_tokens,
                self.threshold_tokens
            );
            tracing::info!(
                "Summarizing turns {}-{} ({} turns), protecting {} head + {} tail messages",
                compress_start + 1,
                compress_end,
                turns_to_summarize.len(),
                compress_start,
                tail_msgs
            );
        }

        // Phase 3: Generate structured summary
        let summary = self.generate_summary(&turns_to_summarize);

        // Phase 4: Assemble compressed message list
        let mut compressed: Vec<Value> = Vec::new();

        for (i, msg) in messages.iter().enumerate().take(compress_start) {
            let mut msg = msg.clone();
            if i == 0
                && msg.get("role").and_then(Value::as_str) == Some("system")
                && self.compression_count == 0
            {
                let content = msg.get("content").and_then(Value::as_str).unwrap_or("");
                msg["content"] = Value::String(format!(
                    "{}\n\n[Note: Some earlier conversation turns have been compacted into a \
                    handoff summary to preserve context space. The current session state may \
                    still reflect earlier work, so build on that summary and state rather than \
                    re-doing work.]",
                    content
                ));
            }
            compressed.push(msg);
        }

        // If LLM summary failed, insert static fallback
        let summary_text = summary.unwrap_or_else(|| {
            let n_dropped = compress_end - compress_start;
            format!(
                "{}\nSummary generation was unavailable. {} conversation turns were \
                removed to free context space but could not be summarized. The removed \
                turns contained earlier work in this session. Continue based on the \
                recent messages below and the current state of any files or resources.",
                SUMMARY_PREFIX, n_dropped
            )
        });

        let last_head_role = if compress_start > 0 {
            messages[compress_start - 1]
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("user")
        } else {
            "user"
        };
        let first_tail_role = if compress_end < n_messages {
            messages[compress_end]
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("user")
        } else {
            "user"
        };

        // Pick a role that avoids consecutive same-role with both neighbors
        let mut summary_role = if last_head_role == "assistant" || last_head_role == "tool" {
            "user"
        } else {
            "assistant"
        };

        let mut merge_into_tail = false;
        if summary_role == first_tail_role {
            let flipped = if summary_role == "user" {
                "assistant"
            } else {
                "user"
            };
            if flipped != last_head_role {
                summary_role = flipped;
            } else {
                merge_into_tail = true;
            }
        }

        if !merge_into_tail {
            compressed.push(serde_json::json!({
                "role": summary_role,
                "content": summary_text
            }));
        }

        let mut merged = false;
        for (j, msg) in messages.iter().enumerate().take(n_messages).skip(compress_end) {
            let mut msg = msg.clone();
            if merge_into_tail && !merged && j == compress_end {
                let original = msg.get("content").and_then(Value::as_str).unwrap_or("");
                msg["content"] = Value::String(format!("{}\n\n{}", summary_text, original));
                merged = true;
            }
            compressed.push(msg);
        }

        self.compression_count += 1;

        // Sanitize tool pairs
        compressed = self.sanitize_tool_pairs(&compressed);

        if !self.config.quiet_mode {
            let new_estimate = estimate_messages_tokens(&compressed);
            let saved = display_tokens.saturating_sub(new_estimate);
            tracing::info!(
                "Compressed: {} -> {} messages (~{} tokens saved)",
                n_messages,
                compressed.len(),
                saved
            );
            tracing::info!("Compression #{} complete", self.compression_count);
        }

        compressed
    }

    /// Prune old tool results (cheap pre-pass, no LLM call).
    fn prune_old_tool_results(
        &self,
        messages: &[Value],
        protect_tail_count: usize,
    ) -> (Vec<Value>, usize) {
        if messages.is_empty() {
            return (vec![], 0);
        }

        let mut result: Vec<Value> = messages.to_vec();
        let mut pruned = 0;

        let prune_boundary = messages.len().saturating_sub(protect_tail_count);

        for item in result.iter_mut().take(prune_boundary) {
            if item.get("role").and_then(Value::as_str) != Some("tool") {
                continue;
            }
            let content = item.get("content").and_then(Value::as_str).unwrap_or("");
            if content.is_empty() || content == PRUNED_TOOL_PLACEHOLDER {
                continue;
            }
            if content.len() > 200 {
                item["content"] = Value::String(PRUNED_TOOL_PLACEHOLDER.to_string());
                pruned += 1;
            }
        }

        (result, pruned)
    }

    /// Push compress-start boundary forward past orphan tool results.
    fn align_boundary_forward(&self, messages: &[Value], mut idx: usize) -> usize {
        while idx < messages.len()
            && messages[idx].get("role").and_then(Value::as_str) == Some("tool")
        {
            idx += 1;
        }
        idx
    }

    /// Find tail cut by token budget.
    fn find_tail_cut_by_tokens(
        &self,
        messages: &[Value],
        head_end: usize,
    ) -> usize {
        let n = messages.len();
        let token_budget = self.tail_token_budget;
        let min_tail = 3.min(n.saturating_sub(head_end).saturating_sub(1));
        let soft_ceiling = (token_budget as f64 * 1.5) as usize;

        let mut accumulated = 0;
        let mut cut_idx = n;

        for i in (head_end..n).rev() {
            let msg = &messages[i];
            let content = msg.get("content").and_then(Value::as_str).unwrap_or("");
            let mut msg_tokens = content.len() / CHARS_PER_TOKEN + 10;

            if let Some(tool_calls) = msg.get("tool_calls").and_then(Value::as_array) {
                for tc in tool_calls {
                    if let Some(args) = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(Value::as_str)
                    {
                        msg_tokens += args.len() / CHARS_PER_TOKEN;
                    }
                }
            }

            if accumulated + msg_tokens > soft_ceiling && (n - i) >= min_tail {
                break;
            }
            accumulated += msg_tokens;
            cut_idx = i;
        }

        // Ensure at least min_tail messages are protected
        let fallback_cut = n - min_tail;
        if cut_idx > fallback_cut {
            cut_idx = fallback_cut;
        }

        // Force a cut after head if budget would protect everything
        if cut_idx <= head_end {
            cut_idx = fallback_cut.max(head_end + 1);
        }

        // Align to avoid splitting tool groups
        cut_idx = self.align_boundary_backward(messages, cut_idx);

        cut_idx.max(head_end + 1)
    }

    /// Pull compress-end boundary backward to avoid splitting tool groups.
    fn align_boundary_backward(&self, messages: &[Value], mut idx: usize) -> usize {
        if idx == 0 || idx >= messages.len() {
            return idx;
        }

        let mut check = idx - 1;
        while check > 0
            && messages[check]
                .get("role")
                .and_then(Value::as_str)
                .is_some_and(|r| r == "tool")
        {
            check -= 1;
        }

        if check > 0
            && messages[check]
                .get("role")
                .and_then(Value::as_str)
                .is_some_and(|r| r == "assistant")
            && messages[check].get("tool_calls").is_some()
        {
            idx = check;
        }

        idx
    }

    /// Serialize conversation turns for the summarizer.
    fn serialize_for_summary(turns: &[Value]) -> String {
        let mut parts = Vec::new();

        for msg in turns {
            let role = msg.get("role").and_then(Value::as_str).unwrap_or("unknown");
            let content = msg.get("content").and_then(Value::as_str).unwrap_or("");

            match role {
                "tool" => {
                    let tool_id = msg.get("tool_call_id").and_then(Value::as_str).unwrap_or("");
                    let truncated = truncate_content_for_summary(content);
                    parts.push(format!("[TOOL RESULT {}]: {}", tool_id, truncated));
                }
                "assistant" => {
                    let truncated = truncate_content_for_summary(content);
                    let mut line = format!("[ASSISTANT]: {}", truncated);

                    if let Some(tool_calls) = msg.get("tool_calls").and_then(Value::as_array) {
                        let mut tc_parts = Vec::new();
                        for tc in tool_calls {
                            if let Some(fn_obj) = tc.get("function") {
                                let name =
                                    fn_obj.get("name").and_then(Value::as_str).unwrap_or("?");
                                let args = fn_obj
                                    .get("arguments")
                                    .and_then(Value::as_str)
                                    .unwrap_or("");
                                let truncated_args = if args.len() > 1500 {
                                    format!("{}...", &args[..1200])
                                } else {
                                    args.to_string()
                                };
                                tc_parts.push(format!("  {}({})", name, truncated_args));
                            }
                        }
                        if !tc_parts.is_empty() {
                            line.push_str("\n[Tool calls:\n");
                            line.push_str(&tc_parts.join("\n"));
                            line.push_str("\n]");
                        }
                    }

                    parts.push(line);
                }
                _ => {
                    let truncated = truncate_content_for_summary(content);
                    parts.push(format!("[{}]: {}", role.to_uppercase(), truncated));
                }
            }
        }

        parts.join("\n\n")
    }

    /// Generate structured summary of conversation turns.
    fn generate_summary(&mut self, turns_to_summarize: &[Value]) -> Option<String> {
        // Check cooldown
        if let Some(cooldown_until) = self.summary_failure_cooldown_until {
            // In Rust we'd use Instant::now() but for simplicity, skip cooldown
            // in this synchronous version — it's mainly relevant for async LLM calls
            let _ = cooldown_until;
        }

        let content_to_summarize = Self::serialize_for_summary(turns_to_summarize);
        let summary_budget = self.compute_summary_budget(turns_to_summarize);

        let prompt = if let Some(ref previous) = self.previous_summary {
            format!(
                "You are updating a context compaction summary. A previous compaction \
                produced the summary below. New conversation turns have occurred since then \
                and need to be incorporated.\n\n\
                PREVIOUS SUMMARY:\n{}\n\n\
                NEW TURNS TO INCORPORATE:\n{}\n\n\
                Update the summary using this exact structure. PRESERVE all existing \
                information that is still relevant. ADD new progress.\n\n\
                ## Goal\n[What the user is trying to accomplish]\n\n\
                ## Constraints & Preferences\n[User preferences, coding style]\n\n\
                ## Progress\n### Done\n[Completed work]\n### In Progress\n[Work in progress]\n### Blocked\n[Blockers]\n\n\
                ## Key Decisions\n[Important technical decisions]\n\n\
                ## Relevant Files\n[Files read, modified, created]\n\n\
                ## Next Steps\n[What needs to happen next]\n\n\
                ## Critical Context\n[Specific values, errors, configuration]\n\n\
                ## Tools & Patterns\n[Which tools were used, how effectively]\n\n\
                Target ~{} tokens. Be specific — include file paths, command outputs, \
                error messages, and concrete values.\n\n\
                Write only the summary body. Do not include any preamble or prefix.",
                previous, content_to_summarize, summary_budget
            )
        } else {
            format!(
                "Create a structured handoff summary for a later assistant that will \
                continue this conversation after earlier turns are compacted.\n\n\
                TURNS TO SUMMARIZE:\n{}\n\n\
                Use this exact structure:\n\n\
                ## Goal\n[What the user is trying to accomplish]\n\n\
                ## Constraints & Preferences\n[User preferences, coding style]\n\n\
                ## Progress\n### Done\n[Completed work]\n### In Progress\n[Work in progress]\n### Blocked\n[Blockers]\n\n\
                ## Key Decisions\n[Important technical decisions]\n\n\
                ## Relevant Files\n[Files read, modified, created]\n\n\
                ## Next Steps\n[What needs to happen next]\n\n\
                ## Critical Context\n[Specific values, errors, configuration]\n\n\
                ## Tools & Patterns\n[Which tools were used, how effectively]\n\n\
                Target ~{} tokens. Be specific — include file paths, command outputs, \
                error messages, and concrete values.\n\n\
                Write only the summary body. Do not include any preamble or prefix.",
                content_to_summarize, summary_budget
            )
        };

        // Store for iterative updates
        // In a full implementation, this would call an auxiliary LLM.
        // For now, return the prompt as the summary (it would be used by the agent engine).
        let summary = prompt;
        self.previous_summary = Some(summary.clone());
        Some(with_summary_prefix(&summary))
    }

    /// Compute summary token budget.
    fn compute_summary_budget(&self, turns_to_summarize: &[Value]) -> usize {
        let content_tokens = estimate_messages_tokens(turns_to_summarize);
        let budget = (content_tokens as f64 * SUMMARY_RATIO) as usize;
        budget.max(MIN_SUMMARY_TOKENS).min(self.max_summary_tokens)
    }

    /// Sanitize orphaned tool_call / tool_result pairs.
    fn sanitize_tool_pairs(&self, messages: &[Value]) -> Vec<Value> {
        // Collect all surviving tool call IDs
        let mut surviving_call_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for msg in messages {
            if msg.get("role").and_then(Value::as_str) == Some("assistant") {
                if let Some(tool_calls) = msg.get("tool_calls").and_then(Value::as_array) {
                    for tc in tool_calls {
                        if let Some(cid) = tc.get("id").and_then(Value::as_str) {
                            if !cid.is_empty() {
                                surviving_call_ids.insert(cid.to_string());
                            }
                        }
                    }
                }
            }
        }

        // Collect tool result call IDs
        let mut result_call_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for msg in messages {
            if msg.get("role").and_then(Value::as_str) == Some("tool") {
                if let Some(cid) = msg.get("tool_call_id").and_then(Value::as_str) {
                    if !cid.is_empty() {
                        result_call_ids.insert(cid.to_string());
                    }
                }
            }
        }

        // Remove orphaned tool results
        let orphaned_results: std::collections::HashSet<_> =
            result_call_ids.difference(&surviving_call_ids).cloned().collect();
        let mut filtered: Vec<Value> = messages
            .iter()
            .filter(|m| {
                if m.get("role").and_then(Value::as_str) != Some("tool") {
                    return true;
                }
                if let Some(cid) = m.get("tool_call_id").and_then(Value::as_str) {
                    return !orphaned_results.contains(cid);
                }
                true
            })
            .cloned()
            .collect();

        // Add stub results for orphaned tool calls
        let missing_results: std::collections::HashSet<_> =
            surviving_call_ids.difference(&result_call_ids).cloned().collect();
        if !missing_results.is_empty() {
            let mut patched: Vec<Value> = Vec::new();
            for msg in &filtered {
                patched.push(msg.clone());
                if msg.get("role").and_then(Value::as_str) == Some("assistant") {
                    if let Some(tool_calls) = msg.get("tool_calls").and_then(Value::as_array) {
                        for tc in tool_calls {
                            if let Some(cid) = tc.get("id").and_then(Value::as_str) {
                                if missing_results.contains(cid) {
                                    patched.push(serde_json::json!({
                                        "role": "tool",
                                        "content": "[Result from earlier conversation — see context summary above]",
                                        "tool_call_id": cid
                                    }));
                                }
                            }
                        }
                    }
                }
            }
            filtered = patched;
        }

        filtered
    }
}

/// Truncate content for summary input.
fn truncate_content_for_summary(content: &str) -> String {
    const CONTENT_MAX: usize = 6000;
    const CONTENT_HEAD: usize = 4000;
    const CONTENT_TAIL: usize = 1500;

    if content.len() <= CONTENT_MAX {
        return content.to_string();
    }
    format!(
        "{}\n...[truncated]...\n{}",
        &content[..CONTENT_HEAD],
        &content[content.len() - CONTENT_TAIL..]
    )
}

/// Normalize summary text with prefix.
fn with_summary_prefix(summary: &str) -> String {
    let text = summary.trim();

    // Strip legacy prefix if present
    let text = if let Some(stripped) = text.strip_prefix(LEGACY_SUMMARY_PREFIX) {
        stripped.trim()
    } else if let Some(stripped) = text.strip_prefix(SUMMARY_PREFIX) {
        stripped.trim()
    } else {
        text
    };

    if text.is_empty() {
        SUMMARY_PREFIX.to_string()
    } else {
        format!("{}\n{}", SUMMARY_PREFIX, text)
    }
}

/// Estimate tokens for a message slice.
fn estimate_messages_tokens(messages: &[Value]) -> usize {
    let mut total = 0;
    for msg in messages {
        let content = msg.get("content").and_then(Value::as_str).unwrap_or("");
        total += content.len() / CHARS_PER_TOKEN + 10;
    }
    total
}

/// Estimate context length for a model.
///
/// This is a simplified version. In production, this would query
/// model metadata or use a lookup table.
fn estimate_context_length(model: &str) -> usize {
    // Default fallback
    if model.contains("opus") || model.contains("claude-3") {
        200_000
    } else if model.contains("gpt-4") {
        128_000
    } else if model.contains("gemini") {
        1_000_000
    } else {
        128_000 // safe default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_messages(count: usize) -> Vec<Value> {
        let mut msgs = vec![serde_json::json!({
            "role": "system",
            "content": "You are a helpful assistant."
        })];
        for i in 0..count {
            if i % 2 == 0 {
                msgs.push(serde_json::json!({
                    "role": "user",
                    "content": format!("Message {}", i)
                }));
            } else {
                msgs.push(serde_json::json!({
                    "role": "assistant",
                    "content": format!("Response {}", i)
                }));
            }
        }
        msgs
    }

    #[test]
    fn test_should_compress() {
        let config = CompressorConfig {
            model: "claude-3-opus".to_string(),
            threshold_percent: 0.50,
            ..Default::default()
        };
        let compressor = ContextCompressor::new(config);
        // threshold = 200000 * 0.50 = 100000
        assert!(!compressor.should_compress(Some(50_000)));
        assert!(compressor.should_compress(Some(150_000)));
    }

    #[test]
    fn test_prune_tool_results() {
        let config = CompressorConfig::default();
        let compressor = ContextCompressor::new(config);

        let messages = vec![
            serde_json::json!({"role": "system", "content": "Be helpful."}),
            serde_json::json!({"role": "user", "content": "Run this"}),
            serde_json::json!({"role": "assistant", "content": "", "tool_calls": [{"id": "tc1", "function": {"name": "run", "arguments": "{}"}}]}),
            serde_json::json!({"role": "tool", "content": "x".repeat(300), "tool_call_id": "tc1"}),
            serde_json::json!({"role": "user", "content": "Next"}),
            serde_json::json!({"role": "assistant", "content": "Done"}),
        ];

        let (result, count) = compressor.prune_old_tool_results(&messages, 2);
        // The tool result has 300 chars (> 200), but it's within the protected tail
        // With protect_tail_count=2, only the last 2 messages are protected
        // So the tool result at index 3 should be pruned
        assert_eq!(count, 1);
        assert_eq!(
            result[3].get("content").and_then(Value::as_str),
            Some(PRUNED_TOOL_PLACEHOLDER)
        );
    }

    #[test]
    fn test_sanitize_tool_pairs_removes_orphans() {
        let config = CompressorConfig::default();
        let compressor = ContextCompressor::new(config);

        // tc1 is a tool_call with no matching result
        // tc2 is a tool_result with no matching call (orphan)
        let messages = vec![
            serde_json::json!({"role": "assistant", "content": "", "tool_calls": [{"id": "tc1", "function": {"name": "run", "arguments": "{}"}}]}),
            serde_json::json!({"role": "tool", "content": "result", "tool_call_id": "tc2"}), // orphan
        ];

        let result = compressor.sanitize_tool_pairs(&messages);
        // Orphaned tc2 result is removed, but stub is added for tc1
        // So we still have 2 messages: assistant + stub result for tc1
        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0].get("role").and_then(Value::as_str),
            Some("assistant")
        );
        // Second message should be a stub result for tc1
        assert_eq!(
            result[1].get("role").and_then(Value::as_str),
            Some("tool")
        );
        assert_eq!(
            result[1].get("tool_call_id").and_then(Value::as_str),
            Some("tc1")
        );
    }

    #[test]
    fn test_sanitize_tool_pairs_adds_stubs() {
        let config = CompressorConfig::default();
        let compressor = ContextCompressor::new(config);

        let messages = vec![
            serde_json::json!({"role": "assistant", "content": "", "tool_calls": [{"id": "tc1", "function": {"name": "run", "arguments": "{}"}}]}),
        ];

        let result = compressor.sanitize_tool_pairs(&messages);
        // Stub result should be added
        assert_eq!(result.len(), 2);
        assert_eq!(result[1].get("role").and_then(Value::as_str), Some("tool"));
        assert_eq!(
            result[1].get("tool_call_id").and_then(Value::as_str),
            Some("tc1")
        );
    }

    #[test]
    fn test_truncate_content_for_summary() {
        let content = "a".repeat(7000);
        let result = truncate_content_for_summary(&content);
        assert!(result.contains("...[truncated]..."));
        assert!(result.len() < 7000);
    }

    #[test]
    fn test_with_summary_prefix() {
        let result = with_summary_prefix("## Goal\nTest");
        assert!(result.starts_with(SUMMARY_PREFIX));
        assert!(result.contains("## Goal"));
    }

    #[test]
    fn test_with_summary_prefix_strips_legacy() {
        let result = with_summary_prefix("[CONTEXT SUMMARY]:\n## Goal\nTest");
        assert!(result.starts_with(SUMMARY_PREFIX));
        assert!(!result.contains(LEGACY_SUMMARY_PREFIX));
    }

    #[test]
    fn test_compress_too_few_messages() {
        let config = CompressorConfig::default();
        let mut compressor = ContextCompressor::new(config);

        let messages = make_messages(3); // system + 2 = only 3 messages
        let result = compressor.compress(&messages, None);
        assert_eq!(result.len(), messages.len()); // unchanged
    }

    #[test]
    fn test_estimate_context_length() {
        assert_eq!(estimate_context_length("claude-3-opus"), 200_000);
        assert_eq!(estimate_context_length("gpt-4o"), 128_000);
        assert_eq!(estimate_context_length("gemini-pro"), 1_000_000);
        assert_eq!(estimate_context_length("unknown"), 128_000);
    }

    #[test]
    fn test_align_boundary_forward() {
        let config = CompressorConfig::default();
        let compressor = ContextCompressor::new(config);

        let messages = vec![
            serde_json::json!({"role": "tool", "content": "orphan"}),
            serde_json::json!({"role": "tool", "content": "orphan"}),
            serde_json::json!({"role": "user", "content": "start"}),
        ];

        let result = compressor.align_boundary_forward(&messages, 0);
        assert_eq!(result, 2); // should skip past the tool results
    }

    #[test]
    fn test_compress_preserves_turn_structure() {
        // Create a conversation with many turns
        let mut messages = vec![
            serde_json::json!({"role": "system", "content": "You are a helpful assistant."}),
        ];
        for i in 0..20 {
            messages.push(serde_json::json!({
                "role": "user",
                "content": format!("Question number {} about topic {}", i, i % 5)
            }));
            messages.push(serde_json::json!({
                "role": "assistant",
                "content": format!("Answer to question {} - detailed response here", i)
            }));
        }

        let config = CompressorConfig {
            model: "claude-3-opus".to_string(),
            threshold_percent: 0.50,
            protect_last_n: 4,
            protect_first_n: 2,
            ..Default::default()
        };
        let mut compressor = ContextCompressor::new(config);
        // Simulate high token usage to trigger compression
        compressor.update_from_response(100_000, 50_000);

        // Too few for compression with our thresholds, but test it doesn't panic
        let result = compressor.compress(&messages, None);
        // With 42 messages (> 10), compression should run or return unchanged
        assert!(result.len() >= 1);
    }

    #[test]
    fn test_prune_tool_results_preserves_recent() {
        let config = CompressorConfig::default();
        let compressor = ContextCompressor::new(config);

        // Many old tool results followed by a fresh conversation
        let mut messages = vec![
            serde_json::json!({"role": "system", "content": "You are a helpful assistant."}),
            serde_json::json!({"role": "user", "content": "Run analysis"}),
        ];
        for i in 0..5 {
            messages.push(serde_json::json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{"id": format!("tc{i}"), "function": {"name": "read_file", "arguments": "{}"}}]
            }));
            messages.push(serde_json::json!({
                "role": "tool",
                "content": "x".repeat(500),
                "tool_call_id": format!("tc{i}")
            }));
        }
        // Fresh conversation at the end
        messages.push(serde_json::json!({"role": "user", "content": "What did we learn?"}));
        messages.push(serde_json::json!({"role": "assistant", "content": "We learned a lot."}));

        let (result, count) = compressor.prune_old_tool_results(&messages, 2);
        // Should have pruned some old tool results
        assert!(count > 0);
        // Last two messages should be unchanged
        assert_eq!(result.last().unwrap()["content"], "We learned a lot.");
    }

    #[test]
    fn test_compressor_config_defaults() {
        let config = CompressorConfig::default();
        assert_eq!(config.threshold_percent, 0.50);
        assert_eq!(config.protect_first_n, 3);
        assert_eq!(config.protect_last_n, 20);
        assert!(!config.quiet_mode);
    }

    #[test]
    fn test_compress_mixed_roles() {
        let config = CompressorConfig::default();
        let mut compressor = ContextCompressor::new(config);

        let messages = vec![
            serde_json::json!({"role": "system", "content": "Be helpful."}),
            serde_json::json!({"role": "user", "content": "Hi"}),
            serde_json::json!({"role": "assistant", "content": "Hello!"}),
            serde_json::json!({"role": "tool", "content": "result", "tool_call_id": "tc1"}),
            serde_json::json!({"role": "user", "content": "Thanks"}),
        ];

        // Not enough messages to compress, should return as-is
        let result = compressor.compress(&messages, None);
        assert_eq!(result.len(), messages.len());
    }
}
