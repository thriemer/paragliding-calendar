use std::{
    fmt::Debug,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Result, anyhow};
use fjall::{Iter, Keyspace};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::task;

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
    pub fn from_keyspace(keyspace: Keyspace) -> Self {
        PersistentCache { store: keyspace }
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
                Ok(Some(entry.value))
            } else {
                self.remove(key).await?;
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    pub async fn get_all_starting_with<T: DeserializeOwned + Send + 'static>(
        &self,
        key: &str,
    ) -> Result<Vec<T>> {
        let store = self.store.clone();
        let key_bytes = key.as_bytes().to_vec();
        let maybe_bytes: Iter = task::spawn_blocking(move || store.prefix(key_bytes)).await?;
        let result = maybe_bytes
            .filter_map(|pair| pair.value().ok())
            .filter_map(|bytes| {
                let entry: postcard::Result<StoredEntry<T>> = postcard::from_bytes(&bytes);
                let entry = entry.ok()?;
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                if now < entry.expires_at {
                    Some(entry.value)
                } else {
                    None
                }
            })
            .collect::<Vec<T>>();
        Ok(result)
    }

    pub async fn remove(&self, key: &str) -> Result<()> {
        let key = key.as_bytes().to_vec();
        let store = self.store.clone();
        let _ = task::spawn_blocking(move || store.remove(key)).await?;
        Ok(())
    }
}
