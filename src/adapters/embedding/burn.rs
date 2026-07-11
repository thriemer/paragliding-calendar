use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use burn::backend::Flex;
use burn::backend::flex::FlexDevice;
use burn::tensor::{DType, Shape, TensorData, Tensor};
use tokenizers::{PaddingParams, Tokenizer, TruncationParams};

use crate::domain::ports::Embedder;

include!(concat!(env!("OUT_DIR"), "/model/model.rs"));

const MAX_LENGTH: usize = 512;

struct Inner {
    model: Model<Flex>,
    tokenizer: Tokenizer,
    device: FlexDevice,
}

pub struct BurnEmbedder {
    inner: Arc<Inner>,
    batch_size: usize,
}

impl BurnEmbedder {
    pub async fn new(model_dir: &str, batch_size: usize) -> Result<Self> {
        let device = FlexDevice::default();

        // Model weights are baked into the binary at compile time by build.rs
        // (ONNX → Burnpack codegen). Rebuild with BURN_MODEL=... to switch
        // between fp32/fp16/int8 variants — no runtime model file needed.
        //
        // NB: `Model::new` only builds the module graph with *default* (zero)
        // weights — it does NOT load the trained parameters. We must feed it the
        // generated `.bpk` burnpack, otherwise every forward pass returns zeros.
        tracing::info!("loading embedded Burn ONNX model");
        let weights = include_bytes!(concat!(env!("OUT_DIR"), "/model/model.bpk"));
        let model = Model::<Flex>::from_bytes(Bytes::from_bytes_vec(weights.to_vec()), &device);

        let tokenizer_path = std::path::PathBuf::from(model_dir).join("tokenizer.json");
        let mut tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow::anyhow!("loading tokenizer.json: {e}"))?;
        tokenizer
            .with_padding(Some(PaddingParams::default()))
            .with_truncation(Some(TruncationParams {
                max_length: MAX_LENGTH,
                ..Default::default()
            }))
            .map_err(|e| anyhow::anyhow!("configuring tokenizer: {e}"))?;

        Ok(Self {
            inner: Arc::new(Inner {
                model,
                tokenizer,
                device,
            }),
            batch_size: batch_size.max(1),
        })
    }
}

impl Inner {
    fn embed_chunk(&self, texts: &[String]) -> Result<Vec<Vec<f64>>> {
        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| anyhow::anyhow!("tokenizing batch: {e}"))?;

        let max_len = encodings
            .iter()
            .map(|e| e.get_ids().len())
            .max()
            .unwrap_or(1);
        let batch = texts.len();

        let rows: Vec<Vec<i64>> = encodings
            .iter()
            .map(|e| {
                let ids: Vec<i64> = e.get_ids().iter().map(|&v| v as i64).collect();
                let pad_len = max_len - ids.len();
                [ids, vec![0i64; pad_len]].concat()
            })
            .collect();
        let flat_ids: Vec<i64> = rows.iter().flat_map(|r| r.iter().copied()).collect();

        let flat_masks: Vec<i64> = encodings
            .iter()
            .map(|e| {
                let m: Vec<i64> = e.get_attention_mask().iter().map(|&v| v as i64).collect();
                let pad_len = max_len - m.len();
                [m, vec![0i64; pad_len]].concat()
            })
            .flat_map(|v| v)
            .collect();

        let input_ids = Tensor::<Flex, 2, Int>::from_data(
            TensorData::new(flat_ids, Shape::from([batch, max_len])),
            &self.device,
        );
        let attention_mask = Tensor::<Flex, 2, Int>::from_data(
            TensorData::new(flat_masks, Shape::from([batch, max_len])),
            &self.device,
        );
        let token_type_ids = Tensor::<Flex, 2, Int>::zeros(
            [batch, max_len],
            (&self.device, DType::I64),
        );

        let hidden = self
            .model
            .forward(input_ids, attention_mask.clone(), token_type_ids);

        // Masked mean pooling over tokens, matching the Candle backend so both
        // engines land in the same embedding space. Note: unlike Candle's
        // rank-reducing `sum`, Burn's `sum_dim` *keeps* the reduced axis as
        // size 1, so every tensor here stays rank 3 ([batch, seq|1, hidden])
        // and we flatten the singleton token axis when reading the data out.
        let mask = attention_mask.float().unsqueeze_dims::<3>(&[2]); // [batch, seq, 1]
        let summed = hidden.mul(mask.clone()).sum_dim(1); // [batch, 1, hidden]
        let counts = mask.sum_dim(1).clamp(1.0_f32, f32::MAX); // [batch, 1, 1]
        let pooled = summed.div(counts); // [batch, 1, hidden]

        let rows_out: Vec<Vec<f32>> = pooled
            .into_data()
            .to_vec::<f32>()
            .map_err(|e| anyhow::anyhow!("reading output tensor: {e}"))?
            .chunks(384)
            .map(|c| c.to_vec())
            .collect();

        Ok(rows_out
            .into_iter()
            .map(|r| r.into_iter().map(|v| v as f64).collect())
            .collect())
    }
}

#[async_trait]
impl Embedder for BurnEmbedder {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f64>>> {
        let prefixed: Vec<String> = texts.iter().map(|t| format!("passage: {t}")).collect();
        let inner = self.inner.clone();
        let batch_size = self.batch_size.max(1);
        let count = prefixed.len();

        tracing::info!(count, batch_size, "embed_batch: dispatching");

        let embeddings = tokio::task::spawn_blocking(move || -> Result<Vec<Vec<f64>>> {
            let mut out = Vec::with_capacity(count);
            for chunk in prefixed.chunks(batch_size) {
                out.extend(inner.embed_chunk(chunk)?);
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
    #[ignore = "loads the model; needs a populated E5_MODEL_DIR with ONNX model"]
    async fn embeds_a_german_sentence() {
        let embedder = BurnEmbedder::new(&model_dir(), 8).await.unwrap();
        let out = embedder
            .embed_batch(&["Ein schöner Wandertag in den Alpen".to_string()])
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), 384);
        assert!(out[0].iter().all(|v| v.is_finite()));
    }

    #[tokio::test]
    #[ignore = "loads the model; needs a populated E5_MODEL_DIR with ONNX model"]
    async fn related_sentences_are_closer_than_unrelated() {
        let embedder = BurnEmbedder::new(&model_dir(), 8).await.unwrap();
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
