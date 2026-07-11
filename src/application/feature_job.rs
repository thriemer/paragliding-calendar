//! Batch feature-extraction pipeline ("FeatureRegistry" in the plan).
//!
//! Collects every activity, embeds its description, reduces the per-kind
//! embeddings with PCA, z-score-normalizes the concatenated feature vectors
//! across the whole corpus, and persists the result to `activity_embeddings`.
//!
//! Whole-corpus batch only: PCA bases and normalizers are fit over the entire
//! per-kind corpus, so there is no incremental mode. The registry state
//! (bases/normalizers) lives only for the duration of a run — planning reads the
//! stored feature vectors directly.

use anyhow::Result;
use ndarray::Array2;
use std::collections::HashMap;

use crate::domain::{
    activities::{ActivityKind, kind_from_category},
    embedding::ActivityEmbedding,
    features::{Normalizer, fit_project_pca},
    image::ImageEmbedding,
    ports::{
        ActivityEmbeddingRow, Embedder, EmbeddingRepository, HappeningRepository, ImageRepository,
        ImageStore, PreferenceRepository, SiteRepository, TourRepository,
    },
    preferences::{FeatureWeight, KindModel},
};

const PCA_TARGET_VARIANCE: f64 = 0.80;
const PCA_MAX_K: usize = 7;

/// A single activity gathered before embedding: identity + kind + the text and
/// raw features from its [`ActivityEmbedding`] impl.
struct RawActivity {
    id: String,
    kind: ActivityKind,
    description: String,
    features: Vec<(String, f64)>,
}

/// Run the whole pipeline against the given ports. The composition root
/// constructs the (lazy, failure-isolated) embedder and passes it in.
///
/// After storing the feature vectors it installs the per-kind preference-model
/// scaffold (feature names + normalizers, weights reset to 0) and re-fits the
/// Bradley-Terry model: re-embedding changes the PCA bases and normalizers, so
/// any previously learned weights are stale and must be re-derived from the
/// stored comparisons/ratings (PLAN.md Phase 3).
pub async fn run(
    tours: &dyn TourRepository,
    events: &dyn HappeningRepository,
    sites: &dyn SiteRepository,
    embedder: &dyn Embedder,
    store: &dyn EmbeddingRepository,
    prefs: &dyn PreferenceRepository,
    images: &dyn ImageRepository,
    image_store: &dyn ImageStore,
    batch_size: usize,
) -> Result<usize> {
    let raw = collect(tours, events, sites).await?;
    if raw.is_empty() {
        tracing::info!("activity_features: no activities to embed");
        return Ok(0);
    }

    let descriptions: Vec<String> = raw.iter().map(|r| r.description.clone()).collect();
    let text_embeddings = embed_in_batches(embedder, &descriptions, batch_size).await?;

    // Re-embed every stored image, cache the vectors back into `activity_images`,
    // and reduce each activity's gallery to one mean visual vector. Failures (no
    // images, or a text-only embedder) degrade to text-only fusion.
    let image_vecs = embed_images_by_activity(embedder, images, image_store).await;
    let embeddings = fuse_text_and_images(&raw, text_embeddings, &image_vecs);

    let (rows, scaffold) = build_rows(&raw, &embeddings, PCA_TARGET_VARIANCE, PCA_MAX_K);
    let saved = store.upsert_batch(rows).await?;
    tracing::info!(saved, "activity_features: stored feature vectors");

    // Install the scaffold (resets weights to 0 for the new normalization), then
    // re-derive the weights from the accumulated feedback.
    prefs.save_model(&scaffold).await?;
    match crate::application::preference_fit::refit(prefs, store).await {
        Ok(models) => tracing::info!(kinds = models.len(), "activity_features: refit preference model"),
        Err(e) => tracing::error!(error = ?e, "activity_features: preference refit failed"),
    }
    Ok(saved)
}

/// Gather every activity from the three sources. Tours whose category doesn't
/// map to a kind are skipped; activities with any non-finite raw feature are
/// dropped with a warning so one bad row can't poison corpus statistics.
async fn collect(
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
        tracing::warn!(id = %a.activity_id(), "activity_features: skipping non-finite features");
        return;
    }
    raw.push(RawActivity {
        id: a.activity_id(),
        kind,
        description: a.description(),
        features,
    });
}

async fn embed_in_batches(
    embedder: &dyn Embedder,
    descriptions: &[String],
    batch_size: usize,
) -> Result<Vec<Vec<f64>>> {
    let total = descriptions.len();
    let mut out = Vec::with_capacity(total);
    for chunk in descriptions.chunks(batch_size.max(1)) {
        out.extend(embedder.embed_batch(chunk).await?);
        tracing::info!(
            "activity_features: embedded {}/{} ({}%)",
            out.len(),
            total,
            out.len() * 100 / total.max(1)
        );
    }
    Ok(out)
}

/// Re-embed every downloaded image (batched — the vision tower runs at full
/// width), persist the per-image vectors back to `activity_images`, and collapse
/// each activity's gallery to the mean of its image vectors. Returns
/// `activity_id → mean image vector`; an empty map means text-only fusion (no
/// images downloaded, or the embedder can't do images). Never fails the pipeline.
async fn embed_images_by_activity(
    embedder: &dyn Embedder,
    images: &dyn ImageRepository,
    image_store: &dyn ImageStore,
) -> HashMap<String, Vec<f64>> {
    match embed_images_inner(embedder, images, image_store).await {
        Ok(map) => map,
        Err(e) => {
            tracing::warn!(error = ?e, "activity_features: image embedding skipped, using text-only");
            HashMap::new()
        }
    }
}

async fn embed_images_inner(
    embedder: &dyn Embedder,
    images: &dyn ImageRepository,
    image_store: &dyn ImageStore,
) -> Result<HashMap<String, Vec<f64>>> {
    let downloaded = images.all_downloaded().await?;
    if downloaded.is_empty() {
        return Ok(HashMap::new());
    }

    // Load bytes; skip any whose blob is missing (keeps the batch aligned).
    let mut bytes: Vec<Vec<u8>> = Vec::with_capacity(downloaded.len());
    let mut kept = Vec::with_capacity(downloaded.len());
    for img in downloaded {
        match image_store.get(&img.content_hash).await {
            Ok(b) => {
                bytes.push(b);
                kept.push(img);
            }
            Err(e) => tracing::warn!(hash = %img.content_hash, error = ?e, "activity_features: image bytes missing"),
        }
    }
    if bytes.is_empty() {
        return Ok(HashMap::new());
    }

    let vectors = embedder.embed_image_batch(&bytes).await?;
    tracing::info!(count = vectors.len(), "activity_features: embedded images");

    // Cache the vectors back into activity_images (refreshed every re-embed).
    let rows: Vec<ImageEmbedding> = kept
        .iter()
        .zip(&vectors)
        .map(|(img, v)| ImageEmbedding {
            activity_id: img.activity_id.clone(),
            kind: img.kind,
            position: img.position,
            embedding: v.clone(),
        })
        .collect();
    images.store_embeddings(&rows).await?;

    // Mean of each activity's image vectors. Per-image vectors are L2-normalized
    // first so one high-magnitude image can't dominate the gallery centroid.
    let mut grouped: HashMap<String, Vec<Vec<f64>>> = HashMap::new();
    for (img, v) in kept.into_iter().zip(vectors) {
        grouped
            .entry(img.activity_id)
            .or_default()
            .push(l2_normalize(&v));
    }
    Ok(grouped
        .into_iter()
        .filter_map(|(id, vs)| mean_vector(&vs).map(|m| (id, m)))
        .collect())
}

/// L2-normalize a vector to unit length. A zero-norm (or empty) vector is
/// returned unchanged — it carries no direction to normalize.
fn l2_normalize(v: &[f64]) -> Vec<f64> {
    let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm == 0.0 {
        return v.to_vec();
    }
    v.iter().map(|x| x / norm).collect()
}

/// Componentwise mean of equal-length vectors. `None` if empty or ragged.
fn mean_vector(vectors: &[Vec<f64>]) -> Option<Vec<f64>> {
    let dim = vectors.first()?.len();
    if dim == 0 || vectors.iter().any(|v| v.len() != dim) {
        return None;
    }
    let mut mean = vec![0.0; dim];
    for v in vectors {
        for (m, x) in mean.iter_mut().zip(v) {
            *m += x;
        }
    }
    let n = vectors.len() as f64;
    for m in &mut mean {
        *m /= n;
    }
    Some(mean)
}

/// Fuse each activity's text embedding with its mean image vector as
/// `(text + image) / 2`. CLIP text and image projections are un-normalized and
/// live at different magnitudes, so each modality is L2-normalized before the
/// average — otherwise the larger-norm modality dominates, and image-bearing
/// activities would end up at a different scale than text-only ones within the
/// same kind. Text is normalized in *both* branches to keep that scale uniform.
/// Activities without images (paragliding, or no gallery) keep their (normalized)
/// text embedding. A dimension mismatch falls back to text-only for that activity.
fn fuse_text_and_images(
    raw: &[RawActivity],
    text_embeddings: Vec<Vec<f64>>,
    image_vecs: &HashMap<String, Vec<f64>>,
) -> Vec<Vec<f64>> {
    text_embeddings
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let text = l2_normalize(&text);
            match image_vecs.get(&raw[i].id) {
                Some(image) if image.len() == text.len() => {
                    let image = l2_normalize(image);
                    text.iter()
                        .zip(&image)
                        .map(|(t, im)| (t + im) / 2.0)
                        .collect()
                }
                _ => text,
            }
        })
        .collect()
}

/// Pure core: per kind, PCA-reduce the embeddings and z-score the concatenated
/// `[pca_1..pca_k, struct features…]` vector across that kind's corpus. The
/// stored `features` are normalized; `pca_dims` keeps the raw projection.
///
/// Also returns the per-kind preference-model scaffold: one [`KindModel`] per
/// kind carrying the feature names and normalizers (in the same order as the
/// stored vectors) with `base_pref` and all `weight`s at 0, ready for the
/// solver to fill in.
fn build_rows(
    raw: &[RawActivity],
    embeddings: &[Vec<f64>],
    target_variance: f64,
    max_k: usize,
) -> (Vec<ActivityEmbeddingRow>, Vec<KindModel>) {
    // Group activity indices by kind.
    let mut by_kind: HashMap<ActivityKind, Vec<usize>> = HashMap::new();
    for (i, r) in raw.iter().enumerate() {
        by_kind.entry(r.kind).or_default().push(i);
    }

    let mut rows = Vec::with_capacity(raw.len());
    let mut scaffold = Vec::with_capacity(by_kind.len());
    for (kind, idxs) in by_kind {
        let n = idxs.len();
        let dim = idxs.first().map_or(0, |&i| embeddings[i].len());

        // Stack this kind's embeddings into an n × dim matrix.
        let mut matrix = Array2::zeros((n, dim));
        for (row, &i) in idxs.iter().enumerate() {
            for (c, &v) in embeddings[i].iter().enumerate() {
                matrix[[row, c]] = v;
            }
        }
        let (k, variance_captured, projected) = fit_project_pca(&matrix, target_variance, max_k);

        // Concatenate named PCA dims with the struct features, per activity.
        let combined: Vec<Vec<(String, f64)>> = idxs
            .iter()
            .enumerate()
            .map(|(row, &i)| {
                let mut f: Vec<(String, f64)> = (0..k)
                    .map(|c| (format!("pca_{}", c + 1), projected[[row, c]]))
                    .collect();
                f.extend(raw[i].features.iter().cloned());
                f
            })
            .collect();

        // Corpus-global normalizer for this kind, then emit one row each.
        let normalizer = Normalizer::fit(&combined);
        tracing::info!(
            kind = kind.as_str(),
            activities = n,
            pca_dims = k,
            variance_captured = variance_captured,
            total_features = normalizer.feature_order().len(),
            "activity_features: fitted kind"
        );
        for (row, &i) in idxs.iter().enumerate() {
            rows.push(ActivityEmbeddingRow {
                activity_id: raw[i].id.clone(),
                kind,
                embedding: embeddings[i].clone(),
                pca_dims: (0..k).map(|c| projected[[row, c]]).collect(),
                features: normalizer.normalize_row(&combined[row]),
            });
        }

        // Scaffold row for this kind: feature names + normalizers in the stored
        // vector's order, weights left at 0 for the solver to fill.
        scaffold.push(KindModel {
            kind,
            base_pref: 0.0,
            features: normalizer
                .scaffold()
                .into_iter()
                .map(|(name, norm_mean, norm_std)| FeatureWeight {
                    name,
                    weight: 0.0,
                    norm_mean,
                    norm_std,
                })
                .collect(),
        });
    }
    (rows, scaffold)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(id: &str, kind: ActivityKind, features: Vec<(&str, f64)>) -> RawActivity {
        RawActivity {
            id: id.into(),
            kind,
            description: id.into(),
            features: features
                .into_iter()
                .map(|(n, v)| (n.to_string(), v))
                .collect(),
        }
    }

    // Deterministic pseudo-embedding with real cross-text variance.
    fn fake_embedding(seed: usize, dim: usize) -> Vec<f64> {
        (0..dim)
            .map(|i| (((seed.wrapping_mul(31).wrapping_add(i * 7)) % 97) as f64) / 97.0)
            .collect()
    }

    #[test]
    fn build_rows_produces_dense_consistent_vectors_per_kind() {
        // Two kinds; tours carry struct features, events carry none.
        let raw_acts = vec![
            raw(
                "t1",
                ActivityKind::Hiking,
                vec![("landscape", 3.0), ("ascent_log", 6.0)],
            ),
            raw(
                "t2",
                ActivityKind::Hiking,
                vec![("landscape", 5.0), ("ascent_log", 7.0)],
            ),
            raw(
                "t3",
                ActivityKind::Hiking,
                vec![("landscape", 1.0), ("ascent_log", 5.0)],
            ),
            raw("e1", ActivityKind::Event, vec![]),
            raw("e2", ActivityKind::Event, vec![]),
        ];
        let embeddings: Vec<Vec<f64>> = (0..raw_acts.len())
            .map(|i| fake_embedding(i + 1, 16))
            .collect();

        let (rows, scaffold) = build_rows(&raw_acts, &embeddings, 0.80, 7);
        assert_eq!(rows.len(), 5);

        // One scaffold per kind, feature names matching the stored vector width.
        assert_eq!(scaffold.len(), 2);
        for m in &scaffold {
            let row = rows.iter().find(|r| r.kind == m.kind).unwrap();
            assert_eq!(m.features.len(), row.features.len());
            assert!(m.features.iter().all(|f| f.weight == 0.0));
        }

        let hiking: Vec<_> = rows
            .iter()
            .filter(|r| r.kind == ActivityKind::Hiking)
            .collect();
        // Every hiking row has the same feature vector length (dense + consistent).
        let len = hiking[0].features.len();
        assert!(hiking.iter().all(|r| r.features.len() == len));
        // features = k pca dims + 2 struct features.
        assert_eq!(len, hiking[0].pca_dims.len() + 2);
        // Raw embedding is preserved at full width.
        assert!(rows.iter().all(|r| r.embedding.len() == 16));
        assert!(
            rows.iter()
                .all(|r| r.features.iter().all(|v| v.is_finite()))
        );
    }

    // --- integration: full pipeline against Postgres with a fake embedder ---

    use crate::adapters::persistence::postgres::PostgresRepository;
    use crate::domain::{
        happening::Happening,
        location::Location,
        paragliding::{ParaglidingLaunch, ParaglidingSite, SiteType},
        tour::Tour,
    };
    use crate::test_support::test_pool;
    use async_trait::async_trait;

    struct FakeEmbedder;

    #[async_trait]
    impl Embedder for FakeEmbedder {
        async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f64>>> {
            Ok(texts
                .iter()
                .map(|t| {
                    let seed = t
                        .bytes()
                        .fold(1usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize));
                    fake_embedding(seed, 384)
                })
                .collect())
        }
    }

    fn tour(id: &str, category: &str) -> Tour {
        Tour {
            id: id.into(),
            title: id.into(),
            category: category.into(),
            location: Location::new(50.0, 13.0, "L".into(), "DE".into()),
            description: format!("Beschreibung {id}"),
            duration_minutes: 120,
            length_meters: 8000,
            ascent_meters: 400,
            descent_meters: 400,
            difficulty: 2,
            stamina: 3,
            landscape: 4,
            experience: 3,
            is_loop: true,
            season_bitmask: 0,
            source_url: String::new(),
            image_urls: vec![],
            raw_json: "{}".into(),
        }
    }

    fn happening(id: &str) -> Happening {
        Happening {
            id: id.into(),
            title: id.into(),
            location: Some(Location::new(50.0, 13.0, String::new(), String::new())),
            category_id: None,
            category_title: None,
            category_keys: vec![],
            description_short: None,
            description_long: Some(format!("Fest {id}")),
            homepage: None,
            address: None,
            organizer: None,
            schedule_rules: None,
            dates: vec![],
            source_url: String::new(),
            image_urls: vec![],
            data: serde_json::Value::Null,
        }
    }

    fn site(name: &str, elevation: f64) -> ParaglidingSite {
        ParaglidingSite {
            name: name.into(),
            launches: vec![ParaglidingLaunch {
                site_type: SiteType::Hang,
                location: Location::new(47.0, 11.0, name.into(), "DE".into()),
                direction_degrees_start: 135.0,
                direction_degrees_stop: 225.0,
                elevation,
            }],
            landings: vec![],
            country: Some("DE".into()),
            data_source: "test".into(),
            parking_location: None,
            mute_alerts: None,
            rating: Some(4),
            preferred_weather_model: None,
        }
    }

    #[tokio::test]
    async fn pipeline_embeds_every_kind_end_to_end() {
        let repo = PostgresRepository::new(test_pool().await);

        TourRepository::save_batch(
            &repo,
            vec![
                tour("t1", "Wanderung"),
                tour("t2", "Mountainbike"),
                tour("t3", "Skitour"),
            ],
        )
        .await
        .unwrap();
        HappeningRepository::save_batch(&repo, vec![happening("e1"), happening("e2")])
            .await
            .unwrap();
        for s in [site("s1", 1200.0), site("s2", 900.0)] {
            SiteRepository::save(&repo, s).await.unwrap();
        }

        let image_store = crate::adapters::blob::FsImageStore::new(
            std::env::temp_dir().join(format!("travelai_fj_imgs_{}", std::process::id())),
        );
        let saved = run(
            &repo,
            &repo,
            &repo,
            &FakeEmbedder,
            &repo,
            &repo,
            &repo,
            &image_store,
            8,
        )
        .await
        .unwrap();
        // t3 "Skitour" maps to no kind → skipped: 2 tours + 2 events + 2 sites.
        assert_eq!(saved, 6);

        let all = EmbeddingRepository::find_all(&repo).await.unwrap();
        assert_eq!(all.len(), 6);
        assert!(all.iter().all(|r| r.embedding.len() == 384));
        assert!(all.iter().all(|r| !r.features.is_empty()));
        // Within a kind, feature-vector length is consistent.
        let mut by_kind: std::collections::HashMap<ActivityKind, usize> = Default::default();
        for r in &all {
            let entry = by_kind.entry(r.kind).or_insert(r.features.len());
            assert_eq!(
                *entry,
                r.features.len(),
                "inconsistent len for {:?}",
                r.kind
            );
        }
        assert!(by_kind.contains_key(&ActivityKind::Paragliding));
        assert!(by_kind.contains_key(&ActivityKind::Event));
    }
}
