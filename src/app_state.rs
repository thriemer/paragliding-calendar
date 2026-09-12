use std::sync::{
    atomic::AtomicBool,
    Arc,
};

use anyhow::Result;
use sqlx::PgPool;

use crate::{
    adapters::{
        calendar::{
            combined_calendar::CombinedCalendar,
            google_calendar::{GoogleCalendar, WebFlowAuthenticator},
            microsoft_calendar::{MicrosoftCalendar, O365Authenticator},
        },
        blob::FsImageStore,
        ingest::outdooractive_feed::OutdoorActiveFeed,
        open_meteo::OpenMeteoClient,
        persistence::{cache::PersistentCache, postgres::PostgresRepository},
        routing::Graphhopper,
    },
    application::{
        Planner,
        preference_scorer::PreferenceScorer,
        preferences::PreferenceService,
        solvers::Nsga2Solver,
        sources::ParaglidingActivitySource,
    },
    config::AppConfig,
    domain::ports::{
        ActivitySource, CalendarProvider, CatalogFeed, EmbeddingRepository, GeoProvider,
        HappeningRepository, ImageRepository, ImageStore, PreferenceRepository, RoutingProvider,
        SettingsRepository, SiteRepository, TourRepository, WeatherProvider, WeekSolver,
    },
};
#[cfg(feature = "new-activities")]
use crate::application::sources::{EventActivitySource, TourActivitySource};

#[derive(Clone)]
pub struct AppState {
    pub site_repo: Arc<dyn SiteRepository>,
    pub settings_repo: Arc<dyn SettingsRepository>,
    pub outdoor_repo: Arc<dyn TourRepository>,
    pub event_repo: Arc<dyn HappeningRepository>,
    pub embedding_repo: Arc<dyn EmbeddingRepository>,
    pub preference_repo: Arc<dyn PreferenceRepository>,
    pub image_repo: Arc<dyn ImageRepository>,
    /// Content-addressed store for downloaded activity images.
    pub image_store: Arc<dyn ImageStore>,
    /// Outdooractive `{variant}` size token + download concurrency for the image job.
    pub image_variant: String,
    pub image_download_concurrency: usize,
    /// Plan-time preference scorer; reloaded at startup, after the batch
    /// embedding job, and after every vote/rating.
    pub preference_scorer: Arc<PreferenceScorer>,
    pub outdoor_feed: Arc<dyn CatalogFeed>,
    /// Where the embedding model is stored, and inference batch size — the
    /// (lazy) `CandleEmbedder` is built from these inside the feature job.
    pub embedding_cache_dir: String,
    pub embedding_batch_size: usize,
    /// `swap(true)` to claim the re-embed job; the spawned task stores `false` when done.
    pub embedding_running: Arc<AtomicBool>,
    pub auth: Arc<WebFlowAuthenticator>,
    pub microsoft_auth: Option<Arc<O365Authenticator>>,
    pub routing: Arc<dyn RoutingProvider>,
    pub weather: Arc<dyn WeatherProvider>,
    pub geo: Arc<dyn GeoProvider>,
    pub calendar: Arc<dyn CalendarProvider>,
    pub planner: Arc<Planner>,
    pub preferences: Arc<PreferenceService>,
}

impl AppState {
    pub fn new(pool: &PgPool, cfg: &AppConfig) -> Result<Self> {
        let cache = Arc::new(PersistentCache::new(pool.clone()));

        let repo = Arc::new(PostgresRepository::new(pool.clone()));
        let site_repo: Arc<dyn SiteRepository> = repo.clone();
        let settings_repo: Arc<dyn SettingsRepository> = repo.clone();
        let outdoor_repo: Arc<dyn TourRepository> = repo.clone();
        let event_repo: Arc<dyn HappeningRepository> = repo.clone();
        let preference_repo: Arc<dyn PreferenceRepository> = repo.clone();
        let image_repo: Arc<dyn ImageRepository> = repo.clone();
        let embedding_repo: Arc<dyn EmbeddingRepository> = repo;
        let image_store: Arc<dyn ImageStore> = Arc::new(FsImageStore::new(cfg.image_store_dir.clone()));
        let outdoor_feed: Arc<dyn CatalogFeed> = Arc::new(OutdoorActiveFeed::new());

        let auth = Arc::new(WebFlowAuthenticator::new(
            cfg.google.client_id.clone(),
            cfg.google.client_secret.clone(),
            cfg.google.redirect_uri.clone(),
            cache.clone(),
        ));

        let microsoft_auth = cfg.microsoft.as_ref().map(|ms| {
            tracing::info!("Found microsoft Client ID {}", ms.client_id);
            Arc::new(O365Authenticator::new(
                ms.client_id.clone(),
                ms.client_secret.clone(),
                ms.tenant_id.clone(),
                ms.redirect_uri.clone(),
                cache.clone(),
            ))
        });

        let routing: Arc<dyn RoutingProvider> =
            Arc::new(Graphhopper::new(cache.clone(), reqwest::Client::new()));

        let open_meteo = Arc::new(OpenMeteoClient::new(cache.clone()));
        let weather: Arc<dyn WeatherProvider> = open_meteo.clone();
        let geo: Arc<dyn GeoProvider> = open_meteo;

        let preference_scorer = Arc::new(PreferenceScorer::new());

        let paragliding_source: Arc<dyn ActivitySource> = Arc::new(ParaglidingActivitySource::new(
            site_repo.clone(),
            settings_repo.clone(),
            weather.clone(),
            preference_scorer.clone(),
        ));
        #[cfg(feature = "new-activities")]
        let tour_source: Arc<dyn ActivitySource> = Arc::new(TourActivitySource::new(
            outdoor_repo.clone(),
            settings_repo.clone(),
            weather.clone(),
            preference_scorer.clone(),
        ));
        #[cfg(feature = "new-activities")]
        let event_source: Arc<dyn ActivitySource> = Arc::new(EventActivitySource::new(
            event_repo.clone(),
            settings_repo.clone(),
            preference_scorer.clone(),
        ));
        let solver: Arc<dyn WeekSolver> = Arc::new(Nsga2Solver::new());

        #[cfg_attr(not(feature = "new-activities"), allow(unused_mut))]
        let mut sources: Vec<Arc<dyn ActivitySource>> = vec![paragliding_source];
        #[cfg(feature = "new-activities")]
        {
            sources.push(tour_source);
            sources.push(event_source);
        }
        let planner = Arc::new(Planner::new(sources, solver.clone(), geo.clone()));

        let preferences = Arc::new(PreferenceService::new(
            outdoor_repo.clone(),
            event_repo.clone(),
            site_repo.clone(),
            preference_repo.clone(),
            embedding_repo.clone(),
            image_repo.clone(),
            image_store.clone(),
            preference_scorer.clone(),
        ));

        let google_cal = GoogleCalendar::new(auth.clone(), cache.clone())?;
        let microsoft_cal = microsoft_auth
            .as_ref()
            .map(|a| MicrosoftCalendar::new(a.clone(), cache.clone()));
        let calendar: Arc<dyn CalendarProvider> =
            Arc::new(CombinedCalendar::new(google_cal, microsoft_cal));

        Ok(Self {
            site_repo,
            settings_repo,
            outdoor_repo,
            event_repo,
            embedding_repo,
            preference_repo,
            image_repo,
            image_store,
            image_variant: cfg.image_variant.clone(),
            image_download_concurrency: cfg.image_download_concurrency,
            preference_scorer,
            outdoor_feed,
            embedding_cache_dir: cfg.embedding_cache_dir.clone(),
            embedding_batch_size: cfg.embedding_batch_size,
            embedding_running: Arc::new(AtomicBool::new(false)),
            auth,
            microsoft_auth,
            routing,
            weather,
            geo,
            calendar,
            planner,
            preferences,
        })
    }
}
