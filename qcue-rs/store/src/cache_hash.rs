// QCue S1-R65 — per-session cache-break self-detection hashes stored in Redis, diffed per call.
use redis::AsyncCommands;

/// djb2 over the serialized component bytes (after serialization, per claude-cc §1.5).
pub fn djb2(s: &str) -> u64 {
    let mut h: u64 = 5381;
    for b in s.as_bytes() {
        h = h.wrapping_mul(33).wrapping_add(*b as u64);
    }
    h
}

pub struct CacheHashes {
    conn: redis::aio::MultiplexedConnection,
}
impl CacheHashes {
    pub async fn connect(url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(url)?;
        Ok(Self { conn: client.get_multiplexed_async_connection().await? })
    }
    /// Store the component hash; return the previous value so the caller can attribute a cache drop.
    pub async fn diff_and_store(
        &self,
        session: &str,
        component: &str,
        value: &str,
    ) -> Result<Option<u64>, redis::RedisError> {
        let mut c = self.conn.clone();
        let key = format!("cachehash:{session}:{component}");
        let prev: Option<u64> = c.get(&key).await?;
        let _: () = c.set(&key, djb2(value)).await?;
        Ok(prev)
    }
}
