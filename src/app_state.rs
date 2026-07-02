use std::sync::Arc;

use anyhow::Result;

use crate::{
    adapters::{
        activities::paragliding::{
            repository::ParaglidingSiteRepository, source::ParaglidingActivitySource,
        },
        cache::PersistentCache,
        combined_calendar::CombinedCalendar,
        crow_flies::CrowFlies,
        google_calendar::{GoogleCalendar, WebFlowAuthenticator},
        microsoft_calendar::{MicrosoftCalendar, O365Authenticator},
        open_meteo::OpenMeteoClient,
        store::PersistentStore,
    },
    application::{Planner, solvers::Nsga2Solver},
    config::AppConfig,
    domain::ports::{
        ActivitySource, CalendarProvider, GeoProvider, RoutingProvider, WeatherProvider, WeekSolver,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub site_repo: Arc<ParaglidingSiteRepository>,
    pub auth: Arc<WebFlowAuthenticator>,
    pub microsoft_auth: Option<Arc<O365Authenticator>>,
    pub routing: Arc<dyn RoutingProvider>,
    pub weather: Arc<dyn WeatherProvider>,
    pub geo: Arc<dyn GeoProvider>,
    pub calendar: Arc<dyn CalendarProvider>,
    pub planner: Arc<Planner>,
}

impl AppState {
    pub fn new(db: &fjall::Database, cfg: &AppConfig) -> Result<Self> {
        let cache_ks = db.keyspace("cache", fjall::KeyspaceCreateOptions::default)?;
        let cache = Arc::new(PersistentCache::from_keyspace(cache_ks));

        let store_ks = db.keyspace("store", fjall::KeyspaceCreateOptions::default)?;
        let store = Arc::new(PersistentStore::from_keyspace(store_ks));

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

        let routing: Arc<dyn RoutingProvider> = Arc::new(CrowFlies::new());

        let open_meteo = Arc::new(OpenMeteoClient::new(cache.clone()));
        let weather: Arc<dyn WeatherProvider> = open_meteo.clone();
        let geo: Arc<dyn GeoProvider> = open_meteo;

        let site_repo = Arc::new(ParaglidingSiteRepository::new(store.clone()));

        let paragliding_source: Arc<dyn ActivitySource> = Arc::new(ParaglidingActivitySource::new(
            site_repo.clone(),
            weather.clone(),
        ));
        let solver: Arc<dyn WeekSolver> = Arc::new(Nsga2Solver::new(routing.clone()));
        let planner = Arc::new(Planner::new(
            vec![paragliding_source],
            solver.clone(),
            geo.clone(),
        ));

        let google_cal = GoogleCalendar::new(auth.clone(), cache.clone())?;
        let microsoft_cal = microsoft_auth
            .as_ref()
            .map(|a| MicrosoftCalendar::new(a.clone(), cache.clone()));
        let calendar: Arc<dyn CalendarProvider> =
            Arc::new(CombinedCalendar::new(google_cal, microsoft_cal));

        Ok(Self {
            site_repo,
            auth,
            microsoft_auth,
            routing,
            weather,
            geo,
            calendar,
            planner,
        })
    }
}
