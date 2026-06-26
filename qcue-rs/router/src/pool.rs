// QCue S1-R30..R37 — 3-state credential pool. now_ms injected for deterministic tests.
use protocol::CredStatus;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolStrategy {
    FillFirst,
    RoundRobin,
    Random,
    LeastUsed,
}

#[derive(Clone, Debug)]
pub struct PooledCredential {
    pub id: Uuid,
    pub label: Option<String>,
    pub priority: i32,
    pub status: CredStatus,
    pub key_hint: String,
    pub last_error_code: Option<u16>,
    pub last_error_reason: Option<String>,
    pub request_count: u64,
}

pub struct CredentialPool {
    entries: Vec<PooledCredential>,
    strategy: PoolStrategy,
    rr_cursor: usize,
    leases: HashMap<Uuid, u32>, // active lease count per credential
    max_concurrent_per_credential: u32, // default 1 (S1-R35)
    dead_at: HashMap<String, i64>, // key_hint → dead_at_ms (S1-R37)
}

/// S1-R32 — TTL policy in ms (self-heal posture): 401→30s, 429→60s, default→60s.
pub fn ttl_for_status(status: u16) -> i64 {
    match status {
        401 => 30 * 1000,
        429 => 60 * 1000,
        _ => 60 * 1000,
    }
}

/// S1-R33 — no single error can park a credential longer than this (5 min), regardless of the
/// provider's Retry-After hint. Bounds the worst-case "stuck cooling" the operator reported.
pub const MAX_COOLDOWN_MS: i64 = 5 * 60 * 1000;

impl CredentialPool {
    pub fn new(entries: Vec<PooledCredential>, strategy: PoolStrategy) -> Self {
        Self {
            entries,
            strategy,
            rr_cursor: 0,
            leases: HashMap::new(),
            max_concurrent_per_credential: 1,
            dead_at: HashMap::new(),
        }
    }

    fn eligible(&self, c: &PooledCredential, now_ms: i64) -> bool {
        let status_ok = match c.status {
            CredStatus::Ok => true,
            CredStatus::Exhausted { until_ms } => now_ms >= until_ms,
            CredStatus::Dead => false,
        };
        let lease_ok =
            self.leases.get(&c.id).copied().unwrap_or(0) < self.max_concurrent_per_credential;
        status_ok && lease_ok
    }

    /// Select the next eligible credential per strategy (skips Dead + cooled + leased-to-cap).
    pub fn select(&mut self, now_ms: i64) -> Option<&PooledCredential> {
        let mut idxs: Vec<usize> = (0..self.entries.len())
            .filter(|&i| self.eligible(&self.entries[i], now_ms))
            .collect();
        if idxs.is_empty() {
            return None;
        }
        let chosen = match self.strategy {
            PoolStrategy::FillFirst => {
                idxs.sort_by_key(|&i| self.entries[i].priority);
                idxs[0]
            }
            PoolStrategy::LeastUsed => {
                idxs.sort_by_key(|&i| self.entries[i].request_count);
                idxs[0]
            }
            PoolStrategy::Random => idxs[(now_ms as usize).wrapping_add(self.rr_cursor) % idxs.len()],
            PoolStrategy::RoundRobin => {
                let i = idxs[self.rr_cursor % idxs.len()];
                self.rr_cursor += 1;
                i
            }
        };
        Some(&self.entries[chosen])
    }

    pub fn find(&self, hint: &str) -> Option<&PooledCredential> {
        self.entries.iter().find(|c| c.key_hint == hint)
    }
    fn find_mut(&mut self, hint: &str) -> Option<&mut PooledCredential> {
        self.entries.iter_mut().find(|c| c.key_hint == hint)
    }

    /// S1-R33 — penalize the credential identified by key_hint (the actual failer under concurrency).
    pub fn mark_exhausted_and_rotate(
        &mut self,
        status: u16,
        reset_at_ms: Option<i64>,
        key_hint: Option<&str>,
        now_ms: i64,
    ) -> Option<&PooledCredential> {
        let until =
            reset_at_ms.unwrap_or(now_ms + ttl_for_status(status)).min(now_ms + MAX_COOLDOWN_MS);
        if let Some(hint) = key_hint
            && let Some(c) = self.find_mut(hint)
        {
            c.status = CredStatus::Exhausted { until_ms: until };
            c.last_error_code = Some(status);
        }
        self.select(now_ms)
    }

    /// S1-R35 — a successful call on this credential heals it back to Ok (clears any cooldown). A
    /// Dead cred stays Dead — terminal-auth is not undone by a stray success (S1-R31). Returns `true`
    /// ONLY when this call actually healed an `Exhausted` credential to `Ok` (a real transition worth
    /// persisting). Returns `false` when the cred was already `Ok` (in-memory no-op — no DB round-trip
    /// needed, the common per-success case) or `Dead` (never resurrected).
    pub fn mark_ok(&mut self, key_hint: &str) -> bool {
        if let Some(c) = self.find_mut(key_hint)
            && matches!(c.status, CredStatus::Exhausted { .. })
        {
            c.status = CredStatus::Ok;
            c.last_error_code = None;
            return true;
        }
        false
    }

    /// S1-R31 — terminal-auth failures go Dead (never re-enter rotation).
    pub fn mark_dead(&mut self, key_hint: &str) {
        if let Some(c) = self.find_mut(key_hint) {
            c.status = CredStatus::Dead;
        }
    }
    pub fn set_dead_at(&mut self, key_hint: &str, now_ms: i64) {
        self.dead_at.insert(key_hint.into(), now_ms);
    }

    /// S1-R37 — prune Dead creds older than 24h.
    pub fn prune_dead(&mut self, now_ms: i64) {
        let dead_at = &self.dead_at;
        self.entries.retain(|c| {
            if c.status != CredStatus::Dead {
                return true;
            }
            match dead_at.get(&c.key_hint) {
                Some(&d) => now_ms < d + 24 * 3_600_000,
                None => true,
            }
        });
    }

    /// S1-R35 — soft lease counters.
    pub fn acquire_lease(&mut self, id: Option<Uuid>) -> Option<Uuid> {
        let id = id?;
        let count = self.leases.entry(id).or_insert(0);
        if *count >= self.max_concurrent_per_credential {
            return None;
        }
        *count += 1;
        Some(id)
    }
    pub fn release_lease(&mut self, id: Uuid) {
        if let Some(c) = self.leases.get_mut(&id) {
            *c = c.saturating_sub(1);
        }
    }
}
