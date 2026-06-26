//! QCue v0.1.1 — the single-use magic-link token store: Redis-backed (cross-worker) when `REDIS_URL`
//! is set, else a process-local in-memory map (single-worker dev/tests). Same contract either way:
//! a token is single-use (GETDEL / map-remove) and TTL'd to 15 minutes. The Redis form keys
//! `{prefix}:magic:{token}` with `SET … EX` + a `GETDEL` one-shot consume, so a token issued on one
//! worker verifies on another (the cross-worker fan-out the single-process map could not provide).
use redis::AsyncCommands;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::OnceCell;
use uuid::Uuid;

/// The backing store, chosen once per process from the environment.
pub enum MagicStore {
    Memory(Mutex<HashMap<String, (Uuid, Uuid, Instant)>>),
    Redis { conn: redis::aio::MultiplexedConnection, prefix: String },
}

static STORE: OnceCell<MagicStore> = OnceCell::const_new();

/// The process-wide magic store, initialized on first use: Redis when `REDIS_URL` is set + reachable,
/// otherwise the in-memory map. A Redis connect failure falls back to memory (never blocks auth).
pub async fn store() -> &'static MagicStore {
    STORE
        .get_or_init(|| async {
            let url = std::env::var("REDIS_URL").unwrap_or_default();
            if !url.is_empty()
                && let Ok(client) = redis::Client::open(url)
                && let Ok(conn) = client.get_multiplexed_async_connection().await
            {
                let prefix = std::env::var("QCUE_REDIS_PREFIX").unwrap_or_else(|_| "qcue".into());
                tracing::info!("magic-link store: Redis (cross-worker)");
                return MagicStore::Redis { conn, prefix };
            }
            tracing::info!("magic-link store: in-memory (single-worker; set REDIS_URL for cross-worker)");
            MagicStore::Memory(Mutex::new(HashMap::new()))
        })
        .await
}

impl MagicStore {
    /// An explicit in-memory store (tests construct this directly to avoid the env/global).
    pub fn memory() -> Self {
        MagicStore::Memory(Mutex::new(HashMap::new()))
    }

    fn key(prefix: &str, token: &str) -> String {
        format!("{prefix}:magic:{token}")
    }

    /// Store a single-use token → (tenant, user) with a TTL.
    pub async fn put(&self, token: &str, tenant: Uuid, user: Uuid, ttl: Duration) {
        match self {
            MagicStore::Memory(m) => {
                if let Ok(mut s) = m.lock() {
                    s.insert(token.to_string(), (tenant, user, Instant::now() + ttl));
                }
            }
            MagicStore::Redis { conn, prefix } => {
                let mut c = conn.clone();
                let val = format!("{tenant}:{user}");
                // best-effort: a Redis hiccup must not 500 the magic-request (it 200s either way).
                let _: Result<(), _> = c.set_ex(Self::key(prefix, token), val, ttl.as_secs()).await;
            }
        }
    }

    /// Consume a token EXACTLY once (single-use). Returns (tenant, user) or None (unknown/expired/used).
    pub async fn consume(&self, token: &str) -> Option<(Uuid, Uuid)> {
        match self {
            MagicStore::Memory(m) => {
                let mut s = m.lock().ok()?;
                let (t, u, exp) = s.remove(token)?; // remove = single-use
                if exp < Instant::now() {
                    return None;
                }
                Some((t, u))
            }
            MagicStore::Redis { conn, prefix } => {
                let mut c = conn.clone();
                // GETDEL is the atomic one-shot consume (get + delete): a second verify finds nothing.
                let val: Option<String> = c.get_del(Self::key(prefix, token)).await.ok()?;
                let (t, u) = val?.split_once(':').map(|(a, b)| (a.to_string(), b.to_string()))?;
                Some((Uuid::parse_str(&t).ok()?, Uuid::parse_str(&u).ok()?))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[tokio::test]
    async fn memory_store_is_single_use() {
        let s = MagicStore::memory();
        let (t, u) = (Uuid::now_v7(), Uuid::now_v7());
        s.put("mgc_x", t, u, Duration::from_secs(900)).await;
        assert_eq!(s.consume("mgc_x").await, Some((t, u)), "first consume returns the ids");
        assert_eq!(s.consume("mgc_x").await, None, "second consume → None (single-use)");
        assert_eq!(s.consume("mgc_unknown").await, None, "unknown token → None");
    }

    // Cross-worker proof: requires a reachable Redis (gated on QCUE_TEST_REDIS, like the store crate).
    #[tokio::test]
    async fn redis_store_is_single_use_and_cross_connection() {
        let Ok(url) = std::env::var("QCUE_TEST_REDIS") else {
            eprintln!("skipping: QCUE_TEST_REDIS not set");
            return;
        };
        let client = redis::Client::open(url).unwrap();
        let prefix = format!("qcuetest:{}", Uuid::now_v7().simple());
        let mk = || async {
            MagicStore::Redis {
                conn: client.get_multiplexed_async_connection().await.unwrap(),
                prefix: prefix.clone(),
            }
        };
        let (t, u) = (Uuid::now_v7(), Uuid::now_v7());
        let token = format!("mgc_{}", Uuid::now_v7().simple());
        // issue on one connection…
        mk().await.put(&token, t, u, Duration::from_secs(900)).await;
        // …verify on a DIFFERENT connection (the cross-worker case).
        assert_eq!(mk().await.consume(&token).await, Some((t, u)), "verifies across connections");
        assert_eq!(mk().await.consume(&token).await, None, "GETDEL makes it single-use across workers");
    }
}
