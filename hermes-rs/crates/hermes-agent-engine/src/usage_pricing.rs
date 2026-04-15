use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingEntry {
    pub input_cost_per_million: f64,
    pub output_cost_per_million: f64,
    pub cache_read_cost_per_million: f64,
    pub cache_write_cost_per_million: f64,
    pub request_cost: f64,
    pub source: String,
    pub source_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CanonicalUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct CostResult {
    pub amount_usd: f64,
    pub status: String,
    pub source: String,
    pub label: String,
}

static PRICING_TABLE: Lazy<HashMap<&'static str, PricingEntry>> = Lazy::new(|| {
    let mut m = HashMap::new();

    // Anthropic
    m.insert(
        "claude-opus-4-6",
        PricingEntry {
            input_cost_per_million: 15.0,
            output_cost_per_million: 75.0,
            cache_read_cost_per_million: 1.875,
            cache_write_cost_per_million: 15.0,
            request_cost: 0.0,
            source: "official_docs".into(),
            source_url: Some("https://docs.anthropic.com/en/docs/about-claude/opus-4-6".into()),
        },
    );
    m.insert(
        "claude-sonnet-4-6",
        PricingEntry {
            input_cost_per_million: 3.0,
            output_cost_per_million: 15.0,
            cache_read_cost_per_million: 0.3,
            cache_write_cost_per_million: 3.0,
            request_cost: 0.0,
            source: "official_docs".into(),
            source_url: Some("https://docs.anthropic.com/en/docs/about-claude/sonnet-4-6".into()),
        },
    );
    m.insert(
        "claude-haiku-4-5-20251001",
        PricingEntry {
            input_cost_per_million: 0.80,
            output_cost_per_million: 4.0,
            cache_read_cost_per_million: 0.08,
            cache_write_cost_per_million: 0.80,
            request_cost: 0.0,
            source: "official_docs".into(),
            source_url: Some("https://docs.anthropic.com/en/docs/about-claude/haiku".into()),
        },
    );

    // OpenAI
    m.insert(
        "gpt-4o",
        PricingEntry {
            input_cost_per_million: 2.50,
            output_cost_per_million: 10.0,
            cache_read_cost_per_million: 1.25,
            cache_write_cost_per_million: 2.50,
            request_cost: 0.0,
            source: "official_docs".into(),
            source_url: Some("https://openai.com/api/pricing/".into()),
        },
    );
    m.insert(
        "gpt-4o-mini",
        PricingEntry {
            input_cost_per_million: 0.15,
            output_cost_per_million: 0.60,
            cache_read_cost_per_million: 0.075,
            cache_write_cost_per_million: 0.15,
            request_cost: 0.0,
            source: "official_docs".into(),
            source_url: Some("https://openai.com/api/pricing/".into()),
        },
    );

    m
});

pub fn normalize_usage(
    usage: &serde_json::Value,
    provider: &str,
    api_mode: Option<&str>,
) -> CanonicalUsage {
    match provider {
        "anthropic" => {
            let input = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            let output = usage
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cache_read = usage
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cache_write = usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let reasoning = usage
                .get("reasoning_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            CanonicalUsage {
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: cache_read,
                cache_write_tokens: cache_write,
                reasoning_tokens: reasoning,
            }
        }
        "openai" | "openrouter" => {
            let prompt = usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            let completion = usage
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cache_read = usage
                .get("prompt_tokens_details")
                .and_then(|v| v.get("cached_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cache_write = 0;
            let reasoning = usage
                .get("completion_tokens_details")
                .and_then(|v| v.get("reasoning_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let input_tokens = prompt.saturating_sub(cache_read).saturating_sub(cache_write);
            CanonicalUsage {
                input_tokens,
                output_tokens: completion,
                cache_read_tokens: cache_read,
                cache_write_tokens: cache_write,
                reasoning_tokens: reasoning,
            }
        }
        _ => {
            let input = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            let output = usage
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cache_read = usage
                .get("cache_read_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cache_write = usage
                .get("cache_write_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let reasoning = usage
                .get("reasoning_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            CanonicalUsage {
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: cache_read,
                cache_write_tokens: cache_write,
                reasoning_tokens: reasoning,
            }
        }
    }
}

pub fn estimate_usage_cost(
    model: &str,
    usage: &CanonicalUsage,
    provider: &str,
    base_url: Option<&str>,
) -> CostResult {
    let entry = PRICING_TABLE.get(model);

    let (pricing, source, status) = match entry {
        Some(p) => (p, p.source.clone(), "known".to_string()),
        None => {
            let is_openrouter = provider == "openrouter"
                || base_url.map(|u| u.contains("openrouter")).unwrap_or(false);
            if is_openrouter {
                let fallback = PricingEntry {
                    input_cost_per_million: 1.0,
                    output_cost_per_million: 5.0,
                    cache_read_cost_per_million: 0.1,
                    cache_write_cost_per_million: 1.0,
                    request_cost: 0.0,
                    source: "openrouter_api".into(),
                    source_url: None,
                };
                (fallback, "openrouter_api".into(), "known".into())
            } else {
                let unknown = PricingEntry {
                    input_cost_per_million: 0.0,
                    output_cost_per_million: 0.0,
                    cache_read_cost_per_million: 0.0,
                    cache_write_cost_per_million: 0.0,
                    request_cost: 0.0,
                    source: "unknown".into(),
                    source_url: None,
                };
                (unknown, "unknown".into(), "unknown".into())
            }
        }
    };

    let amount = (usage.input_tokens as f64 * pricing.input_cost_per_million
        + usage.output_tokens as f64 * pricing.output_cost_per_million
        + usage.cache_read_tokens as f64 * pricing.cache_read_cost_per_million
        + usage.cache_write_tokens as f64 * pricing.cache_write_cost_per_million
        + pricing.request_cost)
        / 1_000_000.0;

    let label = if amount > 0.0 {
        format!("${:.4}", amount)
    } else {
        "$0.00".into()
    };

    CostResult {
        amount_usd: amount,
        status,
        source,
        label,
    }
}

pub fn has_known_pricing(model: &str, _provider: &str) -> bool {
    PRICING_TABLE.contains_key(model)
}

pub fn format_duration_compact(seconds: f64) -> String {
    if seconds < 1.0 {
        return format!("{}ms", (seconds * 1000.0).round() as u64);
    }
    if seconds < 60.0 {
        return format!("{}s", seconds.round() as u64);
    }
    let total_secs = seconds.round() as u64;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    if mins < 60 {
        if secs > 0 {
            format!("{}m {}s", mins, secs)
        } else {
            format!("{}m", mins)
        }
    } else {
        let hours = mins / 60;
        let remain_mins = mins % 60;
        if remain_mins > 0 {
            format!("{}h {}m", hours, remain_mins)
        } else {
            format!("{}h", hours)
        }
    }
}

pub fn format_token_count_compact(value: u64) -> String {
    if value >= 1_000_000 {
        let scaled = value as f64 / 1_000_000.0;
        if scaled == (scaled as u64) as f64 {
            format!("{}M", scaled as u64)
        } else {
            format!("{:.1}M", scaled)
        }
    } else if value >= 1_000 {
        let scaled = value as f64 / 1_000.0;
        if scaled == (scaled as u64) as f64 {
            format!("{}K", scaled as u64)
        } else {
            format!("{:.1}K", scaled)
        }
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_anthropic() {
        let json = serde_json::json!({
            "input_tokens": 1000,
            "output_tokens": 500,
            "cache_read_input_tokens": 200,
            "cache_creation_input_tokens": 300,
        });
        let u = normalize_usage(&json, "anthropic", None);
        assert_eq!(u.input_tokens, 1000);
        assert_eq!(u.output_tokens, 500);
        assert_eq!(u.cache_read_tokens, 200);
        assert_eq!(u.cache_write_tokens, 300);
    }

    #[test]
    fn test_normalize_openai() {
        let json = serde_json::json!({
            "prompt_tokens": 1000,
            "completion_tokens": 500,
            "prompt_tokens_details": {"cached_tokens": 200},
            "completion_tokens_details": {"reasoning_tokens": 100},
        });
        let u = normalize_usage(&json, "openai", None);
        assert_eq!(u.input_tokens, 800);
        assert_eq!(u.output_tokens, 500);
        assert_eq!(u.cache_read_tokens, 200);
        assert_eq!(u.reasoning_tokens, 100);
    }

    #[test]
    fn test_cost_claude_sonnet() {
        let usage = CanonicalUsage {
            input_tokens: 1_000_000,
            output_tokens: 500_000,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
        };
        let cost = estimate_usage_cost("claude-sonnet-4-6", &usage, "anthropic", None);
        assert_eq!(cost.status, "known");
        assert!((cost.amount_usd - 10.5).abs() < 0.001);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration_compact(0.05), "50ms");
        assert_eq!(format_duration_compact(45.0), "45s");
        assert_eq!(format_duration_compact(135.0), "2m 15s");
        assert_eq!(format_duration_compact(3600.0), "1h");
        assert_eq!(format_duration_compact(5000.0), "1h 23m");
    }

    #[test]
    fn test_format_token_count() {
        assert_eq!(format_token_count_compact(500), "500");
        assert_eq!(format_token_count_compact(1500), "1.5K");
        assert_eq!(format_token_count_compact(450_000), "450K");
        assert_eq!(format_token_count_compact(1_200_000), "1.2M");
    }

    #[test]
    fn test_unknown_pricing() {
        let usage = CanonicalUsage::default();
        let cost = estimate_usage_cost("unknown-model", &usage, "unknown", None);
        assert_eq!(cost.status, "unknown");
    }

    #[test]
    fn test_openrouter_fallback() {
        let usage = CanonicalUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            ..Default::default()
        };
        let cost = estimate_usage_cost("some-model", &usage, "openrouter", None);
        assert_eq!(cost.source, "openrouter_api");
        assert!((cost.amount_usd - 6.0).abs() < 0.001);
    }
}
