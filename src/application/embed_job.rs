//! Resumable embed job: the expensive half of the feature pipeline.
//!
//! Runs text + image CLIP inference and persists each batch's vectors
//! immediately — `text_embedding` in `activity_embeddings`, `embedding` in
//! `activity_images` — so a crash resumes from whatever is already stored
//! instead of re-embedding the whole corpus. The cheap whole-corpus reduction
//! (per-kind PCA + z-score) is the separate [`crate::application::reduce_job`].

use anyhow::Result;
use std::collections::HashSet;

use crate::domain::{
    activities::{ActivityKind, kind_from_category},
    embedding::ActivityEmbedding,
    image::ImageEmbedding,
    ports::{
        Embedder, EmbeddingRepository, HappeningRepository, ImageRepository, ImageStore,
        SiteRepository, TourRepository,
    },
};

/// A single activity gathered before embedding: identity + kind + the text and
/// raw features from its [`ActivityEmbedding`] impl. Shared with the reduce job.
pub(crate) struct RawActivity {
    pub(crate) id: String,
    pub(crate) kind: ActivityKind,
    pub(crate) description: String,
    pub(crate) features: Vec<(String, f64)>,
}

/// Embed every not-yet-embedded activity description and image, persisting each
/// batch as it completes. Returns the number of activity text embeddings newly
/// written this run. Idempotent and resumable: already-stored text/image vectors
/// are skipped, so a re-run only does the remainder.
pub async fn run(
    tours: &dyn TourRepository,
    events: &dyn HappeningRepository,
    sites: &dyn SiteRepository,
    embedder: &dyn Embedder,
    store: &dyn EmbeddingRepository,
    images: &dyn ImageRepository,
    image_store: &dyn ImageStore,
    batch_size: usize,
) -> Result<usize> {
    let text_embedded = embed_text(tours, events, sites, embedder, store, batch_size).await?;
    embed_images(embedder, images, image_store, batch_size).await?;
    Ok(text_embedded)
}

/// Embed the descriptions of activities that have no stored text embedding yet,
/// checkpointing after every batch.
async fn embed_text(
    tours: &dyn TourRepository,
    events: &dyn HappeningRepository,
    sites: &dyn SiteRepository,
    embedder: &dyn Embedder,
    store: &dyn EmbeddingRepository,
    batch_size: usize,
) -> Result<usize> {
    let raw = collect(tours, events, sites).await?;
    let done: HashSet<(String, ActivityKind)> = store
        .raw_text_embeddings()
        .await?
        .into_iter()
        .map(|(id, kind, _)| (id, kind))
        .collect();
    let pending: Vec<&RawActivity> = raw
        .iter()
        .filter(|r| !done.contains(&(r.id.clone(), r.kind)))
        .collect();
    if pending.is_empty() {
        tracing::info!("embed_job: all activity text already embedded");
        return Ok(0);
    }

    let total = pending.len();
    let mut embedded = 0usize;
    for chunk in pending.chunks(batch_size.max(1)) {
        let descriptions: Vec<String> = chunk.iter().map(|r| r.description.clone()).collect();
        let vectors = embedder.embed_batch(&descriptions).await?;
        let rows: Vec<(String, ActivityKind, Vec<f64>)> = chunk
            .iter()
            .zip(vectors)
            .map(|(r, v)| (r.id.clone(), r.kind, v))
            .collect();
        store.upsert_text_embeddings(&rows).await?;
        embedded += rows.len();
        tracing::info!(
            "embed_job: text {}/{} ({}%)",
            embedded,
            total,
            embedded * 100 / total.max(1)
        );
    }
    Ok(embedded)
}

/// Embed downloaded images that have no stored vector yet, checkpointing after
/// every batch. A text-only embedder (or a vision failure) ends the image pass
/// cleanly — those images stay unembedded and the pipeline degrades to text-only.
async fn embed_images(
    embedder: &dyn Embedder,
    images: &dyn ImageRepository,
    image_store: &dyn ImageStore,
    batch_size: usize,
) -> Result<()> {
    let pending = images.pending_image_embeddings().await?;
    if pending.is_empty() {
        tracing::info!("embed_job: all downloaded images already embedded");
        return Ok(());
    }

    let total = pending.len();
    let mut embedded = 0usize;
    for chunk in pending.chunks(batch_size.max(1)) {
        // Load bytes; skip any whose blob is missing (keeps the batch aligned).
        let mut bytes: Vec<Vec<u8>> = Vec::with_capacity(chunk.len());
        let mut kept = Vec::with_capacity(chunk.len());
        for img in chunk {
            match image_store.get(&img.content_hash).await {
                Ok(b) => {
                    bytes.push(b);
                    kept.push(img);
                }
                Err(e) => {
                    tracing::warn!(hash = %img.content_hash, error = ?e, "embed_job: image bytes missing")
                }
            }
        }
        if bytes.is_empty() {
            continue;
        }
        let vectors = match embedder.embed_image_batch(&bytes).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = ?e, "embed_job: image embedding unsupported/failed, skipping images");
                return Ok(());
            }
        };
        let rows: Vec<ImageEmbedding> = kept
            .iter()
            .zip(vectors)
            .map(|(img, v)| ImageEmbedding {
                activity_id: img.activity_id.clone(),
                kind: img.kind,
                position: img.position,
                embedding: v,
            })
            .collect();
        images.store_embeddings(&rows).await?;
        embedded += rows.len();
        tracing::info!(
            "embed_job: images {}/{} ({}%)",
            embedded,
            total,
            embedded * 100 / total.max(1)
        );
    }
    Ok(())
}

/// Gather every activity from the three sources. Tours whose category doesn't
/// map to a kind are skipped; activities with any non-finite raw feature are
/// dropped with a warning so one bad row can't poison corpus statistics. Shared
/// by both the embed job (descriptions) and the reduce job (kind + struct
/// features).
pub(crate) async fn collect(
    tours: &dyn TourRepository,
    events: &dyn HappeningRepository,
    sites: &dyn SiteRepository,
) -> Result<Vec<RawActivity>> {
    let mut raw = Vec::new();

    for tour in tours.find_all().await? {
        if let Some(kind) = kind_from_category(&tour.category) {
            push_if_finite(&mut raw, kind, &tour);
        }
    }
    for happening in events.find_all().await? {
        push_if_finite(&mut raw, ActivityKind::Event, &happening);
    }
    for site in sites.find_all().await? {
        push_if_finite(&mut raw, ActivityKind::Paragliding, &site);
    }

    Ok(raw)
}

fn push_if_finite(raw: &mut Vec<RawActivity>, kind: ActivityKind, a: &impl ActivityEmbedding) {
    let features = a.features();
    if !features.iter().all(|(_, v)| v.is_finite()) {
        tracing::warn!(id = %a.activity_id(), "embed_job: skipping non-finite features");
        return;
    }
    raw.push(RawActivity {
        id: a.activity_id(),
        kind,
        description: a.description(),
        features,
    });
}
