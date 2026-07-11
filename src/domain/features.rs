//! Corpus-global feature math for the preference pipeline: per-kind PCA of the
//! sentence embeddings and z-score normalization of the concatenated feature
//! vectors.
//!
//! All computation is `f64`: `linfa`'s PCA `Fit` is implemented only for `f64`,
//! and the extra precision is harmless for a batch job (the iterative LOBPCG
//! eigensolver and the variance accumulation both benefit). The pipeline
//! upcasts the model's `f32` embeddings on the way in and downcasts the results
//! to `f32` at the storage boundary.

use std::collections::{BTreeMap, HashMap};

use linfa::{
    Dataset,
    traits::{Fit, Transformer},
};
use linfa_reduction::Pca;
use ndarray::{Array2, s};

/// Standard deviations at or below this are treated as zero variance.
const EPS: f64 = 1e-9;

/// Per-feature z-score parameters fitted over the corpus.
#[derive(Debug, Clone, Copy)]
pub struct ZScoreParams {
    pub mean: f64,
    pub std: f64,
}

impl ZScoreParams {
    /// Fit population mean/std over the observed values.
    pub fn fit(values: &[f64]) -> Self {
        let n = values.len();
        if n == 0 {
            return Self {
                mean: 0.0,
                std: 0.0,
            };
        }
        let mean = values.iter().sum::<f64>() / n as f64;
        let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;
        Self {
            mean,
            std: var.sqrt(),
        }
    }

    /// Z-score. A zero-variance (constant) feature carries no signal, so it
    /// normalizes to 0 rather than dividing by ~0.
    pub fn normalize(&self, x: f64) -> f64 {
        if self.std <= EPS {
            0.0
        } else {
            (x - self.mean) / self.std
        }
    }
}

/// Per-kind normalizer: a fixed feature order plus z-score params per feature.
/// Fitting unions every feature name seen for the kind, so activities that omit
/// an optional feature still produce a dense, consistently-ordered vector — the
/// missing feature imputes to a normalized 0 (its corpus mean).
#[derive(Debug, Clone)]
pub struct Normalizer {
    order: Vec<String>,
    params: HashMap<String, ZScoreParams>,
}

impl Normalizer {
    pub fn fit(rows: &[Vec<(String, f64)>]) -> Self {
        // BTreeMap → deterministic (alphabetical) canonical feature order.
        let mut values: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for row in rows {
            for (name, v) in row {
                values.entry(name.clone()).or_default().push(*v);
            }
        }
        let order: Vec<String> = values.keys().cloned().collect();
        let params = values
            .into_iter()
            .map(|(name, vs)| (name, ZScoreParams::fit(&vs)))
            .collect();
        Self { order, params }
    }

    /// The canonical feature order the normalized vectors follow — the batch job
    /// logs its width per kind, and Phase 4 uses it to map indices back to names.
    pub fn feature_order(&self) -> &[String] {
        &self.order
    }

    /// Ordered `(name, mean, std)` per feature — the scaffold the preference
    /// model (`preference_model.features`) is built from. Aligned index-for-index
    /// with [`Normalizer::normalize_row`]'s output so a learned weight lines up
    /// with the feature value it multiplies.
    pub fn scaffold(&self) -> Vec<(String, f64, f64)> {
        self.order
            .iter()
            .map(|name| {
                let p = self.params[name];
                (name.clone(), p.mean, p.std)
            })
            .collect()
    }

    /// Dense normalized vector in canonical order; a feature absent from `row`
    /// imputes to 0.
    pub fn normalize_row(&self, row: &[(String, f64)]) -> Vec<f64> {
        let lookup: HashMap<&str, f64> = row.iter().map(|(n, v)| (n.as_str(), *v)).collect();
        self.order
            .iter()
            .map(|name| match lookup.get(name.as_str()) {
                Some(&v) => self.params[name].normalize(v),
                None => 0.0,
            })
            .collect()
    }
}

/// Fit PCA on a per-kind embedding matrix (`n_samples × dim`) and project it,
/// keeping the smallest number of components `k` whose cumulative explained
/// variance reaches `target_variance`, capped at `max_k`. Returns `(k,
/// variance_captured, projected)` where `projected` is `n_samples × k` and
/// `variance_captured` is the cumulative explained-variance ratio of those `k`
/// components (may be below `target_variance` when the corpus can't reach it).
///
/// Returns `k = 0` (variance `0.0`, an `n × 0` matrix) when there are too few
/// samples to fit, or if the decomposition fails — the variable-length
/// `pca_dims` column tolerates a kind with no PCA dimensions.
pub fn fit_project_pca(
    embeddings: &Array2<f64>,
    target_variance: f64,
    max_k: usize,
) -> (usize, f64, Array2<f64>) {
    let n = embeddings.nrows();
    let d = embeddings.ncols();
    // linfa requires embedding_size <= n_features; LOBPCG needs samples > k.
    let k_fit = max_k.min(d).min(n.saturating_sub(1));
    if k_fit == 0 {
        return (0, 0.0, Array2::zeros((n, 0)));
    }

    let pca = match Pca::params(k_fit).fit(&Dataset::from(embeddings.clone())) {
        Ok(p) => p,
        Err(_) => return (0, 0.0, Array2::zeros((n, 0))),
    };

    let projected_full = pca.transform(Dataset::from(embeddings.clone()));
    // For rank-deficient input LOBPCG may return fewer components than
    // requested, so the projection can be narrower than `explained_variance_ratio`.
    let available = projected_full.records().ncols();

    let ratios = pca.explained_variance_ratio();
    let mut cumulative = 0.0;
    let mut k = available;
    for (i, r) in ratios.iter().enumerate() {
        cumulative += *r;
        if cumulative >= target_variance {
            k = i + 1;
            break;
        }
    }
    let k = k.min(available);
    // Variance actually captured by the `k` components we keep.
    let variance_captured = ratios.iter().take(k).sum::<f64>();
    let projected = projected_full.records().slice(s![.., ..k]).to_owned();
    (k, variance_captured, projected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zscore_centers_and_scales() {
        let p = ZScoreParams::fit(&[1.0, 2.0, 3.0]);
        assert!((p.mean - 2.0).abs() < 1e-12);
        assert!((p.normalize(2.0)).abs() < 1e-12);
        assert!(p.normalize(3.0) > 0.0);
    }

    #[test]
    fn zscore_constant_feature_normalizes_to_zero() {
        let p = ZScoreParams::fit(&[5.0, 5.0, 5.0]);
        assert_eq!(p.std, 0.0);
        assert_eq!(p.normalize(5.0), 0.0);
        assert_eq!(p.normalize(9.0), 0.0); // never divides by ~0
    }

    #[test]
    fn normalizer_unions_features_and_imputes_missing() {
        let rows = vec![
            vec![("a".to_string(), 0.0), ("b".to_string(), 10.0)],
            vec![("a".to_string(), 2.0)], // missing "b"
        ];
        let norm = Normalizer::fit(&rows);
        assert_eq!(norm.feature_order(), &["a".to_string(), "b".to_string()]);
        // Row missing "b" imputes it to 0 (the mean of the single observed b).
        let v = norm.normalize_row(&rows[1]);
        assert_eq!(v.len(), 2);
        assert_eq!(v[1], 0.0);
    }

    #[test]
    fn pca_picks_one_component_when_one_direction_dominates() {
        // Column 0 carries almost all the variance; the others are tiny but
        // nonzero (full-rank, like a real embedding subspace).
        let x = Array2::from_shape_fn((10, 3), |(i, j)| match j {
            0 => i as f64 * 10.0,
            1 => (i % 2) as f64 * 0.01,
            _ => (i % 3) as f64 * 0.01,
        });
        let (k, variance, projected) = fit_project_pca(&x, 0.80, 7);
        assert_eq!(k, 1, "one dominant direction should reach 80% at k=1");
        assert!(variance >= 0.80, "kept component must capture ≥80%, got {variance}");
        assert_eq!(projected.dim(), (10, 1));
    }

    #[test]
    fn pca_skips_when_too_few_samples() {
        let x = Array2::from_shape_fn((1, 3), |_| 1.0);
        let (k, variance, projected) = fit_project_pca(&x, 0.80, 7);
        assert_eq!(k, 0);
        assert_eq!(variance, 0.0);
        assert_eq!(projected.dim(), (1, 0));
    }
}
