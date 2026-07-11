//! Preference-learning domain types and the informative-pair selection policy.
//!
//! The scoring model (`KindModel`) is written by the Phase 3 Bradley-Terry
//! solver; until then it is empty and every candidate scores at its kind's
//! `base_pref` (0). Pair selection is deliberately score-based and 1-D: because
//! `score` is a scalar, the closest-scoring — most informative — pairs are
//! adjacent once the candidates are sorted by score, so a neighbour-window
//! sample finds a near-minimal score gap with O(1) work per draw (no O(n²)
//! pairwise scan). See PLAN.md §2b.

use std::collections::HashMap;
use std::sync::Arc;

use rand::{Rng, RngExt};

use crate::domain::activities::ActivityKind;

/// A scored, display-ready activity for the comparison UI. Picked by `Arc<T>`
/// (never id/index) so the solver and pairing share one identity.
#[derive(Debug, Clone, PartialEq)]
pub struct PreferenceCandidate {
    /// Stable identity — `tour.id` / `happening.id` / `site.name`.
    pub id: String,
    pub kind: ActivityKind,
    pub title: String,
    pub description: String,
    /// Ordered label→value pairs shown as key stats on the card.
    pub stats: Vec<(String, String)>,
    /// Content hashes of this activity's downloaded images (position order, 0 =
    /// primary). The UI resolves each to `/api/images/{hash}`.
    pub image_hashes: Vec<String>,
    /// Log-odds preference score: `base_pref[kind] + w·features`. Feature-weight
    /// scoring lands in Phase 4; for now this is `base_pref[kind]`.
    pub score: f64,
}

/// One learned feature: its weight plus the corpus normalizer it was fit
/// against. Mirrors an element of `preference_model.features` (JSONB).
#[derive(Debug, Clone, PartialEq)]
pub struct FeatureWeight {
    pub name: String,
    pub weight: f64,
    pub norm_mean: f64,
    pub norm_std: f64,
}

/// The learned model for one activity kind: a base preference plus per-feature
/// weights. One row of `preference_model`.
#[derive(Debug, Clone, PartialEq)]
pub struct KindModel {
    pub kind: ActivityKind,
    pub base_pref: f64,
    pub features: Vec<FeatureWeight>,
}

impl KindModel {
    /// Log-odds preference score for an activity of this kind given its
    /// normalized feature vector: `base_pref + Σ wᵢ·featureᵢ`. The vector must be
    /// in the same order the model's `features` were fit against (both derive
    /// from the batch job's per-kind [`crate::domain::features::Normalizer`]);
    /// any length mismatch pairs only the overlapping prefix so a stale vector
    /// degrades gracefully rather than panicking.
    pub fn score(&self, features: &[f64]) -> f64 {
        self.base_pref
            + self
                .features
                .iter()
                .zip(features)
                .map(|(fw, x)| fw.weight * x)
                .sum::<f64>()
    }
}

/// Map a raw log-odds score (roughly −5..+5) onto a 0–100 display scale via the
/// logistic function. UI-only — the planner and solver always use raw scores
/// since only relative differences matter (PLAN.md Phase 4).
pub fn display_score(raw: f64) -> f64 {
    100.0 / (1.0 + (-raw).exp())
}

/// Fraction of pairs drawn fully at random to guarantee comparison-graph
/// connectivity (PLAN.md §2b).
const RANDOM_PAIR_PROB: f64 = 0.2;
/// Neighbour-window half-width in score order — the "close enough" knob. `w = 1`
/// is the exact nearest neighbour; larger trades a wider gap for more diversity.
const WINDOW: usize = 5;


/// Pick an informative pair of distinct candidates from a slice **sorted by
/// score**. Returns `None` when there are fewer than two candidates.
///
/// - 20% of draws are a fully random pair (connectivity).
/// - Otherwise the anchor is sampled weighted toward under-compared activities
///   (`1 / (1 + count)`), and the partner is chosen from a **different** kind
///   so the solver learns `base_pref` differences.  When multiple cross-kind
///   candidates exist, the kind-pair
///   with the fewest previous comparisons (from the optional `matrix`) is
///   preferred, and within that kind the score-nearest candidate is selected.
///   If all activities share the same kind (single-kind corpus), falls back to
///   a same-kind neighbour within the tight `±WINDOW` score window.
pub fn select_pair<'a, R: Rng>(
    sorted: &'a [Arc<PreferenceCandidate>],
    counts: &HashMap<String, i64>,
    matrix: Option<&HashMap<String, HashMap<String, i64>>>,
    rng: &mut R,
) -> Option<(&'a Arc<PreferenceCandidate>, &'a Arc<PreferenceCandidate>)> {
    let n = sorted.len();
    if n < 2 {
        return None;
    }

    if rng.random_bool(RANDOM_PAIR_PROB) {
        let (i, j) = random_distinct(n, rng);
        return Some((&sorted[i], &sorted[j]));
    }

    let anchor = weighted_anchor(sorted, counts, rng);
    let anchor_kind = sorted[anchor].kind;

    // Collect all cross-kind candidates across the whole list, grouped by kind.
    let lo = 0;
    let hi = n;
    let mut kind_indices: HashMap<ActivityKind, Vec<usize>> = HashMap::new();
    for i in lo..hi {
        if i != anchor && sorted[i].kind != anchor_kind {
            kind_indices.entry(sorted[i].kind).or_default().push(i);
        }
    }

    if !kind_indices.is_empty() {
        // Weight each kind inversely to total matrix comparisons with the anchor kind,
        // so under-compared kind-pairs are more likely to appear.
        // The matrix is symmetrical (both directions count the same total), so
        // reading one direction is sufficient.
        let total_pair = |other: ActivityKind| -> f64 {
            matrix.map_or(0.0, |m| {
                let ak = anchor_kind.as_str();
                let ok = other.as_str();
                m.get(ak)
                    .and_then(|r| r.get(ok))
                    .copied()
                    .unwrap_or(0) as f64
            })
        };

        let kind_weights: Vec<(ActivityKind, f64)> = kind_indices
            .keys()
            .map(|k| (*k, 1.0 / (1.0 + total_pair(*k))))
            .collect();

        let total_weight: f64 = kind_weights.iter().map(|(_, w)| w).sum();
        let mut target = rng.random_range(0.0..total_weight);
        let chosen_kind = kind_weights
            .iter()
            .find(|(_, w)| {
                target -= w;
                target <= 0.0
            })
            .map(|(k, _)| *k)
            .unwrap_or(kind_weights[0].0);

        // Within the chosen kind, pick the score-nearest (index-closest) candidate.
        let indices = &kind_indices[&chosen_kind];
        let partner = *indices
            .iter()
            .min_by_key(|&&i| i.abs_diff(anchor))
            .unwrap();
        return Some((&sorted[anchor], &sorted[partner]));
    }

    // Fall back to same-kind neighbour within a tight score window.
    let lo = anchor.saturating_sub(WINDOW);
    let hi = (anchor + WINDOW).min(n - 1);
    let partner = loop {
        let j = rng.random_range(lo..=hi);
        if j != anchor {
            break j;
        }
    };
    Some((&sorted[anchor], &sorted[partner]))
}

/// Two distinct indices in `0..n` (`n >= 2`).
fn random_distinct<R: Rng>(n: usize, rng: &mut R) -> (usize, usize) {
    let i = rng.random_range(0..n);
    let j = loop {
        let j = rng.random_range(0..n);
        if j != i {
            break j;
        }
    };
    (i, j)
}

/// Sample an index weighted by `1 / (1 + comparison_count)` so activities seen
/// in fewer comparisons are more likely to be chosen.
fn weighted_anchor<R: Rng>(
    sorted: &[Arc<PreferenceCandidate>],
    counts: &HashMap<String, i64>,
    rng: &mut R,
) -> usize {
    let weight = |c: &PreferenceCandidate| {
        let count = counts.get(&c.id).copied().unwrap_or(0).max(0) as f64;
        1.0 / (1.0 + count)
    };
    let total: f64 = sorted.iter().map(|c| weight(c)).sum();
    if total <= 0.0 {
        return rng.random_range(0..sorted.len());
    }
    let mut target = rng.random_range(0.0..total);
    for (i, c) in sorted.iter().enumerate() {
        target -= weight(c);
        if target <= 0.0 {
            return i;
        }
    }
    sorted.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn candidate(id: &str, score: f64) -> Arc<PreferenceCandidate> {
        Arc::new(PreferenceCandidate {
            id: id.into(),
            kind: ActivityKind::Hiking,
            title: id.into(),
            description: String::new(),
            stats: vec![],
            image_hashes: vec![],
            score,
        })
    }

    #[test]
    fn returns_none_below_two_candidates() {
        let mut rng = StdRng::seed_from_u64(1);
        let counts = HashMap::new();
        assert!(select_pair(&[], &counts, None::<&HashMap<String, HashMap<String, i64>>>, &mut rng).is_none());
        assert!(select_pair(&[candidate("a", 0.0)], &counts, None, &mut rng).is_none());
    }

    #[test]
    fn always_returns_two_distinct_candidates() {
        let sorted: Vec<_> = (0..50).map(|i| candidate(&format!("a{i}"), i as f64)).collect();
        let counts = HashMap::new();
        let mut rng = StdRng::seed_from_u64(42);
        for _ in 0..500 {
            let (a, b) = select_pair(&sorted, &counts, None, &mut rng).unwrap();
            assert_ne!(a.id, b.id, "a pair must be two distinct activities");
        }
    }

    #[test]
    fn non_random_pairs_stay_within_the_score_window() {
        // Distinct scores → index in `sorted` equals score rank. Skipping the
        // random branch, partners must sit within ±WINDOW of the anchor.
        let sorted: Vec<_> = (0..100).map(|i| candidate(&format!("a{i}"), i as f64)).collect();
        let index_of: HashMap<&str, usize> =
            sorted.iter().enumerate().map(|(i, c)| (c.id.as_str(), i)).collect();
        let counts = HashMap::new();
        let mut rng = StdRng::seed_from_u64(7);
        let mut within = 0;
        let mut total = 0;
        for _ in 0..2000 {
            let (a, b) = select_pair(&sorted, &counts, None, &mut rng).unwrap();
            let gap = index_of[a.id.as_str()].abs_diff(index_of[b.id.as_str()]);
            if gap <= WINDOW {
                within += 1;
            }
            total += 1;
        }
        // ~80% are windowed neighbours; the rest are the 20% random draws.
        let ratio = within as f64 / total as f64;
        assert!(ratio > 0.7, "expected mostly windowed pairs, got {ratio}");
    }

    #[test]
    fn anchor_weighting_favours_under_compared_activities() {
        let sorted: Vec<_> = (0..20).map(|i| candidate(&format!("a{i}"), i as f64)).collect();
        // Everything heavily compared except a0.
        let mut counts = HashMap::new();
        for i in 1..20 {
            counts.insert(format!("a{i}"), 100);
        }
        let mut rng = StdRng::seed_from_u64(99);
        let mut a0_hits = 0;
        for _ in 0..2000 {
            let anchor = weighted_anchor(&sorted, &counts, &mut rng);
            if sorted[anchor].id == "a0" {
                a0_hits += 1;
            }
        }
        // a0's weight (1.0) dwarfs the others (~0.0099 each) → picked most of the time.
        assert!(a0_hits > 1200, "under-compared a0 should dominate, got {a0_hits}");
    }
}
