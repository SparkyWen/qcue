// QCue S1-R43..R46 — the retry loop is a single match on the 4 bits; it never re-classifies.
use protocol::{ApiMode, ClassifiedError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Rotate,
    Fallback,
    Compress,
    Backoff,
    Abort,
}

/// S1-R43 — map the 4 action bits to exactly one branch.
pub fn decide_action(ce: &ClassifiedError) -> Action {
    match (
        ce.should_rotate_credential,
        ce.should_fallback,
        ce.should_compress,
        ce.retryable,
    ) {
        (true, _, _, _) => Action::Rotate,
        (_, true, _, _) => Action::Fallback,
        (_, _, true, _) => Action::Compress,
        (false, false, false, true) => Action::Backoff,
        _ => Action::Abort,
    }
}

/// The provider/model/api_mode fallback chain. `advance` RE-DERIVES api_mode (S1-R44, pitfall #20).
pub struct FallbackChain {
    links: Vec<(String, String, ApiMode)>, // (provider, model, api_mode)
    idx: usize,
}
impl FallbackChain {
    pub fn new(links: Vec<(String, String, ApiMode)>) -> Self {
        Self { links, idx: 0 }
    }
    pub fn current(&self) -> &(String, String, ApiMode) {
        &self.links[self.idx]
    }
    /// advance to the next provider; the returned link carries the new wire's api_mode.
    pub fn advance(&mut self) -> Option<&(String, String, ApiMode)> {
        if self.idx + 1 < self.links.len() {
            self.idx += 1;
            Some(&self.links[self.idx])
        } else {
            None
        }
    }
}
