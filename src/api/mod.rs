use std::sync::{Arc, LazyLock};

use axum::{
    Router,
    extract::Query,
    http::StatusCode,
    response::Json,
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    cache, calender::web_flow_authenticator::WebFlowAuthenticator, paragliding::{ParaglidingSite, ParaglidingSiteProvider, SiteType, dhv}, weather::open_meteo
};

const CACHE_KEY: &str = "decision_graph";

#[derive(Serialize, Deserialize)]
pub struct ElevationResponse {
    pub elevation: f64,
}

#[derive(Deserialize)]
pub struct ElevationQuery {
    latitude: f64,
    longitude: f64,
}

async fn get_elevation(
    Query(query): Query<ElevationQuery>,
) -> Result<Json<ElevationResponse>, StatusCode> {
    let elevation = open_meteo::fetch_elevation(query.latitude, query.longitude)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ElevationResponse { elevation }))
}

static SITE_PROVIDER: LazyLock<dhv::DhvParaglidingSiteProvider> =
    LazyLock::new(|| dhv::DhvParaglidingSiteProvider::new("dhv_sites".into()).unwrap());

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ApiLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub name: String,
    pub country: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ApiLaunch {
    pub location: ApiLocation,
    pub direction_degrees_start: f64,
    pub direction_degrees_stop: f64,
    pub elevation: f64,
    pub site_type: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ApiLanding {
    pub location: ApiLocation,
    pub elevation: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ApiSite {
    pub name: String,
    pub country: Option<String>,
    pub launches: Vec<ApiLaunch>,
    pub landings: Vec<ApiLanding>,
}

impl From<&ParaglidingSite> for ApiSite {
    fn from(site: &ParaglidingSite) -> Self {
        Self {
            name: site.name.clone(),
            country: site.country.clone(),
            launches: site
                .launches
                .iter()
                .map(|l| ApiLaunch {
                    location: ApiLocation {
                        latitude: l.location.latitude,
                        longitude: l.location.longitude,
                        name: l.location.name.clone(),
                        country: Some(l.location.country.clone()),
                    },
                    direction_degrees_start: l.direction_degrees_start,
                    direction_degrees_stop: l.direction_degrees_stop,
                    elevation: l.elevation,
                    site_type: match l.site_type {
                        SiteType::Hang => "Hang".to_string(),
                        SiteType::Winch => "Winch".to_string(),
                    },
                })
                .collect(),
            landings: site
                .landings
                .iter()
                .map(|l| ApiLanding {
                    location: ApiLocation {
                        latitude: l.location.latitude,
                        longitude: l.location.longitude,
                        name: l.location.name.clone(),
                        country: Some(l.location.country.clone()),
                    },
                    elevation: l.elevation,
                })
                .collect(),
        }
    }
}

pub fn router() -> Router {
    Router::new()
        .route("/sites", get(get_sites))
        .route("/sites", put(update_site))
        .route("/elevation", get(get_elevation))
        .route("/decision-graph", get(get_decision_graph))
        .route("/decision-graph", post(save_decision_graph))
}

async fn get_sites() -> Result<Json<Vec<ApiSite>>, StatusCode> {
    let all_sites = SITE_PROVIDER.fetch_all_sites().await;
    let mut api_sites: Vec<ApiSite> = all_sites.iter().map(ApiSite::from).collect();

    for site in api_sites.iter_mut() {
        let cache_key = format!("site_{}", site.name);
        if let Ok(Some(cached_site)) = cache::get::<ApiSite>(&cache_key).await {
            *site = cached_site;
        }
    }

    Ok(Json(api_sites))
}

async fn update_site(Json(site): Json<ApiSite>) -> Result<StatusCode, StatusCode> {
    cache::put(
        &format!("site_{}", site.name),
        site.clone(),
        std::time::Duration::from_secs(365 * 24 * 60 * 60),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

async fn get_decision_graph() -> Result<Json<Value>, StatusCode> {
    let cached: Option<String> = cache::get(CACHE_KEY)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(graph) = cached {
        let value: Value =
            serde_json::from_str(&graph).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(Json(value));
    }

    let default = include_str!("../paragliding/flyable_decision_graph.json");
    let value: Value =
        serde_json::from_str(default).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(value))
}

async fn save_decision_graph(Json(payload): Json<Value>) -> Result<StatusCode, StatusCode> {
    let graph = serde_json::to_string(&payload).map_err(|_| StatusCode::BAD_REQUEST)?;

    cache::put::<String>(
        CACHE_KEY,
        graph,
        std::time::Duration::from_secs(365 * 24 * 60 * 60),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::OK)
}
