use std::sync::Arc;

use anyhow::Result;
use sqlx::PgPool;

use crate::{
    adapters::{
        activities::events::source::EventActivitySource,
        activities::paragliding::source::ParaglidingActivitySource,
        activities::tours::source::TourActivitySource,
        cache::PersistentCache,
        combined_calendar::CombinedCalendar,
        crow_flies::CrowFlies,
        google_calendar::{GoogleCalendar, WebFlowAuthenticator},
        microsoft_calendar::{MicrosoftCalendar, O365Authenticator},
        open_meteo::OpenMeteoClient,
        postgres::PostgresRepository,
    },
    application::{Planner, solvers::Nsga2Solver},
    config::AppConfig,
    domain::ports::{
        ActivitySource, CalendarProvider, EventRepository, GeoProvider, OutdoorTourRepository,
        RoutingProvider, SettingsRepository, SiteRepository, WeatherProvider, WeekSolver,
    },
};

#[derive(Clone)]
pub struct AppState {
    pub site_repo: Arc<dyn SiteRepository>,
    pub settings_repo: Arc<dyn SettingsRepository>,
    pub outdoor_repo: Arc<dyn OutdoorTourRepository>,
    pub event_repo: Arc<dyn EventRepository>,
    pub auth: Arc<WebFlowAuthenticator>,
    pub microsoft_auth: Option<Arc<O365Authenticator>>,
    pub routing: Arc<dyn RoutingProvider>,
    pub weather: Arc<dyn WeatherProvider>,
    pub geo: Arc<dyn GeoProvider>,
    pub calendar: Arc<dyn CalendarProvider>,
    pub planner: Arc<Planner>,
}

impl AppState {
    pub fn new(pool: &PgPool, cfg: &AppConfig) -> Result<Self> {
        let cache = Arc::new(PersistentCache::new(pool.clone()));

        let repo = Arc::new(PostgresRepository::new(pool.clone()));
        let site_repo: Arc<dyn SiteRepository> = repo.clone();
        let settings_repo: Arc<dyn SettingsRepository> = repo.clone();
        let outdoor_repo: Arc<dyn OutdoorTourRepository> = repo.clone();
        let event_repo: Arc<dyn EventRepository> = repo;

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

        let paragliding_source: Arc<dyn ActivitySource> = Arc::new(
            ParaglidingActivitySource::new(site_repo.clone(), settings_repo.clone(), weather.clone()),
        );
        let tour_source: Arc<dyn ActivitySource> = Arc::new(TourActivitySource::new(
            outdoor_repo.clone(),
            settings_repo.clone(),
            weather.clone(),
        ));
        let event_source: Arc<dyn ActivitySource> =
            Arc::new(EventActivitySource::new(event_repo.clone(), settings_repo.clone()));
        let solver: Arc<dyn WeekSolver> = Arc::new(Nsga2Solver::new(routing.clone()));
        let planner = Arc::new(Planner::new(
            vec![paragliding_source, tour_source, event_source],
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
            settings_repo,
            outdoor_repo,
            event_repo,
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
