use std::fmt::Debug;

use anyhow::{Result, anyhow};
use fjall::{Iter, Keyspace};
use serde::{Serialize, de::DeserializeOwned};
use tokio::{sync::OnceCell, task};

static GLOBAL_STORE: OnceCell<PersistentStore> = OnceCell::const_new();

pub struct PersistentStore {
    store: Keyspace,
}

fn get_from_store(store: Keyspace, key: Vec<u8>) -> anyhow::Result<Option<Vec<u8>>> {
    Ok(store.get(key)?.map(|v| v.to_vec()))
}

impl PersistentStore {
    fn from_keyspace(keyspace: Keyspace) -> Self {
        PersistentStore { store: keyspace }
    }

    #[tracing::instrument(name = "put_store", level = "debug", skip(self))]
    pub async fn put<T: Serialize + Send + Debug + 'static>(
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

    #[tracing::instrument(name = "query_store", level = "debug", skip(self))]
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

    pub async fn get_all_starting_with<T: DeserializeOwned + Send + 'static>(
        &self,
        key: &str,
    ) -> Result<Vec<T>> {
        let store = self.store.clone();
        let key_bytes = key.as_bytes().to_vec();
        let maybe_bytes: Iter = task::spawn_blocking(move || store.prefix(key_bytes)).await?;
        let result = maybe_bytes
            .filter_map(|pair| pair.value().ok())
            .filter_map(|bytes| postcard::from_bytes::<T>(&bytes).ok())
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

pub fn init(keyspace: Keyspace) -> Result<()> {
    let store = PersistentStore::from_keyspace(keyspace);
    GLOBAL_STORE
        .set(store)
        .map_err(|_| anyhow!("Store already initialized"))?;
    Ok(())
}

fn get_store() -> &'static PersistentStore {
    GLOBAL_STORE
        .get()
        .expect("Store not initialized. Call store::init() first.")
}

pub async fn put<T: Serialize + Send + Debug + 'static>(key: &str, value: T) -> Result<()> {
    get_store().put(key, value).await
}

pub async fn get<T: DeserializeOwned + Send + 'static>(key: &str) -> Result<Option<T>> {
    get_store().get(key).await
}

pub async fn get_all_starting_with<T: DeserializeOwned + Send + 'static>(
    key: &str,
) -> Result<Vec<T>> {
    get_store().get_all_starting_with(key).await
}

pub async fn remove(key: &str) -> Result<()> {
    get_store().remove(key).await
}
