//! Plan-time preference scoring (PLAN.md Phase 4).
//!
//! Holds an immutable snapshot of the learned model (per kind) plus every
//! activity's normalized feature vector, so the planner's activity sources can
//! turn `(kind, activity_id)` into a preference score synchronously inside their
//! hot loops. The snapshot is swapped atomically by [`PreferenceScorer::reload`]
//! after each vote/rating and after the batch embedding job — readers always see
//! a consistent version.
//!
//! Raw scores are log-odds (`base_pref + w·features`, roughly −5..+5). The
//! sources fold preference into a *multiplicative* weather model, so they use
//! [`ScorerSnapshot::quality`] — the logistic squash of the raw score into
//! `(0, 1)` — which keeps the product positive and monotonic in preference. The
//! raw score is exposed for the comparison UI's ranking (`raw_score`).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use anyhow::Result;

use crate::domain::{
    activities::ActivityKind,
    ports::{EmbeddingRepository, PreferenceRepository},
    preferences::KindModel,
};

/// Immutable per-request view of the learned model + feature vectors.
#[derive(Default)]
pub struct ScorerSnapshot {
    models: HashMap<ActivityKind, KindModel>,
    /// `activity_id → normalized feature vector` (aligned to that kind's model).
    features: HashMap<String, Vec<f64>>,
}

impl ScorerSnapshot {
    #[cfg(test)]
    pub fn new(models: Vec<KindModel>, features: HashMap<String, Vec<f64>>) -> Self {
        Self {
            models: models.into_iter().map(|m| (m.kind, m)).collect(),
            features,
        }
    }

    /// Raw log-odds preference score. An un-embedded activity falls back to its
    /// kind's `base_pref`; an unmodeled kind scores `0` (neutral).
    pub fn raw_score(&self, kind: ActivityKind, activity_id: &str) -> f64 {
        match self.models.get(&kind) {
            Some(model) => match self.features.get(activity_id) {
                Some(features) => model.score(features),
                None => model.base_pref,
            },
            None => 0.0,
        }
    }

    /// Positive `(0, 1)` preference multiplier for the planner's weather model:
    /// the logistic squash of [`ScorerSnapshot::raw_score`]. A neutral/unknown
    /// activity scores `σ(0) = 0.5`.
    pub fn quality(&self, kind: ActivityKind, activity_id: &str) -> f32 {
        (1.0 / (1.0 + (-self.raw_score(kind, activity_id)).exp())) as f32
    }
}

/// Long-lived, shared by the activity sources (read) and the preference service
/// (write). Constructed empty; call [`PreferenceScorer::reload`] to populate.
pub struct PreferenceScorer {
    snapshot: RwLock<Arc<ScorerSnapshot>>,
}

impl Default for PreferenceScorer {
    fn default() -> Self {
        Self::new()
    }
}

impl PreferenceScorer {
    pub fn new() -> Self {
        Self {
            snapshot: RwLock::new(Arc::new(ScorerSnapshot::default())),
        }
    }

    /// Clone the current snapshot Arc — cheap, lock held only for the clone.
    pub fn snapshot(&self) -> Arc<ScorerSnapshot> {
        self.snapshot.read().expect("scorer snapshot lock poisoned").clone()
    }

    /// Rebuild the snapshot from the model + feature stores and swap it in.
    pub async fn reload(
        &self,
        prefs: &dyn PreferenceRepository,
        embeddings: &dyn EmbeddingRepository,
    ) -> Result<()> {
        let models: HashMap<ActivityKind, KindModel> = prefs
            .load_model()
            .await?
            .into_iter()
            .map(|m| (m.kind, m))
            .collect();
        let features: HashMap<String, Vec<f64>> = embeddings
            .feature_vectors()
            .await?
            .into_iter()
            .map(|(id, _kind, vec)| (id, vec))
            .collect();
        tracing::info!(
            kinds = models.len(),
            activities = features.len(),
            "preference_scorer: reloaded"
        );
        *self.snapshot.write().expect("scorer snapshot lock poisoned") =
            Arc::new(ScorerSnapshot { models, features });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::preferences::FeatureWeight;

    fn model(kind: ActivityKind, base: f64, weight: f64) -> KindModel {
        KindModel {
            kind,
            base_pref: base,
            features: vec![FeatureWeight {
                name: "f".into(),
                weight,
                norm_mean: 0.0,
                norm_std: 1.0,
            }],
        }
    }

    #[test]
    fn raw_score_combines_base_and_weighted_feature() {
        let mut features = HashMap::new();
        features.insert("a".to_string(), vec![2.0]);
        let snap = ScorerSnapshot::new(vec![model(ActivityKind::Hiking, 0.5, 1.5)], features);
        // 0.5 + 1.5*2.0 = 3.5
        assert!((snap.raw_score(ActivityKind::Hiking, "a") - 3.5).abs() < 1e-9);
    }

    #[test]
    fn falls_back_to_base_pref_without_features() {
        let snap = ScorerSnapshot::new(vec![model(ActivityKind::Event, 1.0, 2.0)], HashMap::new());
        assert!((snap.raw_score(ActivityKind::Event, "missing") - 1.0).abs() < 1e-9);
    }

    #[test]
    fn unknown_kind_is_neutral() {
        let snap = ScorerSnapshot::default();
        assert_eq!(snap.raw_score(ActivityKind::Paragliding, "x"), 0.0);
        assert!((snap.quality(ActivityKind::Paragliding, "x") - 0.5).abs() < 1e-6);
    }

    #[test]
    fn quality_is_monotonic_in_raw_score() {
        let snap = ScorerSnapshot::new(
            vec![model(ActivityKind::Hiking, 0.0, 1.0)],
            HashMap::from([("hi".into(), vec![3.0]), ("lo".into(), vec![-3.0])]),
        );
        assert!(snap.quality(ActivityKind::Hiking, "hi") > snap.quality(ActivityKind::Hiking, "lo"));
        // Bounded to (0, 1).
        assert!(snap.quality(ActivityKind::Hiking, "hi") < 1.0);
        assert!(snap.quality(ActivityKind::Hiking, "lo") > 0.0);
    }
}
