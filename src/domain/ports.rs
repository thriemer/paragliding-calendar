use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;

use crate::domain::{
    activities::{ActivityKind, ActivitySuggestion, TimeWindow},
    calendar::CalendarEvent,
    location::Location,
    plan::{Plan, PlanningContext, ScheduledActivity},
    preferences::KindModel,
    paragliding::ParaglidingSite,
    weather::{WeatherForecast, WeatherModel},
};

pub struct SolverInput {
    pub candidates: Vec<ActivitySuggestion>,
    pub origin: Location,
    pub free_slots: Vec<TimeWindow>,
    /// Fixed calendar commitments (fun = 0) the plan must schedule around.
    pub fixed: Vec<ScheduledActivity>,
    pub num_alternatives: usize,
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait WeekSolver: Send + Sync {
    async fn solve(&self, input: SolverInput) -> Result<Vec<Plan>>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ActivitySource: Send + Sync {
    async fn suggest(&self, ctx: &PlanningContext) -> Result<Vec<ActivitySuggestion>>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait WeatherProvider: Send + Sync {
    async fn get_forecast(
        &self,
        source: Location,
        model: Option<String>,
    ) -> Result<WeatherForecast>;

    fn available_models(&self) -> Vec<WeatherModel>;
}

/// Real drive-time lookup for a single leg. The GA plans on cheap crow-flies estimates
/// (`application::solvers::placement::crow_flies_drive`); this port is only hit to put real
/// numbers on the final rendered plan.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait RoutingProvider: Send + Sync {
    async fn get_travel_time(&self, source: &Location, destination: &Location) -> Result<Duration>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait CalendarProvider: Send + Sync {
    /// Concrete events in `[start, end]` across `calendars`, expanded from recurrences.
    async fn get_events(
        &self,
        calendars: &[String],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<CalendarEvent>>;
    async fn get_calendar_names(&self) -> Result<Vec<String>>;
    async fn clear_calendar(&self, name: &str) -> Result<()>;
    async fn create_event(&self, calendar: &str, event: CalendarEvent) -> Result<()>;
    async fn create_calendar(&self, name: &str) -> Result<()>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait GeoProvider: Send + Sync {
    async fn geocode(&self, location_name: &str) -> Result<Vec<Location>>;

    async fn fetch_elevation(&self, latitude: f64, longitude: f64) -> Result<f64>;
}

use crate::domain::{happening::Happening, settings::UserSettings, tour::Tour};

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait TourRepository: Send + Sync {
    async fn count(&self) -> Result<i64>;
    async fn save_batch(&self, tours: Vec<Tour>) -> Result<usize>;
    /// Every tour — used by the batch feature-embedding pipeline.
    async fn find_all(&self) -> Result<Vec<Tour>>;
    async fn find_within_radius(
        &self,
        center: &Location,
        radius_km: f64,
    ) -> Result<Vec<(Tour, f64)>>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait SiteRepository: Send + Sync {
    async fn save(&self, site: ParaglidingSite) -> Result<()>;
    async fn delete(&self, name: &str) -> Result<()>;
    async fn count(&self) -> Result<i64>;
    async fn find_all(&self) -> Result<Vec<ParaglidingSite>>;
    async fn find_within_radius(
        &self,
        center: &Location,
        radius_km: f64,
    ) -> Result<Vec<(ParaglidingSite, f64)>>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait HappeningRepository: Send + Sync {
    async fn count(&self) -> Result<i64>;
    async fn save_batch(&self, events: Vec<Happening>) -> Result<usize>;
    /// Every happening — used by the batch feature-embedding pipeline.
    async fn find_all(&self) -> Result<Vec<Happening>>;
    async fn find_within_radius_and_time(
        &self,
        center: &Location,
        radius_km: f64,
        time_from: DateTime<Utc>,
        time_to: DateTime<Utc>,
    ) -> Result<Vec<(Happening, f64)>>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait SettingsRepository: Send + Sync {
    async fn get(&self) -> Result<Option<UserSettings>>;
    async fn save(&self, settings: &UserSettings) -> Result<()>;
}

/// Bulk pull of outdoor-active tour/event data from the upstream feed. One trait, both methods:
/// same external source, one adapter. Returns parsed domain data — no persistence side effects.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait CatalogFeed: Send + Sync {
    async fn fetch_tours(&self) -> Result<Vec<Tour>>;
    async fn fetch_happenings(&self) -> Result<Vec<Happening>>;
}

/// Local sentence-embedding model. `f64` at the boundary — the adapter upcasts
/// the model's native `f32` (see [`crate::domain::features`] for why the
/// pipeline is `f64`).
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait Embedder: Send + Sync {
    /// Batch-embed descriptions into one vector each, order-preserving.
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f64>>>;

    /// Batch-embed images (raw encoded bytes: JPEG/PNG/WebP) into one vector each,
    /// order-preserving, in the **same** space as [`Embedder::embed_batch`] so the
    /// two can be fused. Default: unsupported — only the CLIP backend overrides it;
    /// text-only backends return an error, and callers fall back to text-only.
    async fn embed_image_batch(&self, _images: &[Vec<u8>]) -> Result<Vec<Vec<f64>>> {
        anyhow::bail!("image embedding not supported by this embedder")
    }
}

/// One fully-processed activity: the raw embedding kept for re-fit reuse, the
/// per-kind PCA-reduced dims, and the normalized feature vector. Mirrors a row
/// of `activity_embeddings`.
#[derive(Debug, Clone)]
pub struct ActivityEmbeddingRow {
    pub activity_id: String,
    pub kind: ActivityKind,
    pub embedding: Vec<f64>,
    pub pca_dims: Vec<f64>,
    pub features: Vec<f64>,
}

/// Persistence for the preference subsystem: the raw feedback (`preference_comparisons`,
/// `preference_ratings`) and the learned model (`preference_model`). One cohesive
/// port — all three tables are the preference domain's storage, written/read by
/// the same adapter. The Phase 3 solver consumes the feedback and writes the model.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait PreferenceRepository: Send + Sync {
    /// Record one pairwise outcome (`winner` preferred over `loser`).
    async fn record_comparison(&self, winner_id: &str, loser_id: &str) -> Result<()>;
    /// Record one post-activity 1-5 rating.
    async fn record_rating(&self, activity_id: &str, rating: i16) -> Result<()>;
    async fn count_comparisons(&self) -> Result<i64>;
    async fn count_ratings(&self) -> Result<i64>;
    /// Appearances per `activity_id` across winner+loser columns — powers the
    /// "prefer under-compared activities" anchor weighting in pair selection.
    async fn comparison_counts(&self) -> Result<HashMap<String, i64>>;
    /// Every `(winner_id, loser_id)` outcome — the Phase 3 solver's training set.
    async fn list_comparisons(&self) -> Result<Vec<(String, String)>>;
    /// Every `(activity_id, rating)` (rating 1..=5) — the solver's rating term.
    async fn list_ratings(&self) -> Result<Vec<(String, i16)>>;
    /// The learned model, one entry per kind. Empty until the Phase 3 solver runs.
    async fn load_model(&self) -> Result<Vec<KindModel>>;
    /// Upsert the model, one row per kind (base preference + feature weights with
    /// their normalizers). Written by the batch job (feature scaffold, weights 0)
    /// and by the solver (learned weights).
    async fn save_model(&self, models: &[KindModel]) -> Result<()>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait EmbeddingRepository: Send + Sync {
    /// Checkpoint raw text embeddings for a batch: upsert the `text_embedding`
    /// column per `(activity_id, kind)`, leaving the reduce-stage columns intact.
    /// Called once per batch by the embed job so a crash resumes from here.
    async fn upsert_text_embeddings(
        &self,
        rows: &[(String, ActivityKind, Vec<f64>)],
    ) -> Result<usize>;
    /// Every stored raw text embedding (`text_embedding IS NOT NULL`) — the embed
    /// job's resume set (which activities are done) and the reduce job's text input.
    async fn raw_text_embeddings(&self) -> Result<Vec<(String, ActivityKind, Vec<f64>)>>;
    /// Upsert fully-reduced rows (fused embedding + pca_dims + features), replacing
    /// any existing `(activity_id, kind)` while preserving the text checkpoint.
    async fn upsert_batch(&self, rows: Vec<ActivityEmbeddingRow>) -> Result<usize>;
    #[allow(dead_code)]
    async fn count(&self) -> Result<i64>;
    /// All fully-reduced rows — exercised by the persistence round-trip test.
    #[allow(dead_code)]
    async fn find_all(&self) -> Result<Vec<ActivityEmbeddingRow>>;
    /// `(activity_id, kind, normalized features)` for every reduced row, without the
    /// heavy 384-dim raw embedding. The planner's read path (Phase 4) and the
    /// solver's feature source.
    async fn feature_vectors(&self) -> Result<Vec<(String, ActivityKind, Vec<f64>)>>;
}

use crate::domain::image::{ActivityImageLink, DownloadedImage, ImageEmbedding};

/// Content-addressed blob storage for downloaded activity images. Bytes are keyed
/// by the hex digest of their content, so `put` is idempotent and identical images
/// across activities dedupe to one blob. Filesystem-backed today; the port keeps
/// object storage a drop-in later.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ImageStore: Send + Sync {
    /// Store bytes, returning their content hash (hex). Idempotent.
    async fn put(&self, bytes: &[u8]) -> Result<String>;
    /// Fetch bytes by content hash.
    async fn get(&self, hash: &str) -> Result<Vec<u8>>;
    /// Whether a blob with this hash is present.
    async fn has(&self, hash: &str) -> Result<bool>;
}

/// Persistence for `activity_images`: the per-image lifecycle (link → downloaded →
/// embedded). Links come from the source feed; the download job fills in the
/// content hash; the embedding pass (re)writes the CLIP vectors.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ImageRepository: Send + Sync {
    /// Upsert link rows (`source_url` per position), preserving any already
    /// downloaded/embedded state on conflict. Returns rows written.
    async fn upsert_links(&self, links: &[ActivityImageLink]) -> Result<usize>;
    /// Links whose bytes are not yet downloaded (`content_hash IS NULL`).
    async fn pending_downloads(&self) -> Result<Vec<ActivityImageLink>>;
    /// Record a successful download for one image.
    async fn mark_downloaded(
        &self,
        link: &ActivityImageLink,
        content_hash: &str,
        content_type: Option<String>,
        width: Option<i32>,
        height: Option<i32>,
    ) -> Result<()>;
    /// Every downloaded image (`content_hash` present) — the embed pass's input.
    #[allow(dead_code)]
    async fn all_downloaded(&self) -> Result<Vec<DownloadedImage>>;
    /// Downloaded images not yet embedded (`content_hash IS NOT NULL AND
    /// embedding IS NULL`) — the embed job's resumable image work-list. Mirror of
    /// [`ImageRepository::pending_downloads`].
    async fn pending_image_embeddings(&self) -> Result<Vec<DownloadedImage>>;
    /// (Re)write the CLIP vectors for downloaded images and stamp `embedded_at`.
    async fn store_embeddings(&self, embeddings: &[ImageEmbedding]) -> Result<()>;
    /// Every stored image CLIP vector (`embedding IS NOT NULL`) — the reduce
    /// job's image input, grouped into a per-activity mean.
    async fn all_image_embeddings(&self) -> Result<Vec<ImageEmbedding>>;
    /// `activity_id` → ordered content hashes of its downloaded images, for the
    /// comparison UI's served image URLs.
    async fn downloaded_hashes_by_activity(&self) -> Result<HashMap<String, Vec<String>>>;
}
