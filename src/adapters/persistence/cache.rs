use std::{
    fmt::Debug,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use serde::{Serialize, de::DeserializeOwned};
use sqlx::PgPool;

pub struct PersistentCache {
    pool: PgPool,
}

impl PersistentCache {
    pub fn new(pool: PgPool) -> Self {
        PersistentCache { pool }
    }

    #[tracing::instrument(name = "put_cache", level = "debug", skip(self))]
    pub async fn put<T: Serialize + Send + Debug + 'static>(
        &self,
        key: &str,
        value: T,
        ttl: Duration,
    ) -> Result<()> {
        let expires_at = SystemTime::now()
            .checked_add(ttl)
            .ok_or_else(|| anyhow::anyhow!("TTL overflow"))?
            .duration_since(UNIX_EPOCH)?
            .as_secs() as i64;
        let json = serde_json::to_string(&value)?;

        sqlx::query(
            "INSERT INTO cache (key, value, expires_at) VALUES ($1, $2, $3)
             ON CONFLICT (key) DO UPDATE SET value = $2, expires_at = $3",
        )
        .bind(key)
        .bind(&json)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    #[tracing::instrument(name = "query_cache", level = "debug", skip(self))]
    pub async fn get<T: DeserializeOwned + Send + 'static>(&self, key: &str) -> Result<Option<T>> {
        let row: Option<(String, i64)> =
            sqlx::query_as("SELECT value, expires_at FROM cache WHERE key = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;

        match row {
            Some((json, expires_at)) => {
                let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
                if now < expires_at {
                    Ok(Some(serde_json::from_str(&json)?))
                } else {
                    self.remove(key).await?;
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    pub async fn remove(&self, key: &str) -> Result<()> {
        sqlx::query("DELETE FROM cache WHERE key = $1")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_pool;

    #[tokio::test]
    async fn put_then_get_within_ttl_returns_value() {
        let cache = PersistentCache::new(test_pool().await);
        cache
            .put("k", 42u32, Duration::from_secs(60))
            .await
            .unwrap();
        let got: Option<u32> = cache.get("k").await.unwrap();
        assert_eq!(got, Some(42));
    }

    #[tokio::test]
    async fn get_missing_key_returns_none() {
        let cache = PersistentCache::new(test_pool().await);
        let got: Option<u32> = cache.get("missing").await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn get_after_ttl_expiry_returns_none() {
        let cache = PersistentCache::new(test_pool().await);
        cache
            .put("k", 42u32, Duration::from_millis(100))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(1100)).await;
        let got: Option<u32> = cache.get("k").await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn zero_ttl_treats_entry_as_already_expired() {
        let cache = PersistentCache::new(test_pool().await);
        cache.put("k", 42u32, Duration::ZERO).await.unwrap();
        let got: Option<u32> = cache.get("k").await.unwrap();
        assert!(got.is_none(), "expires_at == now should be expired (strict <)");
    }

    #[tokio::test]
    async fn remove_actually_deletes_the_entry() {
        let cache = PersistentCache::new(test_pool().await);
        cache
            .put("k", 42u32, Duration::from_secs(60))
            .await
            .unwrap();
        cache.remove("k").await.unwrap();
        let got: Option<u32> = cache.get("k").await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn put_overwrites_existing_entry_and_resets_ttl() {
        let cache = PersistentCache::new(test_pool().await);
        cache
            .put("k", 1u32, Duration::from_secs(60))
            .await
            .unwrap();
        cache
            .put("k", 2u32, Duration::from_secs(60))
            .await
            .unwrap();
        let got: Option<u32> = cache.get("k").await.unwrap();
        assert_eq!(got, Some(2));
    }
}
