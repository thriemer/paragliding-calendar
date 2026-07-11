//! Preference-collection use cases (PLAN.md Phase 2b): serve informative
//! comparison pairs, record votes and ratings, and summarize the learned model.
//!
//! A snapshot of every scorable activity (display card + score) is built once
//! and cached — the sorted-by-score view backs O(1) neighbour sampling for pair
//! selection (see [`crate::domain::preferences`]). Scores come from the learned
//! `base_pref` per kind; feature-weight scoring and post-vote re-fit are Phase
//! 3/4, so until then every score is its kind's `base_pref` (0 on a fresh model)
//! and the snapshot needs no rebuild between votes. Only the dynamic
//! comparison-appearance counts change, tracked in-memory and bumped per vote.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

use anyhow::Result;
use tokio::sync::{Mutex, RwLock};

use crate::application::{feature_job, preference_fit, preference_scorer::PreferenceScorer};
use crate::domain::{
    activities::{ActivityKind, kind_from_category},
    embedding::ActivityEmbedding,
    happening::Happening,
    paragliding::{ParaglidingSite, SiteType},
    ports::{
        Embedder, EmbeddingRepository, HappeningRepository, ImageRepository, ImageStore,
        PreferenceRepository, SiteRepository, TourRepository,
    },
    preferences::{PreferenceCandidate, select_pair},
    tour::Tour,
};
/// Re-fit the Bradley-Terry model only every N votes, not after every single
/// one. The LBFGS solver is cheap (~100 iterations on 26 params), but skipping
/// intermediate refits avoids redundant work when votes arrive rapidly.
const REFIT_INTERVAL: u32 = 5;

/// A candidate pair, referenced by `Arc` (shared identity with the snapshot).
pub type CandidatePair = (Arc<PreferenceCandidate>, Arc<PreferenceCandidate>);

/// Result of recording a vote: the next pair to show plus progress.
pub struct VoteOutcome {
    pub next: Option<CandidatePair>,
    pub comparisons_done: i64,
}

/// Per-kind comparison matrix: `winner_kind_str → loser_kind_str → count`.
pub type ComparisonMatrix = std::collections::HashMap<String, std::collections::HashMap<String, i64>>;

pub struct KindSummary {
    pub kind: ActivityKind,
    pub base_pref: f64,
    pub activity_count: usize,
    /// Every learned feature weight for this kind (not just top N).
    pub features: Vec<FeatureSummary>,
}

pub struct FeatureSummary {
    pub feature: String,
    pub weight: f64,
    pub higher_is_better: bool,
}

pub struct PreferenceSummary {
    pub comparisons_done: i64,
    pub ratings_done: i64,
    pub kinds: Vec<KindSummary>,
}

/// Immutable scored-candidate snapshot, sorted ascending by score so that
/// adjacent entries are the closest-scoring (most informative) pairs.
struct Snapshot {
    sorted: Vec<Arc<PreferenceCandidate>>,
    kind_counts: HashMap<ActivityKind, usize>,
}

pub struct PreferenceService {
    tours: Arc<dyn TourRepository>,
    events: Arc<dyn HappeningRepository>,
    sites: Arc<dyn SiteRepository>,
    prefs: Arc<dyn PreferenceRepository>,
    embeddings: Arc<dyn EmbeddingRepository>,
    images: Arc<dyn ImageRepository>,
    image_store: Arc<dyn ImageStore>,
    /// Plan-time scorer, hot-reloaded here after each vote so the planner and the
    /// comparison UI stay in sync with the freshly-fit model.
    scorer: Arc<PreferenceScorer>,
    snapshot: RwLock<Option<Arc<Snapshot>>>,
    /// Dynamic per-activity comparison-appearance counts (seeded from the DB on
    /// first snapshot build, bumped in-memory per vote).
    counts: Mutex<HashMap<String, i64>>,
    /// How many votes have been recorded since the last refit. On every
    /// `REFIT_INTERVAL`-th vote the model is re-derived; between refits the
    /// accumulated comparisons are stored in the DB but not yet fitted.
    pending_refit: AtomicU32,
}

impl PreferenceService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tours: Arc<dyn TourRepository>,
        events: Arc<dyn HappeningRepository>,
        sites: Arc<dyn SiteRepository>,
        prefs: Arc<dyn PreferenceRepository>,
        embeddings: Arc<dyn EmbeddingRepository>,
        images: Arc<dyn ImageRepository>,
        image_store: Arc<dyn ImageStore>,
        scorer: Arc<PreferenceScorer>,
    ) -> Self {
        Self {
            tours,
            events,
            sites,
            prefs,
            embeddings,
            images,
            image_store,
            scorer,
            snapshot: RwLock::new(None),
            counts: Mutex::new(HashMap::new()),
            pending_refit: AtomicU32::new(0),
        }
    }

    /// The next informative pair, or `None` when there are fewer than two
    /// scorable activities.
    pub async fn compare(&self) -> Result<Option<CandidatePair>> {
        self.select().await
    }

    /// Record `winner` beat `loser`, re-fit the model, and serve the next pair.
    /// The caller (HTTP layer) resolves winner/loser from the opaque `pair_id`.
    pub async fn vote(&self, winner_id: &str, loser_id: &str) -> Result<VoteOutcome> {
        self.prefs.record_comparison(winner_id, loser_id).await?;
        {
            let mut counts = self.counts.lock().await;
            *counts.entry(winner_id.to_string()).or_insert(0) += 1;
            *counts.entry(loser_id.to_string()).or_insert(0) += 1;
        }
        let prev = self.pending_refit.fetch_add(1, AtomicOrdering::Relaxed);
        if prev + 1 >= REFIT_INTERVAL {
            self.refit().await;
            self.pending_refit.store(0, AtomicOrdering::Relaxed);
        }
        let comparisons_done = self.prefs.count_comparisons().await?;
        let next = self.select().await?;
        Ok(VoteOutcome {
            next,
            comparisons_done,
        })
    }

    /// Record a post-activity 1-5 rating and re-fit (ratings feed the solver's
    /// rating term).
    pub async fn rate(&self, activity_id: &str, rating: i16) -> Result<()> {
        self.prefs.record_rating(activity_id, rating).await?;
        self.refit().await;
        Ok(())
    }

    /// Re-fit the Bradley-Terry model from the accumulated feedback, reload the
    /// plan-time scorer, and drop the cached candidate snapshot so the next
    /// `/compare` re-ranks against the new scores. Failures are logged, not
    /// propagated — a fit hiccup must not fail the vote that triggered it.
    async fn refit(&self) {
        if let Err(e) = preference_fit::refit(self.prefs.as_ref(), self.embeddings.as_ref()).await {
            tracing::error!(error = ?e, "preferences: re-fit failed");
            return;
        }
        if let Err(e) = self
            .scorer
            .reload(self.prefs.as_ref(), self.embeddings.as_ref())
            .await
        {
            tracing::error!(error = ?e, "preferences: scorer reload failed");
        }
        self.invalidate().await;
    }

    /// Current model summary: tallies, per-kind base preferences with activity
    /// counts, and the strongest feature weights.
    pub async fn summary(&self) -> Result<PreferenceSummary> {
        let snapshot = self.snapshot().await?;
        let model = self.prefs.load_model().await?;
        let comparisons_done = self.prefs.count_comparisons().await?;
        let ratings_done = self.prefs.count_ratings().await?;

        let base: HashMap<ActivityKind, f64> =
            model.iter().map(|m| (m.kind, m.base_pref)).collect();

        let mut kinds: Vec<KindSummary> = snapshot
            .kind_counts
            .iter()
            .map(|(kind, count)| {
                let features = model
                    .iter()
                    .find(|m| m.kind == *kind)
                    .map(|m| {
                        m.features
                            .iter()
                            .map(|f| FeatureSummary {
                                feature: format!("{}_{}", kind.as_str(), f.name),
                                weight: f.weight,
                                higher_is_better: f.weight >= 0.0,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                KindSummary {
                    kind: *kind,
                    base_pref: base.get(kind).copied().unwrap_or(0.0),
                    activity_count: *count,
                    features,
                }
            })
            .collect();
        kinds.sort_by(|a, b| cmp_desc(a.base_pref, b.base_pref));

        Ok(PreferenceSummary {
            comparisons_done,
            ratings_done,
            kinds,
        })
    }

    /// Comparison kind-pair matrix: how many times each (winner_kind, loser_kind)
    /// has been compared. Built from the raw comparisons and a live id→kind map
    /// so it works even before the embedding pipeline has run.
    pub async fn matrix(&self) -> Result<ComparisonMatrix> {
        let mut id_kind: HashMap<String, ActivityKind> = HashMap::new();
        for tour in self.tours.find_all().await? {
            if let Some(kind) = kind_from_category(&tour.category) {
                id_kind.insert(tour.id.clone(), kind);
            }
        }
        for happening in self.events.find_all().await? {
            id_kind.insert(happening.id.clone(), ActivityKind::Event);
        }
        for site in self.sites.find_all().await? {
            id_kind.insert(site.name.clone(), ActivityKind::Paragliding);
        }

        let comparisons = self.prefs.list_comparisons().await?;
        let mut matrix: ComparisonMatrix = HashMap::new();
        for (winner, loser) in &comparisons {
            let wk = id_kind
                .get(winner)
                .map(|k| k.as_str().to_string())
                .unwrap_or_else(|| "unknown".into());
            let lk = id_kind
                .get(loser)
                .map(|k| k.as_str().to_string())
                .unwrap_or_else(|| "unknown".into());
            *matrix.entry(wk.clone()).or_default().entry(lk.clone()).or_insert(0) += 1;
            // Symmetrical: increment the opposite cell too so every cell
            // holds the total comparisons between the two kinds.
            if wk != lk {
                *matrix.entry(lk).or_default().entry(wk).or_insert(0) += 1;
            }
        }
        Ok(matrix)
    }

    /// Re-run the full feature-embedding pipeline and reload the scorer. Embeds
    /// whatever images are already downloaded — fetching missing images is the
    /// separate, standalone [`image_job`] (see `main.rs`), decoupled from embedding
    /// so a re-embed never re-downloads.
    pub async fn re_embed(&self, embedder: &dyn Embedder, batch_size: usize) -> Result<usize> {
        let saved = feature_job::run(
            self.tours.as_ref(),
            self.events.as_ref(),
            self.sites.as_ref(),
            embedder,
            self.embeddings.as_ref(),
            self.prefs.as_ref(),
            self.images.as_ref(),
            self.image_store.as_ref(),
            batch_size,
        )
        .await?;
        self.scorer
            .reload(self.prefs.as_ref(), self.embeddings.as_ref())
            .await?;
        self.invalidate().await;
        Ok(saved)
    }

    /// Drop the cached snapshot so the next request rebuilds it against the
    /// current scores. Called by [`PreferenceService::refit`] after a re-fit.
    async fn invalidate(&self) {
        *self.snapshot.write().await = None;
    }

    async fn select(&self) -> Result<Option<CandidatePair>> {
        let snapshot = self.snapshot().await?;
        let counts = self.counts.lock().await;

        // Build kind-pair comparison matrix from stored comparisons so that
        // select_pair can prefer under-compared kind-pairs.
        let id_kind: HashMap<&str, ActivityKind> = snapshot
            .sorted
            .iter()
            .map(|c| (c.id.as_str(), c.kind))
            .collect();
        let comparisons = self.prefs.list_comparisons().await?;
        let mut matrix: HashMap<String, HashMap<String, i64>> = HashMap::new();
        for (winner, loser) in &comparisons {
            if let (Some(wk), Some(lk)) = (
                id_kind.get(winner.as_str()),
                id_kind.get(loser.as_str()),
            ) {
                let wk_s = wk.as_str().to_string();
                let lk_s = lk.as_str().to_string();
                *matrix.entry(wk_s.clone()).or_default().entry(lk_s.clone()).or_insert(0) += 1;
                if wk_s != lk_s {
                    *matrix.entry(lk_s).or_default().entry(wk_s).or_insert(0) += 1;
                }
            }
        }

        let mut rng = rand::rng();
        Ok(select_pair(&snapshot.sorted, &counts, Some(&matrix), &mut rng)
            .map(|(a, b)| (a.clone(), b.clone())))
    }

    async fn snapshot(&self) -> Result<Arc<Snapshot>> {
        if let Some(existing) = self.snapshot.read().await.clone() {
            return Ok(existing);
        }
        let built = Arc::new(self.build_snapshot().await?);
        *self.counts.lock().await = self.prefs.comparison_counts().await?;
        *self.snapshot.write().await = Some(built.clone());
        Ok(built)
    }

    async fn build_snapshot(&self) -> Result<Snapshot> {
        // Full learned scores (base_pref + feature weights) via the plan-time
        // scorer, so the comparison UI ranks candidates exactly as the planner
        // does. The scorer is reloaded after every vote, so it is current here.
        let scorer = self.scorer.snapshot();

        // activity_id → ordered content hashes of its downloaded images, so each
        // card can render its gallery from our own store.
        let mut hashes = self.images.downloaded_hashes_by_activity().await?;
        let mut take_hashes = |id: &str| hashes.remove(id).unwrap_or_default();

        let mut sorted: Vec<Arc<PreferenceCandidate>> = Vec::new();
        for tour in self.tours.find_all().await? {
            if let Some(kind) = kind_from_category(&tour.category) {
                let score = scorer.raw_score(kind, &tour.id);
                let imgs = take_hashes(&tour.id);
                sorted.push(Arc::new(candidate_from_tour(&tour, kind, score, imgs)));
            }
        }
        for happening in self.events.find_all().await? {
            let score = scorer.raw_score(ActivityKind::Event, &happening.id);
            let imgs = take_hashes(&happening.id);
            sorted.push(Arc::new(candidate_from_happening(&happening, score, imgs)));
        }
        for site in self.sites.find_all().await? {
            let score = scorer.raw_score(ActivityKind::Paragliding, &site.name);
            sorted.push(Arc::new(candidate_from_site(&site, score)));
        }

        // Ascending score → neighbours in the vec are the closest-scoring pairs.
        sorted.sort_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(Ordering::Equal));

        let mut kind_counts: HashMap<ActivityKind, usize> = HashMap::new();
        for c in &sorted {
            *kind_counts.entry(c.kind).or_insert(0) += 1;
        }
        Ok(Snapshot {
            sorted,
            kind_counts,
        })
    }
}

/// Descending comparison on `f64`, treating NaN as equal (keeps sort total).
fn cmp_desc(a: f64, b: f64) -> Ordering {
    b.partial_cmp(&a).unwrap_or(Ordering::Equal)
}

fn format_minutes(minutes: u32) -> String {
    let (h, m) = (minutes / 60, minutes % 60);
    match (h, m) {
        (0, m) => format!("{m}min"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}min"),
    }
}

fn candidate_from_tour(
    tour: &Tour,
    kind: ActivityKind,
    score: f64,
    image_hashes: Vec<String>,
) -> PreferenceCandidate {
    PreferenceCandidate {
        id: tour.id.clone(),
        kind,
        title: tour.title.clone(),
        description: tour.description(),
        stats: vec![
            ("duration".into(), format_minutes(tour.duration_minutes)),
            ("difficulty".into(), format!("{}/6", tour.difficulty)),
            ("ascent".into(), format!("{} m", tour.ascent_meters)),
            (
                "length".into(),
                format!("{:.1} km", tour.length_meters as f64 / 1000.0),
            ),
        ],
        image_hashes,
        score,
    }
}

fn candidate_from_happening(
    happening: &Happening,
    score: f64,
    image_hashes: Vec<String>,
) -> PreferenceCandidate {
    let mut stats = Vec::new();
    if let Some(category) = &happening.category_title {
        stats.push(("category".into(), category.clone()));
    }
    PreferenceCandidate {
        id: happening.id.clone(),
        kind: ActivityKind::Event,
        title: happening.title.clone(),
        description: happening.description(),
        stats,
        image_hashes,
        score,
    }
}

fn candidate_from_site(site: &ParaglidingSite, score: f64) -> PreferenceCandidate {
    let mut stats = vec![
        ("launches".into(), site.launches.len().to_string()),
        ("landings".into(), site.landings.len().to_string()),
    ];
    if let Some(max_elev) = site
        .launches
        .iter()
        .map(|l| l.elevation)
        .fold(None, |acc: Option<f64>, e| Some(acc.map_or(e, |a| a.max(e))))
    {
        let min_landing = site
            .landings
            .iter()
            .map(|l| l.elevation)
            .fold(None, |acc: Option<f64>, e| Some(acc.map_or(e, |a| a.min(e))));
        let pseudo = max_elev / 2.0;
        let diff = max_elev - min_landing.unwrap_or(pseudo);
        stats.push(("height diff".into(), format!("{} m", diff.round() as i64)));
    }
    if site
        .launches
        .iter()
        .any(|l| matches!(l.site_type, SiteType::Winch))
    {
        stats.push(("winch".into(), "yes".into()));
    }
    PreferenceCandidate {
        id: site.name.clone(),
        kind: ActivityKind::Paragliding,
        title: site.name.clone(),
        description: site.description(),
        stats,
        // Paragliding sites carry no images.
        image_hashes: vec![],
        score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::persistence::postgres::PostgresRepository;
    use crate::domain::location::Location;
    use crate::domain::paragliding::{ParaglidingLaunch, ParaglidingSite, SiteType};
    use crate::test_support::test_pool;

    #[test]
    fn format_minutes_reads_naturally() {
        assert_eq!(format_minutes(45), "45min");
        assert_eq!(format_minutes(120), "2h");
        assert_eq!(format_minutes(150), "2h 30min");
    }

    fn tour(id: &str, category: &str) -> Tour {
        Tour {
            id: id.into(),
            title: format!("Tour {id}"),
            category: category.into(),
            location: Location::new(50.0, 13.0, "L".into(), "DE".into()),
            description: format!("Beschreibung {id}"),
            duration_minutes: 150,
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

    fn site(name: &str) -> ParaglidingSite {
        ParaglidingSite {
            name: name.into(),
            launches: vec![ParaglidingLaunch {
                site_type: SiteType::Hang,
                location: Location::new(47.0, 11.0, name.into(), "DE".into()),
                direction_degrees_start: 135.0,
                direction_degrees_stop: 225.0,
                elevation: 1200.0,
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

    async fn seeded_service() -> PreferenceService {
        let repo = Arc::new(PostgresRepository::new(test_pool().await));
        TourRepository::save_batch(repo.as_ref(), vec![tour("t1", "Wanderung"), tour("t2", "Mountainbike")])
            .await
            .unwrap();
        SiteRepository::save(repo.as_ref(), site("s1")).await.unwrap();
        SiteRepository::save(repo.as_ref(), site("s2")).await.unwrap();
        let scorer = Arc::new(crate::application::preference_scorer::PreferenceScorer::new());
        let image_store: Arc<dyn crate::domain::ports::ImageStore> =
            Arc::new(crate::adapters::blob::FsImageStore::new(
                std::env::temp_dir().join(format!("travelai_prefs_imgs_{}", std::process::id())),
            ));
        PreferenceService::new(
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo.clone(),
            repo,
            image_store,
            scorer,
        )
    }

    #[tokio::test]
    async fn compare_returns_a_populated_pair() {
        let service = seeded_service().await;
        let (a, b) = service.compare().await.unwrap().expect("a pair");
        assert_ne!(a.id, b.id);
        // Cards carry display data.
        assert!(!a.title.is_empty());
        assert!(!a.description.is_empty());
    }

    #[tokio::test]
    async fn vote_records_and_advances() {
        let service = seeded_service().await;
        let (a, b) = service.compare().await.unwrap().unwrap();
        let outcome = service.vote(&a.id, &b.id).await.unwrap();
        assert_eq!(outcome.comparisons_done, 1);
        assert!(outcome.next.is_some());
    }

    #[tokio::test]
    async fn summary_reflects_kinds_and_tallies() {
        let service = seeded_service().await;
        let (a, b) = service.compare().await.unwrap().unwrap();
        service.vote(&a.id, &b.id).await.unwrap();
        service.rate("t1", 5).await.unwrap();

        let summary = service.summary().await.unwrap();
        assert_eq!(summary.comparisons_done, 1);
        assert_eq!(summary.ratings_done, 1);
        // Seeded kinds: hiking (t1), biking (t2), paragliding (s1, s2).
        let kinds: Vec<ActivityKind> = summary.kinds.iter().map(|k| k.kind).collect();
        assert!(kinds.contains(&ActivityKind::Paragliding));
        assert!(kinds.contains(&ActivityKind::Hiking));
        let paragliding = summary
            .kinds
            .iter()
            .find(|k| k.kind == ActivityKind::Paragliding)
            .unwrap();
        assert_eq!(paragliding.activity_count, 2);
        // Empty model → no learned feature weights per kind.
        for kind in &summary.kinds {
            assert!(kind.features.is_empty());
        }
    }
}
