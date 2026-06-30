use std::sync::Arc;

use anyhow::Result;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use reqwest_tracing::TracingMiddleware;

use crate::{
    adapters::{
        activities::paragliding::{
            repository::ParaglidingSiteRepository, source::ParaglidingActivitySource,
        },
        cache::PersistentCache,
        combined_calendar::CombinedCalendar,
        google_calendar::{GoogleCalendar, WebFlowAuthenticator},
        graphhopper::Routing,
        microsoft_calendar::{MicrosoftCalendar, O365Authenticator},
        open_meteo::OpenMeteoClient,
        store::PersistentStore,
    },
    application::{Planner, solvers::GreedyDiversitySolver},
    config::AppConfig,
    domain::ports::{
        ActivitySource, CalendarProvider, GeoProvider, RoutingProvider, WeatherProvider, WeekSolver,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub cache: Arc<PersistentCache>,
    pub store: Arc<PersistentStore>,
    pub http: ClientWithMiddleware,
    pub site_repo: Arc<ParaglidingSiteRepository>,
    pub auth: Arc<WebFlowAuthenticator>,
    pub microsoft_auth: Option<Arc<O365Authenticator>>,
    pub routing: Arc<dyn RoutingProvider>,
    pub weather: Arc<dyn WeatherProvider>,
    pub geo: Arc<dyn GeoProvider>,
    pub calendar: Arc<dyn CalendarProvider>,
    pub solver: Arc<dyn WeekSolver>,
    pub planner: Arc<Planner>,
}

impl AppState {
    pub fn new(db: &fjall::Database, cfg: &AppConfig) -> Result<Self> {
        let cache_ks = db.keyspace("cache", fjall::KeyspaceCreateOptions::default)?;
        let cache = Arc::new(PersistentCache::from_keyspace(cache_ks));

        let store_ks = db.keyspace("store", fjall::KeyspaceCreateOptions::default)?;
        let store = Arc::new(PersistentStore::from_keyspace(store_ks));

        let http = build_http_client();

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

        let routing: Arc<dyn RoutingProvider> = Arc::new(Routing::new(cache.clone(), http.clone()));

        let open_meteo = Arc::new(OpenMeteoClient::new(cache.clone()));
        let weather: Arc<dyn WeatherProvider> = open_meteo.clone();
        let geo: Arc<dyn GeoProvider> = open_meteo;

        let site_repo = Arc::new(ParaglidingSiteRepository::new(store.clone()));

        let paragliding_source: Arc<dyn ActivitySource> = Arc::new(ParaglidingActivitySource::new(
            site_repo.clone(),
            weather.clone(),
        ));
        let solver: Arc<dyn WeekSolver> = Arc::new(GreedyDiversitySolver::new(routing.clone()));
        let planner = Arc::new(Planner::new(
            vec![paragliding_source],
            solver.clone(),
        ));

        let google_cal = GoogleCalendar::new(auth.clone(), cache.clone())?;
        let microsoft_cal = microsoft_auth
            .as_ref()
            .map(|a| MicrosoftCalendar::new(a.clone(), cache.clone()));
        let calendar: Arc<dyn CalendarProvider> =
            Arc::new(CombinedCalendar::new(google_cal, microsoft_cal));

        Ok(Self {
            cache,
            store,
            http,
            site_repo,
            auth,
            microsoft_auth,
            routing,
            weather,
            geo,
            calendar,
            solver,
            planner,
        })
    }
}

fn build_http_client() -> ClientWithMiddleware {
    let retry_policy = ExponentialBackoff::builder()
        .base(3)
        .retry_bounds(
            std::time::Duration::from_secs(10),
            std::time::Duration::from_mins(30),
        )
        .build_with_max_retries(5);
    ClientBuilder::new(reqwest::Client::new())
        .with(TracingMiddleware::default())
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .build()
}
