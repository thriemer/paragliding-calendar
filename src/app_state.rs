use std::{env, sync::Arc};

use anyhow::Result;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};

use crate::{
    application::ParaglidingCalendarService,
    cache::PersistentCache,
    calendar::web_flow_authenticator::WebFlowAuthenticator,
    paragliding::{
        decision_graph::DecisionGraphRepository, repository::ParaglidingSiteRepository,
    },
    routing::Routing,
    store::PersistentStore,
    weather::open_meteo::OpenMeteoClient,
};

#[derive(Clone)]
pub struct AppState {
    pub cache: Arc<PersistentCache>,
    pub store: Arc<PersistentStore>,
    pub http: ClientWithMiddleware,
    pub site_repo: Arc<ParaglidingSiteRepository>,
    pub decision_graph: Arc<DecisionGraphRepository>,
    pub auth: Arc<WebFlowAuthenticator>,
    pub routing: Arc<Routing>,
    pub weather: Arc<OpenMeteoClient>,
    pub service: Arc<ParaglidingCalendarService>,
}

impl AppState {
    pub fn new(db: &fjall::Database) -> Result<Self> {
        let cache_ks = db.keyspace("cache", fjall::KeyspaceCreateOptions::default)?;
        let cache = Arc::new(PersistentCache::from_keyspace(cache_ks));

        let store_ks = db.keyspace("store", fjall::KeyspaceCreateOptions::default)?;
        let store = Arc::new(PersistentStore::from_keyspace(store_ks));

        let http = build_http_client();

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

        let routing = Arc::new(Routing::new(cache.clone(), http.clone()));
        let weather = Arc::new(OpenMeteoClient::new(cache.clone()));
        let service = Arc::new(ParaglidingCalendarService::new(
            routing.clone(),
            weather.clone(),
        ));
        let site_repo = Arc::new(ParaglidingSiteRepository::new(store.clone()));
        let decision_graph = Arc::new(DecisionGraphRepository::new(store.clone()));

        Ok(Self {
            cache,
            store,
            http,
            site_repo,
            decision_graph,
            auth,
            routing,
            weather,
            service,
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
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .build()
}
