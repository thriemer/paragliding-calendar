use anyhow::{Result, anyhow};
use fjall::Keyspace;
use serde::Deserialize;
use serde::{Serialize, de::DeserializeOwned};
use std::fmt::Debug;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::OnceCell;
use tokio::task;

static GLOBAL_CACHE: OnceCell<PersistentCache> = OnceCell::const_new();

#[derive(Serialize, Deserialize)]
struct StoredEntry<T> {
    value: T,
    expires_at: u64, // Unix timestamp (seconds)
}

pub struct PersistentCache {
    store: Keyspace,
}

fn get_from_store(store: Keyspace, key: Vec<u8>) -> anyhow::Result<Option<Vec<u8>>> {
    Ok(store.get(key)?.map(|v| v.to_vec()))
}
impl PersistentCache {
    fn new(path: impl AsRef<Path>) -> Result<Self> {
        let db = fjall::Database::builder(&path).open()?;
        let items = db.keyspace("cache", fjall::KeyspaceCreateOptions::default)?;
        Ok(PersistentCache { store: items })
    }

    /// Stores a serializable value with a time-to-live (TTL).
    #[tracing::instrument(name = "put_cache", level = "debug", skip(self))]
    pub async fn put<T: Serialize + Send + Debug + 'static>(
        &self,
        key: &str,
        value: T,
        ttl: Duration,
    ) -> Result<()> {
        let store = self.store.clone();
        let key = key.as_bytes().to_vec();
        // Calculate expiry time
        let expires_at = SystemTime::now()
            .checked_add(ttl)
            .ok_or(anyhow!("TTL overflow"))?
            .duration_since(UNIX_EPOCH)?
            .as_secs();
        let entry = StoredEntry { value, expires_at };
        let bytes = postcard::to_stdvec(&entry)?;

        let _ = task::spawn_blocking(move || store.insert(key, bytes)).await?;
        Ok(())
    }

    /// Retrieves a value if it exists and has not expired.
    /// Returns `None` for cache misses or expired entries.
    #[tracing::instrument(name = "query_cache", level = "debug", skip(self))]
    pub async fn get<T: DeserializeOwned + Send + 'static>(&self, key: &str) -> Result<Option<T>> {
        let store = self.store.clone();
        let key_bytes = key.as_bytes().to_vec();

        let maybe_bytes: Option<Vec<u8>> =
            task::spawn_blocking(move || get_from_store(store, key_bytes)).await??;

        if let Some(bytes) = maybe_bytes {
            let entry: StoredEntry<T> = postcard::from_bytes(&bytes)?;
            let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

            if now < entry.expires_at {
                tracing::debug!("Key found and still fresh");
                // Fresh
                Ok(Some(entry.value))
            } else {
                tracing::debug!("Key found but expired");
                self.remove(key).await?;
                Ok(None)
            }
        } else {
            tracing::debug!("Key not found");
            // Key not found
            Ok(None)
        }
    }

    /// Manually removes a key from the cache.
    pub async fn remove(&self, key: &str) -> Result<()> {
        let key = key.as_bytes().to_vec();
        let store = self.store.clone();
        let _ = task::spawn_blocking(move || store.remove(key)).await?;
        Ok(())
    }
}

/// Initializes the global persistent cache. **Must be called once before use.**
pub fn init(path: impl AsRef<Path>) -> Result<()> {
    let cache = PersistentCache::new(path)?;
    GLOBAL_CACHE
        .set(cache)
        .map_err(|_| anyhow!("Cache already initialized"))?;
    Ok(())
}

/// Returns a reference to the globally initialized cache.
/// # Panics
/// Panics if the cache has not been initialized by calling `cache::init().await` first.
fn get_cache() -> &'static PersistentCache {
    GLOBAL_CACHE
        .get()
        .expect("Cache not initialized. Call cache::init().await first.")
}

// Public, ergonomic API endpoints that use the global cache.
pub async fn put<T: Serialize + Send + Debug + 'static>(key: &str, value: T, ttl: Duration) -> Result<()> {
    get_cache().put(key, value, ttl).await
}

pub async fn get<T: DeserializeOwned + Send + 'static>(key: &str) -> Result<Option<T>> {
    get_cache().get(key).await
}

pub async fn remove(key: &str) -> Result<()> {
    get_cache().remove(key).await
}
