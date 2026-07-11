//! Bradley-Terry preference solver (PLAN.md Phase 3).
//!
//! Standard BT learns one strength per item and cannot score unseen activities.
//! Instead we learn, per activity kind, a base preference plus one weight per
//! feature, so `score(a) = base_pref[kind_a] + w[kind_a]·features_a` generalizes
//! to any activity with a known feature vector. This is logistic regression on
//! pairwise difference vectors, extended with a rating term — a convex
//! multi-task objective we hand-write and hand to `argmin`'s LBFGS (gradient
//! only, no Hessian, no matrix inverse → no LAPACK on the Pi).
//!
//! ## Deviations from the draft objective (PLAN.md §Phase 3)
//!
//! The plan's rating loss `0.5·(r − clamp(score,1,5))²` is broken two ways: the
//! `clamp` has zero gradient outside [1,5] (LBFGS can't learn there), and it
//! compares a 1–5 rating against a log-odds score (~−5..+5) — different scales
//! that would fight the pairwise term. We instead map a rating to a *log-odds
//! target* `t(r) = (r − 3)` (so 3★ ↦ 0, 5★ ↦ +2, 1★ ↦ −2) and use
//! `0.5·(score − t(r))²`. That is smooth, convex, and commensurate with the
//! pairwise term. Everything else (α, λ, parameter layout, write-back) follows
//! the plan.

use std::collections::HashMap;

use argmin::core::{CostFunction, Error, Executor, Gradient, State};
use argmin::solver::linesearch::MoreThuenteLineSearch;
use argmin::solver::quasinewton::LBFGS;

use crate::domain::activities::ActivityKind;
use crate::domain::preferences::{FeatureWeight, KindModel};

/// Rating weight α — pairwise comparisons dominate early (PLAN.md default 0.3).
pub const RATING_WEIGHT: f64 = 0.3;
/// L2 regularization λ — keeps the ~26-parameter fit well-posed against ~30
/// comparisons and pins the otherwise shift-invariant base preferences.
pub const L2_LAMBDA: f64 = 0.01;
/// LBFGS memory length and iteration cap — the problem is small and convex.
const LBFGS_MEMORY: usize = 7;
const MAX_ITERS: u64 = 200;

/// Everything the solver consumes. `scaffold` fixes which kinds exist, how many
/// feature weights each has, and the feature order (names + normalizers carried
/// through untouched). `features` maps an `activity_id` to its kind and
/// normalized feature vector — the same vectors stored in `activity_embeddings`.
pub struct TrainingData {
    pub scaffold: Vec<KindModel>,
    pub features: HashMap<String, (ActivityKind, Vec<f64>)>,
    /// `(winner_id, loser_id)` pairwise outcomes.
    pub comparisons: Vec<(String, String)>,
    /// `(activity_id, rating)` with `rating` in 1..=5.
    pub ratings: Vec<(String, i16)>,
}

/// Fit the model. Returns updated [`KindModel`]s (same kinds, order, feature
/// names and normalizers as `scaffold`, with learned `base_pref` and `weight`s).
/// With no comparisons and no ratings the scaffold is returned unchanged (the
/// regularized optimum is all-zero anyway). A solver error is non-fatal: it logs
/// and returns the scaffold so a bad fit never takes down a plan request.
pub fn fit(data: &TrainingData, alpha: f64, lambda: f64) -> Vec<KindModel> {
    if data.comparisons.is_empty() && data.ratings.is_empty() {
        return data.scaffold.clone();
    }

    let layout = Layout::new(&data.scaffold);
    if layout.n_params == 0 {
        return data.scaffold.clone();
    }

    let problem = BtProblem::compile(data, &layout, alpha, lambda);
    if problem.comparisons.is_empty() && problem.ratings.is_empty() {
        // Every observation referenced an un-embedded activity — nothing to fit.
        tracing::warn!("preference_fit: no observations resolved to feature vectors; keeping scaffold");
        return data.scaffold.clone();
    }

    let init = vec![0.0_f64; layout.n_params];
    let solver = LBFGS::new(MoreThuenteLineSearch::new(), LBFGS_MEMORY);
    let params = match Executor::new(problem, solver)
        .configure(|s| s.param(init.clone()).max_iters(MAX_ITERS))
        .run()
    {
        Ok(res) => res.state().get_best_param().cloned().unwrap_or(init),
        Err(e) => {
            tracing::error!(error = %e, "preference_fit: LBFGS failed; keeping scaffold");
            return data.scaffold.clone();
        }
    };

    layout.unpack(&data.scaffold, &params)
}

/// Parameter layout `[base_pref(kind_0..K), weights(kind_0), weights(kind_1), …]`.
/// Kind order follows the scaffold; each kind contributes one base slot plus one
/// slot per feature.
struct Layout {
    /// Per scaffold index: `(base_param_index, feature_start_index, n_features)`.
    slots: Vec<(usize, usize, usize)>,
    n_params: usize,
}

impl Layout {
    fn new(scaffold: &[KindModel]) -> Self {
        let k = scaffold.len();
        let mut slots = Vec::with_capacity(k);
        // Bases occupy 0..K; feature blocks follow contiguously after them.
        let mut feat_cursor = k;
        for (i, m) in scaffold.iter().enumerate() {
            let n = m.features.len();
            slots.push((i, feat_cursor, n));
            feat_cursor += n;
        }
        Self {
            slots,
            n_params: feat_cursor,
        }
    }

    fn unpack(&self, scaffold: &[KindModel], params: &[f64]) -> Vec<KindModel> {
        scaffold
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let (base_idx, feat_start, n) = self.slots[i];
                let features = m
                    .features
                    .iter()
                    .enumerate()
                    .map(|(j, fw)| FeatureWeight {
                        name: fw.name.clone(),
                        weight: params[feat_start + j],
                        norm_mean: fw.norm_mean,
                        norm_std: fw.norm_std,
                    })
                    .collect();
                debug_assert_eq!(n, m.features.len());
                KindModel {
                    kind: m.kind,
                    base_pref: params[base_idx],
                    features,
                }
            })
            .collect()
    }
}

/// An observation resolved to its parameter indices + feature vector, so cost
/// and gradient evaluations (called many times by LBFGS) avoid all map lookups.
struct Resolved {
    base_idx: usize,
    feat_start: usize,
    feats: Vec<f64>,
}

impl Resolved {
    fn score(&self, p: &[f64]) -> f64 {
        let mut s = p[self.base_idx];
        for (i, &x) in self.feats.iter().enumerate() {
            s += p[self.feat_start + i] * x;
        }
        s
    }

    /// Add `g` times this activity's `∂score/∂params` into `grad`.
    fn accumulate(&self, g: f64, grad: &mut [f64]) {
        grad[self.base_idx] += g;
        for (i, &x) in self.feats.iter().enumerate() {
            grad[self.feat_start + i] += g * x;
        }
    }
}

/// The compiled convex objective handed to argmin.
struct BtProblem {
    comparisons: Vec<(Resolved, Resolved)>,
    ratings: Vec<(Resolved, f64)>,
    n_params: usize,
    alpha: f64,
    lambda: f64,
}

impl BtProblem {
    fn compile(data: &TrainingData, layout: &Layout, alpha: f64, lambda: f64) -> Self {
        // Map each kind to its scaffold slot so an id → kind → indices lookup is O(1).
        let slot_of: HashMap<ActivityKind, (usize, usize, usize)> = data
            .scaffold
            .iter()
            .enumerate()
            .map(|(i, m)| (m.kind, layout.slots[i]))
            .collect();

        let resolve = |id: &str| -> Option<Resolved> {
            let (kind, vec) = data.features.get(id)?;
            let &(base_idx, feat_start, n) = slot_of.get(kind)?;
            // Pair only the overlapping prefix: a stale vector longer/shorter
            // than the scaffold still scores on the features they share.
            let take = n.min(vec.len());
            Some(Resolved {
                base_idx,
                feat_start,
                feats: vec[..take].to_vec(),
            })
        };

        let mut comparisons = Vec::new();
        let mut dropped = 0usize;
        for (w, l) in &data.comparisons {
            match (resolve(w), resolve(l)) {
                (Some(w), Some(l)) => comparisons.push((w, l)),
                _ => dropped += 1,
            }
        }
        let mut ratings = Vec::new();
        for (id, r) in &data.ratings {
            match resolve(id) {
                Some(a) => ratings.push((a, (*r as f64) - 3.0)),
                None => dropped += 1,
            }
        }
        if dropped > 0 {
            tracing::warn!(dropped, "preference_fit: observations skipped (activity not embedded)");
        }

        Self {
            comparisons,
            ratings,
            n_params: layout.n_params,
            alpha,
            lambda,
        }
    }
}

/// Numerically stable `ln(1 + eˣ)`.
fn softplus(x: f64) -> f64 {
    x.max(0.0) + (-x.abs()).exp().ln_1p()
}

fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

impl CostFunction for BtProblem {
    type Param = Vec<f64>;
    type Output = f64;

    fn cost(&self, p: &Vec<f64>) -> Result<f64, Error> {
        let mut loss = 0.0;
        // Pairwise NLL: −ln σ(score_w − score_l) = softplus(−(score_w − score_l)).
        for (w, l) in &self.comparisons {
            loss += softplus(-(w.score(p) - l.score(p)));
        }
        // Rating MSE against the log-odds target.
        for (a, t) in &self.ratings {
            let d = a.score(p) - t;
            loss += self.alpha * 0.5 * d * d;
        }
        // L2 on all parameters (weights and bases).
        loss += self.lambda * p.iter().map(|v| v * v).sum::<f64>();
        Ok(loss)
    }
}

impl Gradient for BtProblem {
    type Param = Vec<f64>;
    type Gradient = Vec<f64>;

    fn gradient(&self, p: &Vec<f64>) -> Result<Vec<f64>, Error> {
        let mut grad = vec![0.0_f64; self.n_params];
        for (w, l) in &self.comparisons {
            let d = w.score(p) - l.score(p);
            // ∂/∂d [−ln σ(d)] = σ(d) − 1; chain onto each side of d = s_w − s_l.
            let g = sigmoid(d) - 1.0;
            w.accumulate(g, &mut grad);
            l.accumulate(-g, &mut grad);
        }
        for (a, t) in &self.ratings {
            let g = self.alpha * (a.score(p) - t);
            a.accumulate(g, &mut grad);
        }
        // d/dθ [λ Σθ²] = 2λθ.
        for (gi, &pi) in grad.iter_mut().zip(p.iter()) {
            *gi += 2.0 * self.lambda * pi;
        }
        Ok(grad)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fw(name: &str) -> FeatureWeight {
        FeatureWeight {
            name: name.into(),
            weight: 0.0,
            norm_mean: 0.0,
            norm_std: 1.0,
        }
    }

    fn scaffold_one_kind(kind: ActivityKind, feats: &[&str]) -> KindModel {
        KindModel {
            kind,
            base_pref: 0.0,
            features: feats.iter().map(|n| fw(n)).collect(),
        }
    }

    #[test]
    fn no_data_returns_scaffold_unchanged() {
        let scaffold = vec![scaffold_one_kind(ActivityKind::Hiking, &["a"])];
        let data = TrainingData {
            scaffold: scaffold.clone(),
            features: HashMap::new(),
            comparisons: vec![],
            ratings: vec![],
        };
        assert_eq!(fit(&data, RATING_WEIGHT, L2_LAMBDA), scaffold);
    }

    #[test]
    fn learns_positive_weight_for_preferred_direction() {
        // One kind, one feature. Higher feature always wins → weight should be > 0.
        let mut features = HashMap::new();
        features.insert("hi".to_string(), (ActivityKind::Hiking, vec![2.0]));
        features.insert("lo".to_string(), (ActivityKind::Hiking, vec![-2.0]));
        let data = TrainingData {
            scaffold: vec![scaffold_one_kind(ActivityKind::Hiking, &["landscape"])],
            features,
            // Repeat the outcome so it dominates the L2 prior.
            comparisons: vec![("hi".into(), "lo".into()); 20],
            ratings: vec![],
        };
        let model = fit(&data, RATING_WEIGHT, L2_LAMBDA);
        assert_eq!(model.len(), 1);
        assert!(
            model[0].features[0].weight > 0.5,
            "expected a strong positive weight, got {}",
            model[0].features[0].weight
        );
    }

    #[test]
    fn cross_kind_comparisons_lift_the_winning_kind_base() {
        // Featureless kinds: only base_pref can explain the outcome.
        let mut features = HashMap::new();
        features.insert("fly".to_string(), (ActivityKind::Paragliding, vec![]));
        features.insert("walk".to_string(), (ActivityKind::Hiking, vec![]));
        let data = TrainingData {
            scaffold: vec![
                scaffold_one_kind(ActivityKind::Paragliding, &[]),
                scaffold_one_kind(ActivityKind::Hiking, &[]),
            ],
            features,
            comparisons: vec![("fly".into(), "walk".into()); 20],
            ratings: vec![],
        };
        let model = fit(&data, RATING_WEIGHT, L2_LAMBDA);
        let para = model.iter().find(|m| m.kind == ActivityKind::Paragliding).unwrap();
        let hike = model.iter().find(|m| m.kind == ActivityKind::Hiking).unwrap();
        assert!(
            para.base_pref > hike.base_pref,
            "paragliding base {} should beat hiking {}",
            para.base_pref,
            hike.base_pref
        );
    }

    #[test]
    fn high_ratings_raise_the_score() {
        // No comparisons: a 5★ rating should pull the activity's score above a 1★ one.
        let mut features = HashMap::new();
        features.insert("good".to_string(), (ActivityKind::Event, vec![1.0]));
        features.insert("bad".to_string(), (ActivityKind::Event, vec![-1.0]));
        let data = TrainingData {
            scaffold: vec![scaffold_one_kind(ActivityKind::Event, &["f"])],
            features: features.clone(),
            comparisons: vec![],
            ratings: vec![("good".into(), 5); 10]
                .into_iter()
                .chain(vec![("bad".into(), 1); 10])
                .collect(),
        };
        let model = fit(&data, RATING_WEIGHT, L2_LAMBDA);
        let m = &model[0];
        assert!(
            m.score(&[1.0]) > m.score(&[-1.0]),
            "5★ feature direction should outscore 1★"
        );
    }
}
