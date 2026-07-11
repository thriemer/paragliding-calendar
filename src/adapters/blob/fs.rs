//! Filesystem-backed, content-addressed [`ImageStore`].
//!
//! Bytes live at `<root>/ab/cd/<hash>`, sharded by the first two hex byte-pairs so
//! no single directory holds the whole corpus. Content addressing makes `put`
//! idempotent and dedupes identical images (a stock photo reused across events is
//! stored once). Writes are atomic (temp file + rename).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use async_trait::async_trait;
use sha2::{Digest, Sha256};

use crate::domain::ports::ImageStore;

pub struct FsImageStore {
    root: PathBuf,
    /// Disambiguates temp files so concurrent writers never collide.
    tmp_seq: AtomicU64,
}

impl FsImageStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            tmp_seq: AtomicU64::new(0),
        }
    }

    fn path_for(&self, hash: &str) -> PathBuf {
        self.root.join(&hash[0..2]).join(&hash[2..4]).join(hash)
    }
}

/// Hex sha256 of the bytes — the content address.
fn hash_bytes(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in digest {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// A 64-char lowercase hex string. Guards the serve route against path traversal
/// since the hash comes from the URL.
fn is_valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}

#[async_trait]
impl ImageStore for FsImageStore {
    async fn put(&self, bytes: &[u8]) -> Result<String> {
        let hash = hash_bytes(bytes);
        let dest = self.path_for(&hash);
        if tokio::fs::try_exists(&dest).await.unwrap_or(false) {
            return Ok(hash);
        }
        let parent = dest.parent().expect("sharded path always has a parent");
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("creating image dir {}", parent.display()))?;

        let seq = self.tmp_seq.fetch_add(1, Ordering::Relaxed);
        let tmp = parent.join(format!("{hash}.{seq}.part"));
        tokio::fs::write(&tmp, bytes)
            .await
            .with_context(|| format!("writing {}", tmp.display()))?;
        tokio::fs::rename(&tmp, &dest)
            .await
            .with_context(|| format!("committing image {hash}"))?;
        Ok(hash)
    }

    async fn get(&self, hash: &str) -> Result<Vec<u8>> {
        anyhow::ensure!(is_valid_hash(hash), "invalid image hash");
        tokio::fs::read(self.path_for(hash))
            .await
            .with_context(|| format!("reading image {hash}"))
    }

    async fn has(&self, hash: &str) -> Result<bool> {
        if !is_valid_hash(hash) {
            return Ok(false);
        }
        Ok(tokio::fs::try_exists(self.path_for(hash))
            .await
            .unwrap_or(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_is_idempotent_and_content_addressed() {
        let dir = std::env::temp_dir().join(format!("travelai_imgtest_{}", std::process::id()));
        let store = FsImageStore::new(&dir);

        let h1 = store.put(b"hello world").await.unwrap();
        let h2 = store.put(b"hello world").await.unwrap();
        assert_eq!(h1, h2, "same bytes → same hash");
        assert_eq!(h1.len(), 64);
        assert!(store.has(&h1).await.unwrap());
        assert_eq!(store.get(&h1).await.unwrap(), b"hello world");

        let h3 = store.put(b"different").await.unwrap();
        assert_ne!(h1, h3);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn rejects_traversal_hashes() {
        let store = FsImageStore::new(std::env::temp_dir());
        assert!(!store.has("../../etc/passwd").await.unwrap());
        assert!(store.get("../../etc/passwd").await.is_err());
    }
}
