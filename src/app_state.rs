use std::{env, sync::Arc};

use anyhow::Result;

use crate::{
    application::ParaglidingCalendarService,
    cache::PersistentCache,
    calendar::web_flow_authenticator::WebFlowAuthenticator,
    paragliding::repository::ParaglidingSiteRepository,
    routing::Routing,
    weather::open_meteo::OpenMeteoClient,
};

#[derive(Clone)]
pub struct AppState {
    pub cache: Arc<PersistentCache>,
    pub site_repo: Arc<ParaglidingSiteRepository>,
    pub auth: Arc<WebFlowAuthenticator>,
    pub routing: Arc<Routing>,
    pub weather: Arc<OpenMeteoClient>,
    pub service: Arc<ParaglidingCalendarService>,
}

impl AppState {
    pub fn new(db: &fjall::Database) -> Result<Self> {
        let cache_ks = db.keyspace("cache", fjall::KeyspaceCreateOptions::default)?;
        let cache = Arc::new(PersistentCache::from_keyspace(cache_ks));

        let client_id = env::var("GOOGLE_CLIENT_ID").expect("Missing GOOGLE_CLIENT_ID");
        let client_secret = env::var("GOOGLE_CLIENT_SECRET").expect("Missing GOOGLE_CLIENT_SECRET");
        let redirect_uri = env::var("OAUTH_REDIRECT_URL").unwrap_or_else(|_| {
            "https://linus-x1.bangus-firefighter.ts.net/oauth/callback".to_string()
        });
        let auth = Arc::new(WebFlowAuthenticator::new(
            client_id,
            client_secret,
            redirect_uri,
            cache.clone(),
        ));

        let routing = Arc::new(Routing::new(cache.clone()));
        let weather = Arc::new(OpenMeteoClient::new(cache.clone()));
        let service = Arc::new(ParaglidingCalendarService::new(
            routing.clone(),
            weather.clone(),
        ));
        let site_repo = Arc::new(ParaglidingSiteRepository::new());

        Ok(Self {
            cache,
            site_repo,
            auth,
            routing,
            weather,
            service,
        })
    }
}
