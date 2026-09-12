//! ONNX Runtime embedding backend, via the [`ort`] crate. Runs the **int8
//! quantized** UForm v3 multilingual two-tower model (`unum-cloud/
//! uform3-image-text-multilingual-base`) — the model Burn's ONNX importer can't
//! ingest, because its quantized ops (`DynamicQuantizeLinear`, `ConvInteger`)
//! are unsupported by `burn-onnx`. ort's C++ runtime runs them natively, which
//! is the whole point: int8 is the speed win on the target Raspberry Pi.
//!
//! Both towers bake pooling + projection into the graph, so unlike the Burn/
//! candle CLIP paths there is no separate dense head here — each tower emits the
//! final 256-dim `embeddings` output directly. We L2-normalize (the raw output
//! is not unit-length) so cosine over the stored vectors behaves.
//!
//! Contract (from the shipped ONNX, verified by introspection):
//!   - text:  inputs `input_ids`,`attention_mask` Int32 `[batch, 50]` (FIXED
//!            seq=50); output `embeddings` f32 `[batch, 256]`. Pad id = 1.
//!   - image: input `images` f32 `[batch, 3, 224, 224]`; output `embeddings`
//!            f32 `[batch, 256]`. CLIP-style normalization.
//!
//! ## NixOS / linking
//! `ort` is built with `load-dynamic`, so it dlopen()s libonnxruntime at runtime
//! from `ORT_DYLIB_PATH` (set in the flake devShell and the systemd unit) rather
//! than downloading an FHS-linked binary that can't run on NixOS. NB: an earlier
//! attempt hit a session-init deadlock with an older nixpkgs onnxruntime; that
//! is resolved on 1.23.2 (session build + inference both verified).
//!
//! [`ort`]: https://github.com/pykeio/ort

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use ort::{session::Session, value::Tensor};
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};
use tokio::io::AsyncWriteExt;

use crate::domain::ports::Embedder;

/// HuggingFace resolve base for the UForm v3 multilingual repo.
const HF_BASE: &str =
    "https://huggingface.co/unum-cloud/uform3-image-text-multilingual-base/resolve/main";
/// Files the backend needs, relative to both the repo and the model dir.
const MODEL_FILES: [&str; 3] = ["text_encoder.onnx", "image_encoder.onnx", "tokenizer.json"];
const MAX_DOWNLOAD_ATTEMPTS: usize = 3;

/// Fixed text sequence length baked into the text tower's ONNX input shape.
const SEQ_LEN: usize = 50;
/// UForm text tokenizer pad id (config.json `padding_idx`).
const PAD_ID: u32 = 1;
const IMAGE_SIZE: u32 = 224;
const EMBED_DIM: usize = 256;
/// CLIP normalization constants — identical to the Burn CLIP path.
const IMAGE_MEAN: [f32; 3] = [0.48145466, 0.4578275, 0.40821073];
const IMAGE_STD: [f32; 3] = [0.26862954, 0.26130258, 0.27577711];

/// `Session::run` needs `&mut self`, so each tower sits behind its own `Mutex`
/// (separate locks let text and image inference proceed independently).
struct Inner {
    text: Mutex<Session>,
    image: Mutex<Session>,
    tokenizer: Tokenizer,
}

pub struct OrtEmbedder {
    inner: Arc<Inner>,
    batch_size: usize,
}

impl OrtEmbedder {
    /// Load both ONNX towers + tokenizer, downloading them into `model_dir` on
    /// first use. Fallible (network on first run, missing `ORT_DYLIB_PATH`,
    /// corrupt files) — construct lazily, only when embedding is about to run.
    pub async fn new(model_dir: &str, batch_size: usize) -> Result<Self> {
        let dir = PathBuf::from(model_dir);
        ensure_model_files(&dir).await?;

        tracing::info!(model_dir, "loading UForm v3 (int8 ONNX) embedder via ort");
        let inner = tokio::task::spawn_blocking(move || load_inner(&dir)).await??;
        tracing::info!("ort embedding backend loaded and ready");

        Ok(Self {
            inner: Arc::new(inner),
            batch_size: batch_size.max(1),
        })
    }
}

/// Blocking session + tokenizer load from the local files in `dir`.
fn load_inner(dir: &Path) -> Result<Inner> {
    let text = Session::builder()
        .context("ort session builder (text)")?
        .commit_from_file(dir.join("text_encoder.onnx"))
        .context("loading text_encoder.onnx")?;
    let image = Session::builder()
        .context("ort session builder (image)")?
        .commit_from_file(dir.join("image_encoder.onnx"))
        .context("loading image_encoder.onnx")?;

    let mut tokenizer = Tokenizer::from_file(dir.join("tokenizer.json"))
        .map_err(|e| anyhow::anyhow!("loading tokenizer.json: {e}"))?;
    // The text graph's input dim is a HARD 50, so pad to exactly 50 (not
    // batch-longest) and truncate at 50.
    tokenizer
        .with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::Fixed(SEQ_LEN),
            pad_id: PAD_ID,
            ..Default::default()
        }))
        .with_truncation(Some(TruncationParams {
            max_length: SEQ_LEN,
            ..Default::default()
        }))
        .map_err(|e| anyhow::anyhow!("configuring tokenizer: {e}"))?;

    Ok(Inner {
        text: Mutex::new(text),
        image: Mutex::new(image),
        tokenizer,
    })
}

impl Inner {
    /// Tokenize → text tower → L2-normalized (batch, 256).
    fn embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f64>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| anyhow::anyhow!("tokenizing: {e}"))?;

        let batch = encodings.len();
        let mut ids = Vec::with_capacity(batch * SEQ_LEN);
        let mut mask = Vec::with_capacity(batch * SEQ_LEN);
        for enc in &encodings {
            // Fixed-padding guarantees exactly SEQ_LEN ids/mask per row.
            ids.extend(enc.get_ids().iter().map(|&v| v as i32));
            mask.extend(enc.get_attention_mask().iter().map(|&v| v as i32));
        }

        let ids_t = Tensor::from_array(([batch, SEQ_LEN], ids)).context("input_ids tensor")?;
        let mask_t = Tensor::from_array(([batch, SEQ_LEN], mask)).context("attention_mask tensor")?;

        let mut sess = self.text.lock().expect("text session mutex poisoned");
        let outputs = sess
            .run(ort::inputs!["input_ids" => ids_t, "attention_mask" => mask_t])
            .context("text inference")?;
        let (_, data) = outputs["embeddings"]
            .try_extract_tensor::<f32>()
            .context("extracting text embeddings")?;
        Ok(normalize_rows(data, batch))
    }

    /// Preprocess → image tower → L2-normalized (N, 256).
    fn embed_images(&self, images: &[Vec<u8>]) -> Result<Vec<Vec<f64>>> {
        if images.is_empty() {
            return Ok(Vec::new());
        }
        let batch = images.len();
        let n = (IMAGE_SIZE * IMAGE_SIZE) as usize;
        let mut pixels = Vec::with_capacity(batch * 3 * n);
        for bytes in images {
            pixels.extend(preprocess_image(bytes)?);
        }
        let img_t = Tensor::from_array((
            [batch, 3, IMAGE_SIZE as usize, IMAGE_SIZE as usize],
            pixels,
        ))
        .context("images tensor")?;

        let mut sess = self.image.lock().expect("image session mutex poisoned");
        let outputs = sess
            .run(ort::inputs!["images" => img_t])
            .context("image inference")?;
        let (_, data) = outputs["embeddings"]
            .try_extract_tensor::<f32>()
            .context("extracting image embeddings")?;
        Ok(normalize_rows(data, batch))
    }
}

/// `(batch*256)` f32 → `batch` L2-normalized `Vec<f64>` rows.
fn normalize_rows(flat: &[f32], batch: usize) -> Vec<Vec<f64>> {
    flat.chunks(EMBED_DIM)
        .take(batch)
        .map(|row| {
            let norm = row.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>().sqrt();
            let inv = if norm > 0.0 { 1.0 / norm } else { 0.0 };
            row.iter().map(|&v| v as f64 * inv).collect()
        })
        .collect()
}

/// Decode → center-crop → resize 224 → CLIP-normalize into a CHW `[3,224,224]`
/// f32 buffer. Same pipeline as the Burn CLIP backend.
fn preprocess_image(bytes: &[u8]) -> Result<Vec<f32>> {
    use image::ImageReader;
    use std::io::Cursor;

    let img = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .context("guessing image format")?
        .decode()
        .context("decoding image")?
        .to_rgb8();

    let (w, h) = img.dimensions();
    let min_dim = w.min(h);
    let x = (w - min_dim) / 2;
    let y = (h - min_dim) / 2;
    let cropped = image::imageops::crop_imm(&img, x, y, min_dim, min_dim).to_image();
    let resized = image::imageops::resize(
        &cropped,
        IMAGE_SIZE,
        IMAGE_SIZE,
        image::imageops::FilterType::Triangle,
    );

    let raw = resized.into_raw();
    let n = (IMAGE_SIZE * IMAGE_SIZE) as usize;
    let mut chw = vec![0f32; 3 * n];
    for i in 0..n {
        for c in 0..3 {
            let pixel = raw[i * 3 + c] as f32 / 255.0;
            chw[c * n + i] = (pixel - IMAGE_MEAN[c]) / IMAGE_STD[c];
        }
    }
    Ok(chw)
}

// --- model file provisioning (mirrors the candle backend) ---

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
        tracing::info!(file = name, "downloading UForm model file");
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

/// Stream `url` to a temp file, then atomically rename into place.
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

#[async_trait]
impl Embedder for OrtEmbedder {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f64>>> {
        let inner = self.inner.clone();
        let batch_size = self.batch_size;
        let texts = texts.to_vec();
        let count = texts.len();
        tracing::info!(count, batch_size, "ort embed_batch: dispatching to blocking pool");
        let out = tokio::task::spawn_blocking(move || -> Result<Vec<Vec<f64>>> {
            let mut out = Vec::with_capacity(count);
            for chunk in texts.chunks(batch_size) {
                out.extend(inner.embed_texts(chunk)?);
            }
            Ok(out)
        })
        .await??;
        tracing::info!(
            count = out.len(),
            dim = out.first().map(Vec::len).unwrap_or(0),
            "ort embed_batch: complete"
        );
        Ok(out)
    }

    async fn embed_image_batch(&self, images: &[Vec<u8>]) -> Result<Vec<Vec<f64>>> {
        if images.is_empty() {
            return Ok(Vec::new());
        }
        let inner = self.inner.clone();
        let batch_size = self.batch_size;
        let images = images.to_vec();
        let count = images.len();
        tracing::info!(count, batch_size, "ort embed_image_batch: dispatching to blocking pool");
        let out = tokio::task::spawn_blocking(move || -> Result<Vec<Vec<f64>>> {
            let mut out = Vec::with_capacity(count);
            for chunk in images.chunks(batch_size) {
                out.extend(inner.embed_images(chunk)?);
            }
            Ok(out)
        })
        .await??;
        tracing::info!(count = out.len(), "ort embed_image_batch: complete");
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_dir() -> String {
        std::env::var("UFORM_MODEL_DIR").unwrap_or_else(|_| {
            let base = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            format!("{base}/.cache/travelai/models/uform3-multilingual")
        })
    }

    #[tokio::test]
    #[ignore = "loads the UForm ONNX model; needs ORT_DYLIB_PATH + model files"]
    async fn embeds_text_into_256_dim() {
        let embedder = OrtEmbedder::new(&model_dir(), 8).await.unwrap();
        let out = embedder
            .embed_batch(&["Ein schöner Wandertag in den Alpen".to_string()])
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), EMBED_DIM);
        assert!(out[0].iter().all(|v| v.is_finite()));
        let norm: f64 = out[0].iter().map(|v| v * v).sum::<f64>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6, "not unit-normalized: {norm}");
    }

    #[tokio::test]
    #[ignore = "loads the UForm ONNX model; needs ORT_DYLIB_PATH + model files"]
    async fn related_texts_are_closer_than_unrelated() {
        let embedder = OrtEmbedder::new(&model_dir(), 8).await.unwrap();
        let out = embedder
            .embed_batch(&[
                "Wandern in den Bergen".to_string(),
                "Eine Bergwanderung in den Alpen".to_string(),
                "Ich repariere mein Fahrrad in der Garage".to_string(),
            ])
            .await
            .unwrap();
        let cos = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
        assert!(cos(&out[0], &out[1]) > cos(&out[0], &out[2]));
    }
}
