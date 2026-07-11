//! Pure-Rust sentence-embedding adapter using [Candle]. The ONLY module that
//! runs the embedding model. Implements the [`Embedder`] port so the rest of
//! the app is unaware of the engine.
//!
//! We use Candle rather than ONNX Runtime because the nixpkgs `onnxruntime`
//! build deadlocks during session init (its split abseil/protobuf shared libs
//! self-deadlock when loaded via `ort`'s `load-dynamic`). Candle is pure Rust —
//! no native runtime, no ONNX — and `intfloat/multilingual-e5-small` ships as a
//! plain `BertModel` (multilingual vocab), which Candle's `bert` model runs
//! directly from local `safetensors`.
//!
//! The three model files are downloaded once (via `reqwest`, which does
//! Happy-Eyeballs IPv4 fallback and honours timeouts — unlike hf-hub's `ureq`)
//! into the model directory, then loaded locally on every subsequent start.
//!
//! [Candle]: https://github.com/huggingface/candle

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use candle_core::{Device, DType, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use tokenizers::{PaddingParams, Tokenizer, TruncationParams};
use tokio::io::AsyncWriteExt;

use crate::domain::ports::Embedder;

/// e5 was trained with a max sequence length of 512.
const MAX_LENGTH: usize = 512;
/// HuggingFace resolve base for the model repo.
const HF_BASE: &str = "https://huggingface.co/intfloat/multilingual-e5-small/resolve/main";
/// Files Candle needs to run the model, relative to both the repo and the dir.
const MODEL_FILES: [&str; 3] = ["config.json", "tokenizer.json", "model.safetensors"];
const MAX_DOWNLOAD_ATTEMPTS: usize = 3;

struct Model {
    bert: BertModel,
    tokenizer: Tokenizer,
    device: Device,
}

pub struct CandleEmbedder {
    // `BertModel::forward` takes `&self`, so no interior mutability is needed;
    // `Arc` just lets the blocking inference run on a `spawn_blocking` thread.
    model: Arc<Model>,
    batch_size: usize,
}

impl CandleEmbedder {
    /// Load `multilingual-e5-small`, downloading its files into `model_dir` on
    /// first use. Fallible (network on first run, missing/corrupt files) — so
    /// construct lazily, only when the batch job is about to run.
    pub async fn new(model_dir: &str, batch_size: usize) -> Result<Self> {
        let dir = PathBuf::from(model_dir);
        ensure_model_files(&dir).await?;

        // mmap + graph build is blocking CPU/IO work — keep it off the async workers.
        tracing::info!(model_dir, "loading embedding model (multilingual-e5-small) via candle");
        let load_dir = dir.clone();
        let model = tokio::task::spawn_blocking(move || load_model(&load_dir)).await??;
        tracing::info!("embedding model loaded and ready");

        Ok(Self {
            model: Arc::new(model),
            batch_size,
        })
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

/// Blocking model load from the local files in `dir`.
fn load_model(dir: &Path) -> Result<Model> {
    let device = Device::Cpu;

    let config_bytes = std::fs::read(dir.join("config.json")).context("reading config.json")?;
    let config: Config = serde_json::from_slice(&config_bytes).context("parsing config.json")?;

    // The weights ship as F32; load them as F32 (BERT inference in F16 on CPU
    // overflows to NaN in softmax / layernorm).
    let weights = dir.join("model.safetensors");
    let vb = unsafe {
        VarBuilder::from_mmaped_safetensors(&[weights], DType::F32, &device)
            .context("mmapping model.safetensors")?
    };
    let bert = BertModel::load(vb, &config).context("building BERT model")?;

    let mut tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
        .map_err(|e| anyhow::anyhow!("loading tokenizer.json: {e}"))?;
    tokenizer
        .with_padding(Some(PaddingParams::default()))
        .with_truncation(Some(TruncationParams {
            max_length: MAX_LENGTH,
            ..Default::default()
        }))
        .map_err(|e| anyhow::anyhow!("configuring tokenizer: {e}"))?;

    Ok(Model { bert, tokenizer, device })
}

impl Model {
    /// Embed one already-prefixed chunk: tokenize → BERT → masked mean pool.
    fn embed_chunk(&self, texts: &[String]) -> Result<Vec<Vec<f64>>> {
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| anyhow::anyhow!("tokenizing batch: {e}"))?;

        let ids: Vec<Tensor> = encodings
            .iter()
            .map(|e| Tensor::new(e.get_ids(), &self.device))
            .collect::<candle_core::Result<_>>()?;
        let masks: Vec<Tensor> = encodings
            .iter()
            .map(|e| Tensor::new(e.get_attention_mask(), &self.device))
            .collect::<candle_core::Result<_>>()?;
        let input_ids = Tensor::stack(&ids, 0)?;
        let attention_mask = Tensor::stack(&masks, 0)?;
        let token_type_ids = input_ids.zeros_like()?;

        // [batch, seq, hidden]
        let hidden = self
            .bert
            .forward(&input_ids, &token_type_ids, Some(&attention_mask))?;

        // Masked mean pooling over tokens: sum(hidden * mask) / sum(mask).
        let mask = attention_mask.to_dtype(DType::F32)?.unsqueeze(2)?; // [batch, seq, 1]
        let summed = hidden.broadcast_mul(&mask)?.sum(1)?; // [batch, hidden]
        let counts = mask.sum(1)?; // [batch, 1]
        let mean = summed.broadcast_div(&counts)?; // [batch, hidden]

        let rows: Vec<Vec<f32>> = mean.to_vec2()?;
        Ok(rows
            .into_iter()
            .map(|r| r.into_iter().map(|v| v as f64).collect())
            .collect())
    }
}

#[async_trait]
impl Embedder for CandleEmbedder {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f64>>> {
        // e5 expects a uniform prefix for the passages it embeds; applying it
        // consistently keeps every kind in the same embedding space.
        let prefixed: Vec<String> = texts.iter().map(|t| format!("passage: {t}")).collect();
        let model = self.model.clone();
        let batch_size = self.batch_size.max(1);
        let count = prefixed.len();
        tracing::info!(count, batch_size, "embed_batch: dispatching to blocking pool");

        // Candle inference is blocking CPU work — keep it off the async workers.
        let embeddings = tokio::task::spawn_blocking(move || -> Result<Vec<Vec<f64>>> {
            let mut out = Vec::with_capacity(count);
            for chunk in prefixed.chunks(batch_size) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn model_dir() -> String {
        std::env::var("E5_MODEL_DIR")
            .unwrap_or_else(|_| "/home/private/.cache/travelai/models/multilingual-e5-small".into())
    }

    #[tokio::test]
    async fn embeds_a_german_sentence() {
        let embedder = CandleEmbedder::new(&model_dir(), 8).await.unwrap();
        let out = embedder
            .embed_batch(&["Ein schöner Wandertag in den Alpen".to_string()])
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), 384); // multilingual-e5-small
        assert!(out[0].iter().all(|v| v.is_finite()));
    }

    #[tokio::test]
    async fn related_sentences_are_closer_than_unrelated() {
        let embedder = CandleEmbedder::new(&model_dir(), 8).await.unwrap();
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
