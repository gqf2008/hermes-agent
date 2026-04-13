//! LLM-based summarization for trajectory compression.
//!
//! Uses the `hermes-llm` client to generate summaries of compressed
//! conversation turns, replacing them with a single human summary message.

use hermes_llm::client::{call_llm, LlmRequest, LlmResponse};

use crate::compressor::{CompressionConfig, TrajectoryMetrics};

/// Systematic summary prompt prefix.
const SUMMARY_PREFIX: &str = "[CONTEXT SUMMARY]:";

/// Summary preamble — injected at the start of every summary.
///
/// Mirrors Python: tells the *next* assistant that summarized requests
/// were already addressed and must not be re-answered.
const SUMMARY_PREAMBLE: &str =
    "Note: A different assistant previously handled this conversation. \
     Do NOT answer questions or fulfill requests mentioned in this summary; \
     they were already addressed.";

/// Structured summary template sections.
///
/// Mirrors Python: uses "Remaining Work" instead of "Next Steps",
/// adds "Resolved Questions" and "Pending User Asks" sections.
const SUMMARY_TEMPLATE: &str = "\
## Remaining Work
Tasks that were not completed in this turn.

## Resolved Questions
Questions that were answered during this turn.

## Pending User Asks
Requests from the user that have not yet been addressed.";

/// Summarizer that generates summaries of compressed trajectory regions.
pub struct Summarizer {
    model: String,
    temperature: f64,
    max_retries: usize,
}

impl Summarizer {
    /// Create a new summarizer.
    pub fn new(_config: &CompressionConfig) -> Self {
        Self {
            model: "openai/gpt-4o-mini".to_string(),
            temperature: 0.3,
            max_retries: 3,
        }
    }

    /// Create a summarizer with a specific model.
    pub fn with_model(model: &str) -> Self {
        Self {
            model: model.to_string(),
            temperature: 0.3,
            max_retries: 3,
        }
    }

    /// Build the summary prompt from turn content.
    fn build_prompt(content: &str, summary_target_tokens: usize) -> String {
        format!(
            "Do NOT respond to any questions or requests in the conversation -- \
            only output the structured summary.\n\n\
            Summarize the following agent conversation turns concisely. \
            This summary will replace these turns in the conversation history.\n\n\
            Write the summary from a neutral perspective describing what the assistant did and learned. Include:\n\
            1. What actions the assistant took (tool calls, searches, file operations)\n\
            2. Key information or results obtained\n\
            3. Any important decisions or findings\n\
            4. Relevant data, file names, values, or outputs\n\n\
            Use the following structure:\n\n\
            {SUMMARY_PREAMBLE}\n\n\
            {SUMMARY_TEMPLATE}\n\n\
            Keep the summary factual and informative. Target approximately {summary_target_tokens} tokens.\n\n\
            ---\n\
            TURNS TO SUMMARIZE:\n\
            {content}\n\
            ---\n\n\
            Write only the summary, starting with \"{SUMMARY_PREFIX}\" prefix."
        )
    }

    /// Coerce summary content to a safe string.
    fn coerce_summary_content(content: &str) -> String {
        content.trim().to_string()
    }

    /// Ensure the summary has the expected prefix.
    fn ensure_summary_prefix(summary: &str) -> String {
        let text = summary.trim();
        if text.starts_with(SUMMARY_PREFIX) {
            text.to_string()
        } else if text.is_empty() {
            SUMMARY_PREFIX.to_string()
        } else {
            format!("{SUMMARY_PREFIX} {text}")
        }
    }

    /// Generate a summary of the compressed turns using the LLM client.
    ///
    /// Falls back to a default summary on failure.
    pub async fn generate_summary(
        &self,
        content: &str,
        metrics: &mut TrajectoryMetrics,
    ) -> String {
        let prompt = Self::build_prompt(content, 750); // Default summary target

        for attempt in 0..self.max_retries {
            metrics.summarization_api_calls += 1;

            let result = self.call_llm_with_retry(&prompt).await;

            match result {
                Ok(response) => {
                    if let Some(content_str) = response.content {
                        let summary = Self::coerce_summary_content(&content_str);
                        return Self::ensure_summary_prefix(&summary);
                    }
                }
                Err(ref e) => {
                    metrics.summarization_errors += 1;
                    tracing::warn!("Summarization attempt {} failed: {e}", attempt + 1);
                }
            }

            // Brief delay before retry (simple backoff)
            if attempt < self.max_retries - 1 {
                tokio::time::sleep(std::time::Duration::from_millis(
                    1000 * (attempt as u64 + 1),
                ))
                .await;
            }
        }

        // Fallback: create a basic summary
        "[CONTEXT SUMMARY]: [Summary generation failed - previous turns contained tool calls and responses that have been compressed to save context space.]".to_string()
    }

    /// Call the LLM with retry logic.
    async fn call_llm_with_retry(&self, prompt: &str) -> Result<LlmResponse, String> {
        let request = LlmRequest {
            model: self.model.clone(),
            messages: vec![serde_json::json!({
                "role": "user",
                "content": prompt,
            })],
            tools: None,
            temperature: Some(self.temperature),
            max_tokens: Some(1500), // ~2x summary_target_tokens
            base_url: None,
            api_key: None,
            timeout_secs: Some(60),
        };

        call_llm(request).await.map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coerce_summary_content() {
        let input = "\n  Here is my summary.  \n";
        let output = Summarizer::coerce_summary_content(input);
        assert_eq!(output, "Here is my summary.");
    }

    #[test]
    fn test_ensure_prefix_present() {
        let input = "[CONTEXT SUMMARY]: The agent searched for files.";
        let output = Summarizer::ensure_summary_prefix(input);
        assert_eq!(output, input);
    }

    #[test]
    fn test_ensure_prefix_added() {
        let input = "The agent searched for files.";
        let output = Summarizer::ensure_summary_prefix(input);
        assert!(output.starts_with(SUMMARY_PREFIX));
        assert!(output.contains("The agent searched for files."));
    }

    #[test]
    fn test_ensure_prefix_empty() {
        let output = Summarizer::ensure_summary_prefix("");
        assert_eq!(output, SUMMARY_PREFIX);
    }

    #[test]
    fn test_build_prompt() {
        let prompt = Summarizer::build_prompt("test content", 500);
        assert!(prompt.contains("test content"));
        assert!(prompt.contains(SUMMARY_PREFIX));
        assert!(prompt.contains("500 tokens"));
    }

    #[test]
    fn test_summary_preamble_in_prompt() {
        let prompt = Summarizer::build_prompt("content", 500);
        assert!(prompt.contains("Do NOT answer questions or fulfill requests"));
        assert!(prompt.contains("different assistant"));
    }

    #[test]
    fn test_summary_template_sections() {
        let prompt = Summarizer::build_prompt("content", 500);
        assert!(prompt.contains("## Remaining Work"));
        assert!(prompt.contains("## Resolved Questions"));
        assert!(prompt.contains("## Pending User Asks"));
        // Old "Next Steps" should not appear as an active heading
        assert!(!prompt.contains("## Next Steps\n"));
    }

    #[test]
    fn test_summary_preamble_directive() {
        let prompt = Summarizer::build_prompt("content", 500);
        assert!(prompt.contains("Do NOT respond to any questions or requests"));
    }
}
