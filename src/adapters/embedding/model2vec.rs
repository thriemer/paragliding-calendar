//! Static-embedding adapter using a [model2vec] distilled model
//! (`minishlab/potion-multilingual-128M` by default). Implements the
//! [`Embedder`] port, so the rest of the app is unaware of the engine.
//!
//! Unlike the Candle/Burn BERT backends, there is **no transformer forward
//! pass**: the model is a `[vocab, dim]` matrix of pre-computed per-token
//! vectors. Embedding a text is just tokenize → gather the token rows → mean →
//! L2-normalize. That is why model2vec is orders of magnitude faster on a CPU
//! and the natural fit for a Raspberry Pi 4.
//!
//! The weight matrix is ~512 MB (`500353 × 256` f32). We `mmap` it and read
//! individual rows on demand rather than copying it into a heap allocation, so
//! only the token rows we actually touch fault into RAM (and stay reclaimable,
//! being file-backed) — important on a memory-constrained board.
//!
//! No `passage:` prefix is applied: that is an e5 convention. potion is
//! distilled from `bge-m3`, which does not use instruction prefixes.
//!
//! [model2vec]: https://github.com/MinishLab/model2vec

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use memmap2::Mmap;
use safetensors::{Dtype, SafeTensors};
use tokenizers::Tokenizer;
use tokio::io::AsyncWriteExt;

use crate::domain::ports::Embedder;

/// HuggingFace resolve base for the default model repo.
const HF_BASE: &str = "https://huggingface.co/minishlab/potion-multilingual-128M/resolve/main";
/// Files needed to run the model, relative to both the repo and the local dir.
const MODEL_FILES: [&str; 3] = ["config.json", "tokenizer.json", "model.safetensors"];
/// Name of the single tensor in the safetensors file.
const EMBEDDINGS_TENSOR: &str = "embeddings";
/// Defensive cap on tokens per text. potion's `seq_length` is effectively
/// unlimited (static embeddings have no attention cost); we truncate only to
/// bound work on a pathological input. Activity descriptions are far shorter.
const MAX_TOKENS: usize = 1024;
const MAX_DOWNLOAD_ATTEMPTS: usize = 3;

/// model2vec `config.json` — we only read what inference needs.
#[derive(serde::Deserialize)]
struct Model2VecConfig {
    /// Whether to L2-normalize the pooled embedding. Defaults to true, which
    /// matches every published potion model.
    #[serde(default = "default_true")]
    normalize: bool,
}

fn default_true() -> bool {
    true
}

struct Model {
    /// The safetensors file, kept mapped for the lifetime of the model.
    mmap: Mmap,
    /// Byte offset of the embedding matrix within `mmap`.
    data_offset: usize,
    vocab: usize,
    dim: usize,
    normalize: bool,
    tokenizer: Tokenizer,
}

pub struct Model2VecEmbedder {
    model: Arc<Model>,
    batch_size: usize,
}

impl Model2VecEmbedder {
    /// Load the model, downloading its files into `model_dir` on first use.
    /// Fallible (network on first run, missing/corrupt files) — construct lazily,
    /// only when the batch job is about to run.
    pub async fn new(model_dir: &str, batch_size: usize) -> Result<Self> {
        let dir = PathBuf::from(model_dir);
        ensure_model_files(&dir).await?;

        tracing::info!(model_dir, "loading embedding model (potion-multilingual-128M) via model2vec");
        let load_dir = dir.clone();
        let model = tokio::task::spawn_blocking(move || load_model(&load_dir)).await??;
        tracing::info!(vocab = model.vocab, dim = model.dim, "embedding model loaded and ready");

        Ok(Self {
            model: Arc::new(model),
            batch_size: batch_size.max(1),
        })
    }
}

/// Blocking model load from the local files in `dir`.
fn load_model(dir: &Path) -> Result<Model> {
    let config_bytes = std::fs::read(dir.join("config.json")).context("reading config.json")?;
    let config: Model2VecConfig =
        serde_json::from_slice(&config_bytes).context("parsing config.json")?;

    let tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
        .map_err(|e| anyhow::anyhow!("loading tokenizer.json: {e}"))?;

    // mmap the weights and locate the embedding matrix inside the mapping.
    let file = std::fs::File::open(dir.join("model.safetensors")).context("opening weights")?;
    // SAFETY: the file is read-only and lives as long as the mapping; we only
    // ever read from it.
    let mmap = unsafe { Mmap::map(&file).context("mmapping model.safetensors")? };

    let (data_offset, vocab, dim) = {
        let st = SafeTensors::deserialize(&mmap).context("parsing safetensors header")?;
        let view = st
            .tensor(EMBEDDINGS_TENSOR)
            .with_context(|| format!("tensor `{EMBEDDINGS_TENSOR}` missing"))?;
        anyhow::ensure!(
            view.dtype() == Dtype::F32,
            "expected f32 embeddings, got {:?}",
            view.dtype()
        );
        let shape = view.shape();
        anyhow::ensure!(shape.len() == 2, "expected 2-D embedding matrix, got {shape:?}");
        // Offset of the tensor data relative to the start of the mapping — the
        // header borrows the same buffer, so pointer subtraction is valid.
        let offset = view.data().as_ptr() as usize - mmap.as_ptr() as usize;
        (offset, shape[0], shape[1])
    };

    Ok(Model {
        mmap,
        data_offset,
        vocab,
        dim,
        normalize: config.normalize,
        tokenizer,
    })
}

impl Model {
    /// Row `id` of the embedding matrix as a byte slice (`dim` little-endian f32).
    fn row(&self, id: usize) -> &[u8] {
        let stride = self.dim * 4;
        let base = self.data_offset + id * stride;
        &self.mmap[base..base + stride]
    }

    /// Mean-pool the token rows into one vector, then optionally L2-normalize.
    /// Accumulates in f64 directly (the port's boundary type); empty input
    /// yields a zero vector, matching model2vec.
    fn pool(&self, ids: &[u32]) -> Vec<f64> {
        let mut acc = vec![0f64; self.dim];
        let mut n = 0usize;
        for &id in ids {
            let id = id as usize;
            if id >= self.vocab {
                continue; // out-of-vocab guard; should not happen with the paired tokenizer
            }
            for (slot, chunk) in acc.iter_mut().zip(self.row(id).chunks_exact(4)) {
                *slot += f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64;
            }
            n += 1;
        }
        if n == 0 {
            return acc;
        }
        let inv = 1.0 / n as f64;
        for v in acc.iter_mut() {
            *v *= inv;
        }
        if self.normalize {
            let norm = acc.iter().map(|x| x * x).sum::<f64>().sqrt().max(1e-32);
            for v in acc.iter_mut() {
                *v /= norm;
            }
        }
        acc
    }

    /// Tokenize `texts` (no special tokens, per model2vec) and pool each.
    fn embed_chunk(&self, texts: &[String]) -> Result<Vec<Vec<f64>>> {
        // `false` = no special tokens: model2vec pools over content tokens only.
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), false)
            .map_err(|e| anyhow::anyhow!("tokenizing batch: {e}"))?;

        Ok(encodings
            .iter()
            .map(|e| {
                let ids = e.get_ids();
                let ids = &ids[..ids.len().min(MAX_TOKENS)];
                self.pool(ids)
            })
            .collect())
    }
}

#[async_trait]
impl Embedder for Model2VecEmbedder {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f64>>> {
        let model = self.model.clone();
        let batch_size = self.batch_size.max(1);
        let texts = texts.to_vec();
        let count = texts.len();
        tracing::info!(count, batch_size, "embed_batch: dispatching to blocking pool");

        // Table lookups + page faults are blocking work — keep off the async workers.
        let embeddings = tokio::task::spawn_blocking(move || -> Result<Vec<Vec<f64>>> {
            let mut out = Vec::with_capacity(count);
            for chunk in texts.chunks(batch_size) {
                out.extend(model.embed_chunk(chunk)?);
            }
            Ok(out)
        })
        .await??;

        tracing::info!(
            count = embeddings.len(),
            dim = embeddings.first().map(Vec::len).unwrap_or(0),
            "embed_batch: complete"
        );
        Ok(embeddings)
    }
}

/// Download any missing model files into `dir`. Cheap no-op once cached.
async fn ensure_model_files(dir: &Path) -> Result<()> {
    tokio::fs::create_dir_all(dir)
        .await
        .with_context(|| format!("creating model dir {}", dir.display()))?;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()?;

    for name in MODEL_FILES {
        let dest = dir.join(name);
        if tokio::fs::metadata(&dest).await.map(|m| m.len() > 0).unwrap_or(false) {
            continue;
        }
        let url = format!("{HF_BASE}/{name}");
        tracing::info!(file = name, "downloading embedding model file");
        download_with_retry(&client, &url, &dest)
            .await
            .with_context(|| format!("downloading {name}"))?;
    }
    Ok(())
}

async fn download_with_retry(client: &reqwest::Client, url: &str, dest: &Path) -> Result<()> {
    for attempt in 1..=MAX_DOWNLOAD_ATTEMPTS {
        match download_file(client, url, dest).await {
            Ok(()) => return Ok(()),
            Err(e) if attempt < MAX_DOWNLOAD_ATTEMPTS => {
                let delay = 3u64.pow(attempt as u32); // 3s, 9s
                tracing::warn!(attempt, delay_secs = delay, error = %e, "download failed, retrying");
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

/// Stream `url` to a temp file, then atomically rename into place so a partial
/// download is never mistaken for a complete one.
async fn download_file(client: &reqwest::Client, url: &str, dest: &Path) -> Result<()> {
    let part = dest.with_extension("part");
    let mut resp = client.get(url).send().await?.error_for_status()?;
    let expected = resp.content_length();

    let mut file = tokio::fs::File::create(&part).await?;
    let mut written: u64 = 0;
    while let Some(chunk) = resp.chunk().await? {
        file.write_all(&chunk).await?;
        written += chunk.len() as u64;
    }
    file.flush().await?;
    drop(file);

    if let Some(total) = expected {
        if written != total {
            anyhow::bail!("incomplete download: {written}/{total} bytes");
        }
    }
    tokio::fs::rename(&part, dest).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_dir() -> String {
        std::env::var("POTION_MODEL_DIR").unwrap_or_else(|_| {
            "/home/private/.cache/travelai/models/potion-multilingual-128M".into()
        })
    }

    #[tokio::test]
    async fn embeds_a_german_sentence() {
        let embedder = Model2VecEmbedder::new(&model_dir(), 8).await.unwrap();
        let out = embedder
            .embed_batch(&["Ein schöner Wandertag in den Alpen".to_string()])
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), 256); // potion-multilingual-128M
        assert!(out[0].iter().all(|v| v.is_finite()));
    }

    #[tokio::test]
    async fn related_sentences_are_closer_than_unrelated() {
        let embedder = Model2VecEmbedder::new(&model_dir(), 8).await.unwrap();
        let out = embedder
            .embed_batch(&[
                "Wandern in den Bergen".to_string(),
                "Eine Bergwanderung in den Alpen".to_string(),
                "Ich repariere mein Fahrrad in der Garage".to_string(),
            ])
            .await
            .unwrap();

        let cos = |a: &[f64], b: &[f64]| {
            let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
            let na: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
            let nb: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
            dot / (na * nb)
        };
        let related = cos(&out[0], &out[1]);
        let unrelated = cos(&out[0], &out[2]);
        assert!(
            related > unrelated,
            "expected related ({related}) > unrelated ({unrelated})"
        );
    }
}
