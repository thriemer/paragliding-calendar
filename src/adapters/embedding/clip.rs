//! Multilingual CLIP embedder (`clip-ViT-B-32-multilingual-v1`).
//!
//! Text and images are embedded into the **same** 512-dim CLIP ViT-B/32 space,
//! so the application-layer fusion can average them directly (no per-modality
//! split). Two towers back this single adapter:
//!
//! - **Text** — a multilingual DistilBERT (`clip-ViT-B-32-multilingual-v1`),
//!   mean-pooled over real tokens, then a bias-less Linear(768→512, identity)
//!   projection into CLIP space. This is the multilingual half: German
//!   descriptions embed correctly, unlike the original English-only CLIP text
//!   tower.
//! - **Images** — the CLIP ViT-B/32 vision tower the text tower was distilled
//!   against (`sentence-transformers/clip-ViT-B-32`, `0_CLIPModel/`), used via
//!   `get_image_features`. Its CLIP text tower is loaded but unused.
//!
//! Both towers are pure-Rust candle (no ONNX/LAPACK), downloaded once as
//! `safetensors` into `EMBEDDING_CACHE_DIR` and loaded locally thereafter.

use std::{
    io::Cursor,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use candle_core::{DType, Device, Tensor};
use candle_nn::{Linear, Module, VarBuilder};
use candle_transformers::models::clip::{
    self,
    text_model::{Activation, ClipTextConfig},
    vision_model::ClipVisionConfig,
    ClipConfig, ClipModel,
};
use candle_transformers::models::distilbert::{Config as DistilBertConfig, DistilBertModel};
use image::ImageReader;
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams};
use tokio::io::AsyncWriteExt;

use crate::domain::ports::Embedder;

/// Multilingual DistilBERT text tower (+ mean-pool + dense projection).
const TEXT_REPO: &str =
    "https://huggingface.co/sentence-transformers/clip-ViT-B-32-multilingual-v1/resolve/main";
/// CLIP ViT-B/32 the text tower was aligned to — the only safetensors source
/// for this vision tower (`openai/clip-vit-base-patch32` ships only a `.bin`).
const VISION_REPO: &str =
    "https://huggingface.co/sentence-transformers/clip-ViT-B-32/resolve/main";

/// Max text tokens (this model's `sentence_bert_config.json` uses 128).
const MAX_LENGTH: usize = 128;
const IMAGE_SIZE: u32 = 224;
const MAX_DOWNLOAD_ATTEMPTS: usize = 3;

/// Projected text-embedding width and the pooled DistilBERT hidden size.
const PROJ_DIM: usize = 512;
const HIDDEN_DIM: usize = 768;

struct ModelInner {
    text: DistilBertModel,
    /// Linear(768→512), no bias — the `2_Dense` module in CLIP space.
    text_proj: Linear,
    tokenizer: Tokenizer,
    /// Full CLIP ViT-B/32; only the vision half (`get_image_features`) is used.
    vision: ClipModel,
    device: Device,
}

impl ModelInner {
    /// Embed a batch of texts: DistilBERT → attention-masked mean pool → dense
    /// projection into the 512-dim CLIP space.
    fn embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f64>>> {
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| anyhow::anyhow!("tokenizing: {e}"))?;

        let batch_size = encodings.len();
        let seq_len = encodings[0].get_ids().len();
        let mut ids = Vec::with_capacity(batch_size * seq_len);
        // `keep`: 1.0 for real tokens, 0.0 for padding (the pooling weight).
        let mut keep = Vec::with_capacity(batch_size * seq_len);
        for enc in &encodings {
            ids.extend_from_slice(enc.get_ids());
            keep.extend(enc.get_attention_mask().iter().map(|&m| m as f32));
        }

        let input_ids = Tensor::from_slice(&ids, (batch_size, seq_len), &self.device)?;
        let keep = Tensor::from_slice(&keep, (batch_size, seq_len), &self.device)?;

        // DistilBERT wants an *ignore* mask (1 = mask out) broadcastable over
        // `(bs, heads, q, q)`; invert `keep` and give it a `(bs,1,1,seq)` shape.
        let ignore = keep
            .affine(-1.0, 1.0)?
            .reshape((batch_size, 1, 1, seq_len))?
            .to_dtype(DType::U8)?;
        let hidden = self.text.forward(&input_ids, &ignore)?; // (bs, seq, 768)

        // Mean over real tokens: sum(hidden * keep) / sum(keep).
        let keep3 = keep.reshape((batch_size, seq_len, 1))?;
        let summed = hidden.broadcast_mul(&keep3)?.sum(1)?; // (bs, 768)
        let counts = keep.sum(1)?.reshape((batch_size, 1))?; // (bs, 1), always ≥1
        let pooled = summed.broadcast_div(&counts)?;
        let projected = self.text_proj.forward(&pooled)?; // (bs, 512)
        to_f64_rows(&projected)
    }

    /// Embed a batch of images in a single vision-tower forward pass: each image
    /// is preprocessed to a `(1,3,224,224)` tensor, the batch is concatenated into
    /// `(N,3,224,224)`, and `get_image_features` runs once. Batching amortizes the
    /// per-call overhead and runs the ViT matmuls at full width — the expensive
    /// half of CLIP — so it is markedly faster than one call per image.
    fn embed_images(&self, images: &[Vec<u8>]) -> Result<Vec<Vec<f64>>> {
        if images.is_empty() {
            return Ok(Vec::new());
        }
        let tensors: Vec<Tensor> = images
            .iter()
            .map(|bytes| preprocess_image(bytes, &self.device))
            .collect::<Result<_>>()?;
        let refs: Vec<&Tensor> = tensors.iter().collect();
        let batch = Tensor::cat(&refs, 0)?;
        let features = self.vision.get_image_features(&batch)?;
        to_f64_rows(&features)
    }
}

/// `(batch, dim)` f32 tensor → `Vec<Vec<f64>>` (the port's `f64` contract).
fn to_f64_rows(t: &Tensor) -> Result<Vec<Vec<f64>>> {
    let raw: Vec<Vec<f32>> = t.to_vec2()?;
    Ok(raw
        .into_iter()
        .map(|r| r.into_iter().map(|v| v as f64).collect())
        .collect())
}

pub struct ClipEmbedder {
    inner: Arc<ModelInner>,
    batch_size: usize,
}

impl ClipEmbedder {
    pub async fn new(model_dir: &str, batch_size: usize) -> Result<Self> {
        let dir = PathBuf::from(model_dir);
        ensure_model_files(&dir).await?;

        let load_dir = dir.clone();
        tracing::info!(model_dir, "loading multilingual CLIP (DistilBERT + ViT-B/32) via candle");
        let model = tokio::task::spawn_blocking(move || load_model(&load_dir)).await??;
        tracing::info!("multilingual CLIP model loaded and ready");

        Ok(Self {
            inner: Arc::new(model),
            batch_size: batch_size.max(1),
        })
    }

    pub fn embed_text_batch(&self, texts: &[String]) -> Result<Vec<Vec<f64>>> {
        self.inner.embed_texts(texts)
    }

    /// Embed a single image — the reference path the batched trait method is
    /// checked against in tests. Fusion of image + text embeddings lives in the
    /// application layer, not here (the adapter is I/O only).
    pub fn embed_image(&self, bytes: &[u8]) -> Result<Vec<f64>> {
        let pixel_values = preprocess_image(bytes, &self.inner.device)?;
        // `get_image_features` keeps the batch dim: a single `(1,3,224,224)` input
        // yields `(1,512)`. Drop the batch axis before flattening to a vector.
        let features = self.inner.vision.get_image_features(&pixel_values)?.squeeze(0)?;
        let raw: Vec<f32> = features.to_vec1()?;
        Ok(raw.into_iter().map(|v| v as f64).collect())
    }
}

#[async_trait]
impl Embedder for ClipEmbedder {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f64>>> {
        let inner = self.inner.clone();
        let batch_size = self.batch_size;
        let texts = texts.to_vec();
        let count = texts.len();
        tracing::info!(count, batch_size, "clip_embed_batch: dispatching to blocking pool");
        let embeddings = tokio::task::spawn_blocking(move || -> Result<Vec<Vec<f64>>> {
            let mut out = Vec::with_capacity(count);
            for chunk in texts.chunks(batch_size) {
                out.extend(inner.embed_texts(chunk)?);
            }
            Ok(out)
        })
        .await??;
        tracing::info!(
            count = embeddings.len(),
            dim = embeddings.first().map(Vec::len).unwrap_or(0),
            "clip_embed_batch: complete"
        );
        Ok(embeddings)
    }

    async fn embed_image_batch(&self, images: &[Vec<u8>]) -> Result<Vec<Vec<f64>>> {
        if images.is_empty() {
            return Ok(Vec::new());
        }
        let inner = self.inner.clone();
        let batch_size = self.batch_size;
        let images = images.to_vec();
        let count = images.len();
        tracing::info!(count, batch_size, "clip_embed_image_batch: dispatching to blocking pool");
        let embeddings = tokio::task::spawn_blocking(move || -> Result<Vec<Vec<f64>>> {
            let mut out = Vec::with_capacity(count);
            for chunk in images.chunks(batch_size) {
                out.extend(inner.embed_images(chunk)?);
            }
            Ok(out)
        })
        .await??;
        tracing::info!(count = embeddings.len(), "clip_embed_image_batch: complete");
        Ok(embeddings)
    }
}

fn preprocess_image(bytes: &[u8], device: &Device) -> Result<Tensor> {
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
    let mean: [f32; 3] = [0.48145466, 0.4578275, 0.40821073];
    let std: [f32; 3] = [0.26862954, 0.26130258, 0.27577711];

    let n = (IMAGE_SIZE * IMAGE_SIZE) as usize;
    let mut chw = vec![0f32; 3 * n];
    for y in 0..IMAGE_SIZE {
        for x in 0..IMAGE_SIZE {
            let idx = (y * IMAGE_SIZE + x) as usize * 3;
            for c in 0..3 {
                let pixel = raw[idx + c] as f32 / 255.0;
                chw[c * n + (y * IMAGE_SIZE + x) as usize] = (pixel - mean[c]) / std[c];
            }
        }
    }

    let tensor = Tensor::from_slice(&chw, (1, 3, IMAGE_SIZE as usize, IMAGE_SIZE as usize), device)?;
    Ok(tensor)
}

#[derive(serde::Deserialize)]
struct HfClipConfig {
    #[serde(default)]
    text_config: HfClipTextConfig,
    #[serde(default)]
    vision_config: HfClipVisionConfig,
    #[serde(default)]
    projection_dim: usize,
}

#[derive(serde::Deserialize, Default)]
struct HfClipTextConfig {
    hidden_size: usize,
    intermediate_size: usize,
    num_hidden_layers: usize,
    num_attention_heads: usize,
    #[serde(default)]
    vocab_size: usize,
    #[serde(default)]
    max_position_embeddings: usize,
}

#[derive(serde::Deserialize, Default)]
struct HfClipVisionConfig {
    hidden_size: usize,
    intermediate_size: usize,
    num_hidden_layers: usize,
    num_attention_heads: usize,
    #[serde(default)]
    patch_size: usize,
    #[serde(default)]
    image_size: usize,
}

fn hf_to_candle_config(hf: &HfClipConfig) -> ClipConfig {
    ClipConfig {
        text_config: ClipTextConfig {
            embed_dim: hf.text_config.hidden_size,
            intermediate_size: hf.text_config.intermediate_size,
            num_hidden_layers: hf.text_config.num_hidden_layers,
            num_attention_heads: hf.text_config.num_attention_heads,
            vocab_size: hf.text_config.vocab_size,
            max_position_embeddings: hf.text_config.max_position_embeddings,
            projection_dim: hf.projection_dim,
            activation: Activation::QuickGelu,
            pad_with: None,
        },
        vision_config: ClipVisionConfig {
            embed_dim: hf.vision_config.hidden_size,
            intermediate_size: hf.vision_config.intermediate_size,
            num_hidden_layers: hf.vision_config.num_hidden_layers,
            num_attention_heads: hf.vision_config.num_attention_heads,
            projection_dim: hf.projection_dim,
            num_channels: 3,
            image_size: hf.vision_config.image_size,
            patch_size: hf.vision_config.patch_size,
            activation: Activation::QuickGelu,
        },
        logit_scale_init_value: 2.6592,
        image_size: hf.vision_config.image_size,
    }
}

fn load_model(dir: &Path) -> Result<ModelInner> {
    let device = Device::Cpu;

    // --- Text tower: multilingual DistilBERT -------------------------------
    let text_config_bytes =
        std::fs::read(dir.join("text/config.json")).context("reading text/config.json")?;
    let text_config: DistilBertConfig =
        serde_json::from_slice(&text_config_bytes).context("parsing text/config.json")?;
    let text_vb = unsafe {
        VarBuilder::from_mmaped_safetensors(&[dir.join("text/model.safetensors")], DType::F32, &device)
            .context("mmapping text/model.safetensors")?
    };
    let text = DistilBertModel::load(text_vb, &text_config).context("building DistilBERT")?;

    // Dense projection (768→512, no bias, identity activation).
    let dense_vb = unsafe {
        VarBuilder::from_mmaped_safetensors(
            &[dir.join("text/2_Dense/model.safetensors")],
            DType::F32,
            &device,
        )
        .context("mmapping text/2_Dense/model.safetensors")?
    };
    let proj_w = dense_vb
        .get((PROJ_DIM, HIDDEN_DIM), "linear.weight")
        .context("loading dense linear.weight")?;
    let text_proj = Linear::new(proj_w, None);

    // --- Tokenizer: multilingual WordPiece ---------------------------------
    let mut tokenizer = Tokenizer::from_file(dir.join("text/tokenizer.json"))
        .map_err(|e| anyhow::anyhow!("loading text/tokenizer.json: {e}"))?;
    tokenizer
        .with_truncation(Some(TruncationParams {
            max_length: MAX_LENGTH,
            ..Default::default()
        }))
        .map_err(|e| anyhow::anyhow!("configuring truncation: {e}"))?
        // Pad to the longest sequence in each batch (BERT [PAD] = id 0).
        .with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            pad_id: 0,
            ..Default::default()
        }));

    // --- Vision tower: CLIP ViT-B/32 ---------------------------------------
    let vision_config_bytes =
        std::fs::read(dir.join("vision/config.json")).context("reading vision/config.json")?;
    let hf_config: HfClipConfig =
        serde_json::from_slice(&vision_config_bytes).context("parsing vision/config.json")?;
    let clip_config = hf_to_candle_config(&hf_config);
    let vision_vb = unsafe {
        VarBuilder::from_mmaped_safetensors(
            &[dir.join("vision/model.safetensors")],
            DType::F32,
            &device,
        )
        .context("mmapping vision/model.safetensors")?
    };
    let vision = clip::ClipModel::new(vision_vb, &clip_config).context("building CLIP vision")?;

    Ok(ModelInner {
        text,
        text_proj,
        tokenizer,
        vision,
        device,
    })
}

async fn ensure_model_files(dir: &Path) -> Result<()> {
    tokio::fs::create_dir_all(dir)
        .await
        .with_context(|| format!("creating model dir {}", dir.display()))?;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()?;

    // `(remote_url, local_relative_path)`. Both repos carry a `config.json` and
    // a `model.safetensors`, so they are namespaced into `text/` and `vision/`.
    let files = [
        // Text tower: DistilBERT weights + config + WordPiece tokenizer.
        (format!("{TEXT_REPO}/config.json"), "text/config.json"),
        (format!("{TEXT_REPO}/model.safetensors"), "text/model.safetensors"),
        (format!("{TEXT_REPO}/tokenizer.json"), "text/tokenizer.json"),
        // Dense projection (768→512, bias-less, identity activation).
        (
            format!("{TEXT_REPO}/2_Dense/model.safetensors"),
            "text/2_Dense/model.safetensors",
        ),
        // Vision tower: full CLIP ViT-B/32 (only the image side is used).
        (
            format!("{VISION_REPO}/0_CLIPModel/config.json"),
            "vision/config.json",
        ),
        (
            format!("{VISION_REPO}/0_CLIPModel/model.safetensors"),
            "vision/model.safetensors",
        ),
    ];

    for (url, local) in &files {
        let dest = dir.join(local);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        if tokio::fs::metadata(&dest)
            .await
            .map(|m| m.len() > 0)
            .unwrap_or(false)
        {
            continue;
        }
        tracing::info!(file = local, "downloading multilingual CLIP model file");
        download_with_retry(&client, url, &dest)
            .await
            .with_context(|| format!("downloading {local}"))?;
    }
    Ok(())
}

async fn download_with_retry(client: &reqwest::Client, url: &str, dest: &Path) -> Result<()> {
    for attempt in 1..=MAX_DOWNLOAD_ATTEMPTS {
        match download_file(client, url, dest).await {
            Ok(()) => return Ok(()),
            Err(e) if attempt < MAX_DOWNLOAD_ATTEMPTS => {
                let delay = 3u64.pow(attempt as u32);
                tracing::warn!(attempt, delay_secs = delay, error = %e, "download failed, retrying");
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

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
        std::env::var("CLIP_MODEL_DIR").unwrap_or_else(|_| {
            let base = std::env::var("HOME")
                .or_else(|_| std::env::var("XDG_CACHE_HOME"))
                .unwrap_or_else(|_| "/tmp".to_string());
            format!("{base}/.cache/travelai/models/clip-multilingual")
        })
    }

    #[tokio::test]
    async fn embeds_text_into_512_dim() {
        let embedder = ClipEmbedder::new(&model_dir(), 8).await.unwrap();
        let out = embedder
            .embed_text_batch(&["Ein schöner Wandertag in den Alpen".to_string()])
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), 512);
        assert!(out[0].iter().all(|v| v.is_finite()));
    }

    #[tokio::test]
    async fn related_texts_are_closer_than_unrelated() {
        let embedder = ClipEmbedder::new(&model_dir(), 8).await.unwrap();
        let out = embedder
            .embed_text_batch(&[
                "Wandern in den Bergen".to_string(),
                "Eine Bergwanderung in den Alpen".to_string(),
                "Ich repariere mein Fahrrad in der Garage".to_string(),
            ])
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

    fn solid_png(r: u8, g: u8, b: u8) -> Vec<u8> {
        use image::{ImageBuffer, Rgb};
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_pixel(64, 64, Rgb([r, g, b]));
        let mut bytes: Vec<u8> = Vec::new();
        img.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[tokio::test]
    async fn image_batch_matches_single_image() {
        let embedder = ClipEmbedder::new(&model_dir(), 8).await.unwrap();
        let a = solid_png(220, 30, 30);
        let b = solid_png(30, 120, 220);

        let single_a = embedder.embed_image(&a).unwrap();
        let single_b = embedder.embed_image(&b).unwrap();

        let batch = embedder
            .embed_image_batch(&[a.clone(), b.clone()])
            .await
            .unwrap();

        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].len(), 512);
        assert!(batch.iter().all(|v| v.iter().all(|x| x.is_finite())));
        // Batched forward pass must reproduce the per-image results (order-preserving).
        for (x, y) in single_a.iter().zip(&batch[0]) {
            assert!((x - y).abs() < 1e-4, "batch[0] diverged: {x} vs {y}");
        }
        for (x, y) in single_b.iter().zip(&batch[1]) {
            assert!((x - y).abs() < 1e-4, "batch[1] diverged: {x} vs {y}");
        }
    }
}
