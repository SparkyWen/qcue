// QCue S2-R15 — convergence detector (PORT convergence-detector.ts:39-76).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Convergence {
    Continue,
    Halve,
    Stop,
}

#[derive(Debug, Clone, Copy)]
pub struct RoundState {
    pub batch_size: usize,
    pub new_this_round: usize,
    pub already_halved: bool,
    pub cumulative_items: usize,
    pub cap: usize,
    pub empty_or_all_dup: bool,
}

pub fn detect_convergence(s: &RoundState) -> Convergence {
    if s.empty_or_all_dup {
        return Convergence::Stop; // checkEmptyBatch
    }
    if s.cumulative_items >= s.cap {
        return Convergence::Stop; // checkCumulativeLimits
    }
    let low_yield = s.new_this_round * 2 < s.batch_size; // < 50% of batch
    match (low_yield, s.already_halved) {
        (true, true) => Convergence::Stop,  // already halved and still low → stop
        (true, false) => Convergence::Halve, // low → halve
        (false, _) => Convergence::Continue,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    fn s(batch: usize, new: usize, halved: bool, total: usize, cap: usize, empty: bool) -> RoundState {
        RoundState {
            batch_size: batch,
            new_this_round: new,
            already_halved: halved,
            cumulative_items: total,
            cap,
            empty_or_all_dup: empty,
        }
    }
    #[test]
    fn stop_conditions_table() {
        // yield ≥ 50% of batch, room left → Continue
        assert_eq!(detect_convergence(&s(8, 5, false, 5, 50, false)), Convergence::Continue);
        // yield < 50% of batch, not halved → Halve
        assert_eq!(detect_convergence(&s(8, 2, false, 2, 50, false)), Convergence::Halve);
        // yield < 50% AND already halved → Stop
        assert_eq!(detect_convergence(&s(4, 1, true, 3, 50, false)), Convergence::Stop);
        // empty / all-dup batch → Stop (checkEmptyBatch)
        assert_eq!(detect_convergence(&s(8, 0, false, 10, 50, true)), Convergence::Stop);
        // cumulative cap reached → Stop (checkCumulativeLimits)
        assert_eq!(detect_convergence(&s(8, 8, false, 50, 50, false)), Convergence::Stop);
    }
}
