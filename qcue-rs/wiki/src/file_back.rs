// QCue S2-R25 — file-the-answer-back dedup by query hash (lastOfferedQueryHash, query-engine.ts:256).
//
// When a query is synthesized, the answer becomes a new wiki page so explorations COMPOUND (the
// file-answer-back loop). This tracker dedups the OFFER by a normalized query hash so the same Q&A is
// not re-offered to be filed twice in a session. The actual page write goes through the single
// write-gate (`WikiWriteGate::write_page`); this is just the should-offer gate.
use sha2::{Digest, Sha256};
use std::collections::HashSet;

#[derive(Default)]
pub struct FileBackTracker {
    offered: HashSet<String>,
}

impl FileBackTracker {
    fn hash(q: &str) -> String {
        format!("{:x}", Sha256::digest(q.trim().to_lowercase().as_bytes()))
    }
    /// True the first time a query is seen (offer to file the answer back); false once marked.
    pub fn should_offer(&self, query: &str) -> bool {
        !self.offered.contains(&Self::hash(query))
    }
    /// Record that this query's answer was offered (so the same Q&A is not re-offered).
    pub fn mark_offered(&mut self, query: &str) {
        self.offered.insert(Self::hash(query));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    #[test]
    fn dedups_by_query_hash_so_same_qa_not_reoffered() {
        let mut tracker = FileBackTracker::default();
        let q = "What is Rust?";
        assert!(tracker.should_offer(q)); // first time → offer
        tracker.mark_offered(q);
        assert!(!tracker.should_offer(q)); // same query (normalized) → not re-offered
        // normalization: case + surrounding whitespace fold to the same hash.
        assert!(!tracker.should_offer("  what is rust?  "));
        assert!(tracker.should_offer("Different question?"));
    }
}
