// QCue A-R3/A-R9/A-R10/A-R11 — the cheapest-gate-first ladder (App. A §2.3): the first failing gate
// short-circuits, so the per-tick cost stays near-zero (one config read + one indexed single-row read,
// A-R3). The session gate uses the LIVE `IdeasRepo::captures_since` COUNT (AUTHORITATIVE, A-R10) —
// `wiki_consolidation.sessions_since_last` is cached telemetry ONLY and MUST NOT drive the decision.
//
// The harness-driven `DreamAgent` (the run_turn fork) lives in the `ideas` crate; the scheduler reaches
// it through the `DreamRunner` seam so `wiki` stays provider-agnostic (no router reach). On Ok →
// release (clock stays advanced); on cancel → no-op (kill already rolled back); on Err → rollback
// (clock rewound, the scan-throttle is the backoff).
use crate::dream::lock::ConsolidationLock;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Dream tuning. Defaults are Claude's: 24h time-gate, 5 sessions, 10-min scan throttle (App. A §2.7).
/// Per-tenant overrides (tenants.dream_min_hours/min_sessions) are validated defensively by the caller.
#[derive(Debug, Clone, Copy)]
pub struct DreamConfig {
    pub min_hours: f64,
    pub min_sessions: i64,
    pub scan_throttle: Duration,
}
impl Default for DreamConfig {
    fn default() -> Self {
        Self { min_hours: 24.0, min_sessions: 5, scan_throttle: Duration::from_secs(600) }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum GateReason {
    Disabled,
    TooSoon,
    Throttled,
    TooFewSessions,
    Locked,
}
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Gate {
    Pass,
    Stop(GateReason),
}

/// The inputs to the pure gate ladder (computed cheapest-first by the scheduler).
pub struct GateInputs {
    pub enabled: bool,
    pub hours_since: f64,
    pub throttle_ok: bool,
    pub captures_since: i64,
}

/// Pure gate evaluation: the first failing gate short-circuits (no later gate is considered). This is
/// the exact ascending-cost order from `autoDream.ts:5-9`.
pub fn eval_gate(g: &GateInputs, cfg: &DreamConfig) -> Gate {
    if !g.enabled {
        return Gate::Stop(GateReason::Disabled);
    }
    if g.hours_since < cfg.min_hours {
        return Gate::Stop(GateReason::TooSoon);
    }
    if !g.throttle_ok {
        return Gate::Stop(GateReason::Throttled);
    }
    if g.captures_since < cfg.min_sessions {
        return Gate::Stop(GateReason::TooFewSessions);
    }
    Gate::Pass
}

/// What a Dream run produced (files-touched feed the "Improved N pages" report).
#[derive(Debug, Clone, Default)]
pub struct DreamOutcome {
    pub files_touched: Vec<String>,
    pub turns: u32,
}

/// The session-gate count source (the LIVE authoritative COUNT, A-R10). `IdeasRepo` implements it.
#[async_trait]
pub trait CapturesSince: Send + Sync {
    async fn captures_since(
        &self,
        tenant: Uuid,
        since: DateTime<Utc>,
        current_session: Uuid,
    ) -> anyhow::Result<i64>;
}

// `store::IdeasRepo` is the canonical authoritative count (A-R10). The trait is local to `wiki`, so the
// impl over the foreign repo is allowed by the orphan rule (and `store` never imports `wiki`).
#[async_trait]
impl CapturesSince for store::ideas_repo::IdeasRepo {
    async fn captures_since(
        &self,
        tenant: Uuid,
        since: DateTime<Utc>,
        current_session: Uuid,
    ) -> anyhow::Result<i64> {
        Ok(store::ideas_repo::IdeasRepo::captures_since(self, tenant, since, current_session).await?)
    }
}

/// The harness-driven agent seam. The concrete `DreamAgent` (in `ideas`) implements this — it drives
/// `router::run_turn` with the dream tool policy. Keeping it a trait keeps `wiki` provider-agnostic.
#[async_trait]
pub trait DreamRunner: Send + Sync {
    /// Run the read-only fork over the tenant's wiki since `since`. Errs on cost-ceiling / failure
    /// (→ the scheduler rolls back the clock). `cancel` is the user kill switch.
    async fn run(
        &self,
        tenant: Uuid,
        user: Uuid,
        since: DateTime<Utc>,
        cancel: CancellationToken,
    ) -> anyhow::Result<DreamOutcome>;
}

/// The per-tenant scheduler. Holds the lock-as-clock, the live session-count source, the agent seam,
/// and the in-memory scan-throttle map (persisted as `last_scan_at` for the on-device path).
pub struct DreamScheduler<L: ConsolidationLock, C: CapturesSince, R: DreamRunner> {
    lock: L,
    captures: C,
    runner: R,
    cfg: DreamConfig,
    last_scan: Mutex<HashMap<Uuid, Instant>>,
}

impl<L: ConsolidationLock, C: CapturesSince, R: DreamRunner> DreamScheduler<L, C, R> {
    pub fn new(lock: L, captures: C, runner: R) -> Self {
        Self {
            lock,
            captures,
            runner,
            cfg: DreamConfig::default(),
            last_scan: Mutex::new(HashMap::new()),
        }
    }
    pub fn with_config(mut self, cfg: DreamConfig) -> Self {
        self.cfg = cfg;
        self
    }

    /// `dream_due(tenant) -> bool` — the cheap pre-check the S3 cron will call before enqueueing a job
    /// (the cheap gates only: enabled + time + throttle; NOT the count/lock). Leaves the S3 enqueue
    /// wiring as a seam (the S3-finish milestone owns the cron tick → enqueue).
    pub async fn dream_due(&self, tenant: Uuid) -> anyhow::Result<bool> {
        if !dream_enabled() {
            return Ok(false);
        }
        let last = self.lock.read_clock(tenant).await?;
        let hours_since = (Utc::now() - last).num_seconds() as f64 / 3600.0;
        Ok(hours_since >= self.cfg.min_hours && self.scan_throttle_ok(tenant))
    }

    /// The cheapest-gate-first ladder (App. A §2.3). Returns `Ok(None)` if any gate stops.
    pub async fn try_dream(
        &self,
        tenant: Uuid,
        user: Uuid,
        current_session: Uuid,
        cancel: CancellationToken,
    ) -> anyhow::Result<Option<DreamOutcome>> {
        let enabled = dream_enabled(); // pitfall #16 — DREAM_ENABLED=false gates workers off in dev
        let last = self.lock.read_clock(tenant).await?; // gate 1 input (the one indexed single-row read, A-R3)
        let hours_since = (Utc::now() - last).num_seconds() as f64 / 3600.0;
        let throttle_ok = self.scan_throttle_ok(tenant);
        // Evaluate the cheap gates (enabled → time → throttle) BEFORE the count query (captures_since is
        // the expensive gate; pass i64::MAX as a sentinel so only the cheap gates can stop here).
        if let Gate::Stop(_) = eval_gate(
            &GateInputs { enabled, hours_since, throttle_ok, captures_since: i64::MAX },
            &self.cfg,
        ) {
            return Ok(None);
        }
        // A-R11 — stamp the scan BEFORE the count query (matches autoDream.ts:151).
        self.stamp_scan(tenant);
        // gate 3 — the LIVE authoritative session COUNT (A-R10), excluding the current session.
        let n = self.captures.captures_since(tenant, last, current_session).await?;
        if let Gate::Stop(_) = eval_gate(
            &GateInputs { enabled, hours_since, throttle_ok: true, captures_since: n },
            &self.cfg,
        ) {
            return Ok(None);
        }
        // gate 4 — acquire the lock-as-clock (advances the clock; returns the prior for rollback).
        let prior = match self.lock.try_acquire(tenant, "dream-worker").await? {
            Some(p) => p,
            None => return Ok(None), // a live unexpired holder blocked us
        };
        match self.runner.run(tenant, user, last, cancel.clone()).await {
            Ok(o) => {
                self.lock.release(tenant).await?; // clock stays advanced (A-R7)
                Ok(Some(o))
            }
            Err(_) if cancel.is_cancelled() => Ok(None), // kill already rolled back
            Err(e) => {
                self.lock.rollback(tenant, prior).await?; // A-R8 rewind; the scan-throttle is the backoff
                Err(e)
            }
        }
    }

    fn scan_throttle_ok(&self, tenant: Uuid) -> bool {
        self.last_scan
            .lock()
            .map(|m| {
                m.get(&tenant)
                    .map(|t| t.elapsed() >= self.cfg.scan_throttle)
                    .unwrap_or(true)
            })
            .unwrap_or(true)
    }
    fn stamp_scan(&self, tenant: Uuid) {
        if let Ok(mut m) = self.last_scan.lock() {
            m.insert(tenant, Instant::now());
        }
    }
}

/// The `DREAM_ENABLED` dev gate (pitfall #16): unset → enabled; `=false` → disabled.
fn dream_enabled() -> bool {
    std::env::var("DREAM_ENABLED").map(|v| v != "false").unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_ladder_short_circuits_in_order() {
        let cfg = DreamConfig::default();
        // disabled → Stop(Disabled), no later gate considered
        assert_eq!(
            eval_gate(
                &GateInputs { enabled: false, hours_since: 100.0, throttle_ok: true, captures_since: 100 },
                &cfg
            ),
            Gate::Stop(GateReason::Disabled)
        );
        // enabled but too soon → Stop(TooSoon)
        assert_eq!(
            eval_gate(
                &GateInputs { enabled: true, hours_since: 1.0, throttle_ok: true, captures_since: 100 },
                &cfg
            ),
            Gate::Stop(GateReason::TooSoon)
        );
        // throttled
        assert_eq!(
            eval_gate(
                &GateInputs { enabled: true, hours_since: 100.0, throttle_ok: false, captures_since: 100 },
                &cfg
            ),
            Gate::Stop(GateReason::Throttled)
        );
        // too few sessions
        assert_eq!(
            eval_gate(
                &GateInputs { enabled: true, hours_since: 100.0, throttle_ok: true, captures_since: 2 },
                &cfg
            ),
            Gate::Stop(GateReason::TooFewSessions)
        );
        // all pass → Pass
        assert_eq!(
            eval_gate(
                &GateInputs { enabled: true, hours_since: 100.0, throttle_ok: true, captures_since: 5 },
                &cfg
            ),
            Gate::Pass
        );
    }
}
