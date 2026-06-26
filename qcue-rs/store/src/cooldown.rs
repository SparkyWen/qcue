// QCue S1-R34/R35 — Redis cooldown timers + lease counters, keyed tenant:<id>:cred:<id>.
use redis::AsyncCommands;
use uuid::Uuid;

pub struct Cooldown {
    conn: redis::aio::MultiplexedConnection,
}
impl Cooldown {
    pub async fn connect(url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(url)?;
        let conn = client.get_multiplexed_async_connection().await?;
        Ok(Self { conn })
    }
    fn key(tenant: Uuid, cred: Uuid) -> String {
        format!("tenant:{tenant}:cred:{cred}:cooldown")
    }

    pub async fn set_cooldown(&self, tenant: Uuid, cred: Uuid, secs: u64) -> Result<(), redis::RedisError> {
        let mut c = self.conn.clone();
        let _: () = c.set_ex(Self::key(tenant, cred), 1, secs).await?;
        Ok(())
    }
    pub async fn get_cooldown(&self, tenant: Uuid, cred: Uuid) -> Result<Option<i64>, redis::RedisError> {
        let mut c = self.conn.clone();
        let ttl: i64 = c.ttl(Self::key(tenant, cred)).await?;
        Ok(if ttl > 0 { Some(ttl) } else { None })
    }
    /// S1-R35 — atomic lease INCR/DECR with TTL.
    pub async fn acquire_lease(&self, tenant: Uuid, cred: Uuid, max: i64) -> Result<bool, redis::RedisError> {
        let mut c = self.conn.clone();
        let k = format!("tenant:{tenant}:cred:{cred}:lease");
        let n: i64 = c.incr(&k, 1).await?;
        let _: () = c.expire(&k, 120).await?;
        if n > max {
            let _: () = c.decr(&k, 1).await?;
            Ok(false)
        } else {
            Ok(true)
        }
    }
}
