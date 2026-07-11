//! Preference-model re-fit use case (PLAN.md Phase 3): load the accumulated
//! feedback and feature vectors, run the pure Bradley-Terry solver
//! ([`crate::domain::preference_fit`]), and persist the learned model.
//!
//! Called after every vote/rating (so scores track the latest feedback) and at
//! the end of the batch embedding job (a re-embed changes the normalization, so
//! the weights must be re-derived). All I/O lives here; the math stays pure in
//! the domain.

use std::collections::HashMap;

use anyhow::Result;

use crate::domain::{
    preference_fit::{L2_LAMBDA, RATING_WEIGHT, TrainingData, fit},
    preferences::KindModel,
    ports::{EmbeddingRepository, PreferenceRepository},
};

/// Re-fit and persist the preference model. Returns the learned per-kind models
/// (empty if there is no scaffold yet — i.e. before the first embedding pass).
pub async fn refit(
    prefs: &dyn PreferenceRepository,
    embeddings: &dyn EmbeddingRepository,
) -> Result<Vec<KindModel>> {
    let scaffold = prefs.load_model().await?;
    if scaffold.is_empty() {
        // No feature scaffold installed yet → nothing to fit against.
        return Ok(Vec::new());
    }

    let features: HashMap<String, (crate::domain::activities::ActivityKind, Vec<f64>)> = embeddings
        .feature_vectors()
        .await?
        .into_iter()
        .map(|(id, kind, vec)| (id, (kind, vec)))
        .collect();
    let comparisons = prefs.list_comparisons().await?;
    let ratings = prefs.list_ratings().await?;

    let data = TrainingData {
        scaffold,
        features,
        comparisons,
        ratings,
    };
    let models = fit(&data, RATING_WEIGHT, L2_LAMBDA);
    prefs.save_model(&models).await?;
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::persistence::postgres::PostgresRepository;
    use crate::domain::activities::ActivityKind;
    use crate::domain::ports::ActivityEmbeddingRow;
    use crate::domain::preferences::{FeatureWeight, KindModel};
    use crate::test_support::test_pool;
    use std::sync::Arc;

    fn scaffold(kind: ActivityKind, feats: &[&str]) -> KindModel {
        KindModel {
            kind,
            base_pref: 0.0,
            features: feats
                .iter()
                .map(|n| FeatureWeight {
                    name: (*n).into(),
                    weight: 0.0,
                    norm_mean: 0.0,
                    norm_std: 1.0,
                })
                .collect(),
        }
    }

    fn embedding_row(id: &str, kind: ActivityKind, features: Vec<f64>) -> ActivityEmbeddingRow {
        ActivityEmbeddingRow {
            activity_id: id.into(),
            kind,
            embedding: vec![0.0; 4],
            pca_dims: vec![],
            features,
        }
    }

    #[tokio::test]
    async fn refit_learns_weight_from_comparisons_and_persists() {
        let repo = Arc::new(PostgresRepository::new(test_pool().await));

        // Scaffold: one hiking feature. Two activities differing on that feature.
        PreferenceRepository::save_model(repo.as_ref(), &[scaffold(ActivityKind::Hiking, &["landscape"])])
            .await
            .unwrap();
        EmbeddingRepository::upsert_batch(
            repo.as_ref(),
            vec![
                embedding_row("hi", ActivityKind::Hiking, vec![2.0]),
                embedding_row("lo", ActivityKind::Hiking, vec![-2.0]),
            ],
        )
        .await
        .unwrap();
        for _ in 0..15 {
            PreferenceRepository::record_comparison(repo.as_ref(), "hi", "lo")
                .await
                .unwrap();
        }

        let models = refit(repo.as_ref(), repo.as_ref()).await.unwrap();
        let hiking = models.iter().find(|m| m.kind == ActivityKind::Hiking).unwrap();
        assert!(
            hiking.features[0].weight > 0.3,
            "expected positive learned weight, got {}",
            hiking.features[0].weight
        );

        // Persisted, so a reload sees the same weights.
        let reloaded = PreferenceRepository::load_model(repo.as_ref()).await.unwrap();
        let reloaded_hiking = reloaded.iter().find(|m| m.kind == ActivityKind::Hiking).unwrap();
        assert!((reloaded_hiking.features[0].weight - hiking.features[0].weight).abs() < 1e-9);
    }

    #[tokio::test]
    async fn refit_without_scaffold_is_a_noop() {
        let repo = Arc::new(PostgresRepository::new(test_pool().await));
        let models = refit(repo.as_ref(), repo.as_ref()).await.unwrap();
        assert!(models.is_empty());
    }
}
