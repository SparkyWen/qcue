//! Token-usage → cost_micros pricing (the piece the audit found missing entirely, which is why the
//! cost ledger was structurally always 0). Rates are USD per 1M tokens; since 1 USD = 1_000_000 micros,
//! micros-per-token == USD-per-1M-tokens, so `cost_micros = tokens * rate` (rounded). Keyed by model id
//! (more specific than provider); cache_read is discounted, cache_write surcharged, reasoning billed at
//! the output rate. Approximate published list prices — exact enough for the daily safety ceiling.
use protocol::CanonicalUsage;

/// (input_rate, output_rate) in USD-per-1M-tokens == micros-per-token, by model id prefix.
fn rates(model: &str) -> (f64, f64) {
    match model {
        m if m.starts_with("deepseek-reasoner") => (0.55, 2.19),
        m if m.starts_with("deepseek") => (0.28, 1.10),
        m if m.starts_with("gpt-5") => (1.25, 10.0),
        m if m.starts_with("gpt-4o-mini") => (0.15, 0.60),
        m if m.starts_with("gpt-4o") => (2.50, 10.0),
        m if m.starts_with("o4") || m.starts_with("o3") || m.starts_with("o1") => (1.10, 4.40),
        // The Claude 4.x Opus line (opus-4-5/4-6/4-7/4-8) is $5/$25 — NOT the legacy Opus-3 $15/$75 the
        // table used to carry (which over-billed the ledger 3× for every opus-4-8 recall/dream turn).
        m if m.starts_with("claude-opus") => (5.0, 25.0),
        m if m.starts_with("claude-fable") || m.starts_with("claude-mythos") => (10.0, 50.0),
        m if m.starts_with("claude-sonnet") => (3.0, 15.0),
        m if m.starts_with("claude-haiku") => (1.0, 5.0),
        m if m.starts_with("gemini-3-flash") => (0.30, 2.50),
        m if m.starts_with("gemini") => (1.25, 10.0),
        _ => (1.0, 3.0), // conservative default for an unknown / OpenAiCompatible long-tail model
    }
}

/// Cost in micros for one call's usage on `model`. cache_read is 0.1× input, cache_write 1.25× input,
/// reasoning billed at the output rate.
pub fn cost_micros(model: &str, u: &CanonicalUsage) -> i64 {
    let (in_rate, out_rate) = rates(model);
    let micros = u.input as f64 * in_rate
        + u.output as f64 * out_rate
        + u.cache_read as f64 * in_rate * 0.1
        + u.cache_write as f64 * in_rate * 1.25
        + u.reasoning as f64 * out_rate;
    micros.round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u64, output: u64) -> CanonicalUsage {
        CanonicalUsage { input, output, ..Default::default() }
    }

    #[test]
    fn input_output_priced_per_million() {
        // 1M input tokens at $0.15/1M = 150_000 micros; 1M output at $0.60/1M = 600_000 micros.
        assert_eq!(cost_micros("gpt-4o-mini", &usage(1_000_000, 0)), 150_000);
        assert_eq!(cost_micros("gpt-4o-mini", &usage(0, 1_000_000)), 600_000);
        assert_eq!(cost_micros("gpt-4o-mini", &usage(1_000_000, 1_000_000)), 750_000);
    }

    #[test]
    fn deepseek_and_anthropic_have_distinct_rates() {
        assert_eq!(cost_micros("deepseek-v4-pro", &usage(1_000_000, 0)), 280_000);
        // Opus 4.x is $5/1M input (not the legacy Opus-3 $15) → 5_000_000 micros for 1M input tokens.
        assert_eq!(cost_micros("claude-opus-4-8", &usage(1_000_000, 0)), 5_000_000);
        assert_eq!(cost_micros("claude-opus-4-8", &usage(0, 1_000_000)), 25_000_000);
    }

    #[test]
    fn unknown_model_uses_the_default_rate() {
        assert_eq!(cost_micros("some-future-model", &usage(1_000_000, 0)), 1_000_000);
    }

    #[test]
    fn reasoning_tokens_billed_at_output_rate() {
        let u = CanonicalUsage { reasoning: 1_000_000, ..Default::default() };
        assert_eq!(cost_micros("o4-mini", &u), 4_400_000);
    }

    #[test]
    fn zero_usage_is_zero_cost() {
        assert_eq!(cost_micros("gpt-5.1", &CanonicalUsage::default()), 0);
    }
}
