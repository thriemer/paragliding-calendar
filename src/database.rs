use std::{fmt::Debug, path::Path};

use anyhow::Result;
use async_trait::async_trait;
use fjall::{Iter, Keyspace};
use serde::{Serialize, de::DeserializeOwned};
use tokio::task;

#[async_trait]
pub trait DbProvider: Send + Sync {
    async fn save(&self, key: &str, value: Vec<u8>) -> Result<()>;
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    async fn find_by_prefix(&self, prefix: &str) -> Result<Vec<Vec<u8>>>;
    async fn delete(&self, key: &str) -> Result<()>;
}

pub struct Database {
    store: Keyspace,
}

fn get_from_store(store: Keyspace, key: Vec<u8>) -> anyhow::Result<Option<Vec<u8>>> {
    Ok(store.get(key)?.map(|v| v.to_vec()))
}

impl Database {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let db = fjall::Database::builder(&path).open()?;
        let items = db.keyspace("data", fjall::KeyspaceCreateOptions::default)?;
        Ok(Database { store: items })
    }

    #[tracing::instrument(name = "db_save", level = "debug", skip(self))]
    pub async fn save<T: Serialize + Send + Debug + 'static>(
        &self,
        key: &str,
        value: T,
    ) -> Result<()> {
        let store = self.store.clone();
        let key = key.as_bytes().to_vec();
        let bytes = postcard::to_stdvec(&value)?;

        let _ = task::spawn_blocking(move || store.insert(key, bytes)).await?;
        Ok(())
    }

    #[tracing::instrument(name = "db_get", level = "debug", skip(self))]
    pub async fn get<T: DeserializeOwned + Send + 'static>(&self, key: &str) -> Result<Option<T>> {
        let store = self.store.clone();
        let key_bytes = key.as_bytes().to_vec();

        let maybe_bytes: Option<Vec<u8>> =
            task::spawn_blocking(move || get_from_store(store, key_bytes)).await??;

        if let Some(bytes) = maybe_bytes {
            let value: T = postcard::from_bytes(&bytes)?;
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    #[tracing::instrument(name = "db_find_by_prefix", level = "debug", skip(self))]
    pub async fn find_by_prefix<T: DeserializeOwned + Send + 'static>(
        &self,
        prefix: &str,
    ) -> Result<Vec<T>> {
        let store = self.store.clone();
        let prefix_bytes = prefix.as_bytes().to_vec();
        let iter: Iter = task::spawn_blocking(move || store.prefix(prefix_bytes)).await?;
        let result = iter
            .filter_map(|pair| pair.value().ok())
            .filter_map(|bytes| postcard::from_bytes(&bytes).ok())
            .collect::<Vec<T>>();
        Ok(result)
    }

    #[tracing::instrument(name = "db_delete", level = "debug", skip(self))]
    pub async fn delete(&self, key: &str) -> Result<()> {
        let key = key.as_bytes().to_vec();
        let store = self.store.clone();
        let _ = task::spawn_blocking(move || store.remove(key)).await?;
        Ok(())
    }
}

#[async_trait]
impl DbProvider for Database {
    async fn save(&self, key: &str, value: Vec<u8>) -> Result<()> {
        let store = self.store.clone();
        let key = key.as_bytes().to_vec();

        let _ = task::spawn_blocking(move || store.insert(key, value)).await?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let store = self.store.clone();
        let key_bytes = key.as_bytes().to_vec();

        let maybe_bytes: Option<Vec<u8>> =
            task::spawn_blocking(move || get_from_store(store, key_bytes)).await??;

        Ok(maybe_bytes)
    }

    async fn find_by_prefix(&self, prefix: &str) -> Result<Vec<Vec<u8>>> {
        let store = self.store.clone();
        let prefix_bytes = prefix.as_bytes().to_vec();
        let iter: Iter = task::spawn_blocking(move || store.prefix(prefix_bytes)).await?;
        let result = iter
            .filter_map(|pair| pair.value().ok())
            .map(|v| v.to_vec())
            .collect::<Vec<Vec<u8>>>();
        Ok(result)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let key = key.as_bytes().to_vec();
        let store = self.store.clone();
        let _ = task::spawn_blocking(move || store.remove(key)).await?;
        Ok(())
    }
}

#[async_trait]
impl<T: DbProvider> DbProvider for std::sync::Arc<T> {
    async fn save(&self, key: &str, value: Vec<u8>) -> Result<()> {
        (**self).save(key, value).await
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        (**self).get(key).await
    }

    async fn find_by_prefix(&self, prefix: &str) -> Result<Vec<Vec<u8>>> {
        (**self).find_by_prefix(prefix).await
    }

    async fn delete(&self, key: &str) -> Result<()> {
        (**self).delete(key).await
    }
}
