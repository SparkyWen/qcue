// QCue S2-R13 — granularity table + short-content downgrade + per-item token budget (PORT batch-limits.ts).
pub const TOKENS_PER_ITEM_BUDGET: u32 = 400; // constants.ts:52
const CHARS_PER_ITEM: usize = 600; // short-content downgrade ~1 item / 600 chars

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Fine,
    Standard,
    Coarse,
    Minimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchLimits {
    pub items_per_batch: usize,
    pub max_total_items: usize,
    pub max_tokens: u32,
}

fn base_items_per_batch(g: Granularity) -> usize {
    match g {
        Granularity::Fine => 15,
        Granularity::Standard => 8,
        Granularity::Coarse => 5,
        Granularity::Minimal => 3,
    }
}
fn base_cap(g: Granularity) -> usize {
    match g {
        Granularity::Fine => 100,
        Granularity::Standard => 50,
        Granularity::Coarse => 25,
        Granularity::Minimal => 10,
    }
}

pub fn calculate_batch_limits(content_len: usize, g: Granularity) -> BatchLimits {
    let cap = base_cap(g);
    // short-content downgrade: never plan more items than the content can plausibly hold.
    let content_cap = (content_len / CHARS_PER_ITEM).max(1);
    let max_total_items = cap.min(content_cap);
    let items_per_batch = base_items_per_batch(g).min(max_total_items.max(1));
    BatchLimits {
        items_per_batch,
        max_total_items,
        max_tokens: items_per_batch as u32 * TOKENS_PER_ITEM_BUDGET,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    #[test]
    fn table_and_downgrade_match_committed_thresholds() {
        // granularity table (batch-limits.ts:22-28): items-per-batch by granularity. Use content long
        // enough (≥ 15*600) that the short-content downgrade never clamps the base table value — the
        // table is the per-batch size, the downgrade only narrows the cumulative cap (see below).
        let long = 20_000;
        assert_eq!(calculate_batch_limits(long, Granularity::Standard).items_per_batch, 8);
        assert_eq!(calculate_batch_limits(long, Granularity::Fine).items_per_batch, 15);
        assert_eq!(calculate_batch_limits(long, Granularity::Coarse).items_per_batch, 5);
        assert_eq!(calculate_batch_limits(long, Granularity::Minimal).items_per_batch, 3);
        // short-content auto-downgrade: ~1 item per 600 chars (batch-limits.ts:64-69)
        let l = calculate_batch_limits(1200, Granularity::Standard);
        assert_eq!(l.max_total_items, 2); // 1200/600 = 2
        assert!(l.items_per_batch <= l.max_total_items); // per-batch clamped to the short-content cap
        // per-item token budget (constants.ts:52)
        assert_eq!(l.max_tokens, l.items_per_batch as u32 * 400);
    }
}
