use std::fmt::Debug;

use anyhow::Result;
use fjall::{Iter, Keyspace};
use serde::{Serialize, de::DeserializeOwned};
use tokio::task;

pub struct PersistentStore {
    store: Keyspace,
}

fn get_from_store(store: Keyspace, key: Vec<u8>) -> anyhow::Result<Option<Vec<u8>>> {
    Ok(store.get(key)?.map(|v| v.to_vec()))
}

impl PersistentStore {
    pub fn from_keyspace(keyspace: Keyspace) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::TempDir;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Sample {
        a: u32,
        b: String,
    }

    fn fresh_store() -> (TempDir, PersistentStore) {
        let dir = tempfile::tempdir().unwrap();
        let db = fjall::Database::builder(dir.path()).open().unwrap();
        let ks = db
            .keyspace("store", fjall::KeyspaceCreateOptions::default)
            .unwrap();
        (dir, PersistentStore::from_keyspace(ks))
    }

    #[tokio::test]
    async fn put_then_get_returns_the_value() {
        let (_dir, store) = fresh_store();
        let s = Sample {
            a: 42,
            b: "hi".into(),
        };
        store.put("k", s).await.unwrap();
        let got: Sample = store.get("k").await.unwrap().unwrap();
        assert_eq!(
            got,
            Sample {
                a: 42,
                b: "hi".into()
            }
        );
    }

    #[tokio::test]
    async fn get_returns_none_for_unknown_key() {
        let (_dir, store) = fresh_store();
        let got: Option<Sample> = store.get("missing").await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn put_overwrites_existing_key() {
        let (_dir, store) = fresh_store();
        store
            .put(
                "k",
                Sample {
                    a: 1,
                    b: "x".into(),
                },
            )
            .await
            .unwrap();
        store
            .put(
                "k",
                Sample {
                    a: 2,
                    b: "y".into(),
                },
            )
            .await
            .unwrap();
        let got: Sample = store.get("k").await.unwrap().unwrap();
        assert_eq!(got.a, 2);
        assert_eq!(got.b, "y");
    }

    #[tokio::test]
    async fn remove_deletes_existing_key() {
        let (_dir, store) = fresh_store();
        store
            .put(
                "k",
                Sample {
                    a: 1,
                    b: "x".into(),
                },
            )
            .await
            .unwrap();
        store.remove("k").await.unwrap();
        let got: Option<Sample> = store.get("k").await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn get_all_starting_with_returns_matching_entries() {
        let (_dir, store) = fresh_store();
        store
            .put(
                "site_a",
                Sample {
                    a: 1,
                    b: "a".into(),
                },
            )
            .await
            .unwrap();
        store
            .put(
                "site_b",
                Sample {
                    a: 2,
                    b: "b".into(),
                },
            )
            .await
            .unwrap();
        store
            .put(
                "other",
                Sample {
                    a: 99,
                    b: "z".into(),
                },
            )
            .await
            .unwrap();

        let sites: Vec<Sample> = store.get_all_starting_with("site_").await.unwrap();
        assert_eq!(sites.len(), 2);
        assert!(sites.iter().any(|s| s.a == 1));
        assert!(sites.iter().any(|s| s.a == 2));
        assert!(!sites.iter().any(|s| s.a == 99));
    }
}
