use axum::{
    Router,
    body::Body,
    extract::Extension,
    extract::{Path, Query},
    http::StatusCode,
    response::Json,
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower_http::limit::RequestBodyLimitLayer;

use crate::{
    database::{self, Db},
    email::{EmailProvider, GmailEmailProvider},
    location::{Location, LocationProvider, open_meteo::OpenMeteoLocationProvider},
    paragliding::{
        ParaglidingSite, ParaglidingSiteProvider,
        database::{CachedParaglidingSiteProvider, UserSettings},
        dhv,
    },
    weather::{self, WeatherModel},
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

#[derive(Deserialize)]
pub struct GeocodeQuery {
    name: String,
}

#[derive(Serialize)]
pub struct GeocodeResponse {
    results: Vec<Location>,
}

async fn get_elevation(
    Extension(state): Extension<ApiState>,
    Query(query): Query<ElevationQuery>,
) -> Result<Json<ElevationResponse>, StatusCode> {
    let elevation = state
        .location_provider
        .fetch_elevation(query.latitude, query.longitude)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ElevationResponse { elevation }))
}

async fn geocode(
    Extension(state): Extension<ApiState>,
    Query(query): Query<GeocodeQuery>,
) -> Result<Json<GeocodeResponse>, StatusCode> {
    let locations = state
        .location_provider
        .geocode(query.name)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(GeocodeResponse { results: locations }))
}

async fn get_settings(
    Extension(state): Extension<ApiState>,
) -> Result<Json<UserSettings>, StatusCode> {
    let provider = CachedParaglidingSiteProvider::new(state.db);
    let settings = provider
        .get_settings()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match settings {
        Some(s) => Ok(Json(s)),
        None => Ok(Json(UserSettings::default())),
    }
}

async fn save_settings(
    Extension(state): Extension<ApiState>,
    Json(settings): Json<UserSettings>,
) -> Result<StatusCode, StatusCode> {
    let provider = CachedParaglidingSiteProvider::new(state.db);
    provider
        .save_settings(&settings)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

#[derive(Clone)]
pub struct ApiState {
    pub db: Db,
    pub location_provider: OpenMeteoLocationProvider,
    pub email_provider: GmailEmailProvider,
}

pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/sites", get(get_sites))
        .route("/sites", put(update_site))
        .route("/sites/{site_name}", delete(delete_site))
        .route(
            "/sites/import",
            post(import_sites).layer(RequestBodyLimitLayer::new(50 * 1024 * 1024)),
        )
        .route("/elevation", get(get_elevation))
        .route("/geocode", get(geocode))
        .route("/settings", get(get_settings))
        .route("/settings", put(save_settings))
        .route("/decision-graph", get(get_decision_graph))
        .route("/decision-graph", post(save_decision_graph))
        .route("/weather-models", get(get_weather_models))
        .layer(Extension(state))
}

async fn get_sites(
    Extension(state): Extension<ApiState>,
) -> Result<Json<Vec<ParaglidingSite>>, StatusCode> {
    let provider = CachedParaglidingSiteProvider::new(state.db);
    let sites = provider.fetch_all_sites().await;
    Ok(Json(sites))
}

async fn update_site(
    Extension(state): Extension<ApiState>,
    Json(site): Json<ParaglidingSite>,
) -> Result<StatusCode, StatusCode> {
    let provider = CachedParaglidingSiteProvider::new(state.db);
    provider
        .save_site(site)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

async fn delete_site(
    Extension(state): Extension<ApiState>,
    Path(site_name): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let provider = CachedParaglidingSiteProvider::new(state.db);
    provider
        .delete_site(&site_name)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

#[derive(Serialize, Deserialize)]
pub struct ImportResponse {
    pub imported: usize,
}

async fn import_sites(
    Extension(state): Extension<ApiState>,
    body: Body,
) -> Result<Json<ImportResponse>, StatusCode> {
    tracing::info!("Starting DHV file import");

    let bytes = axum::body::to_bytes(body, 50 * 1024 * 1024)
        .await
        .map_err(|e| {
            tracing::error!("Failed to read request body: {:?}", e);
            StatusCode::BAD_REQUEST
        })?;

    tracing::info!("Read {} bytes from request", bytes.len());

    let xml_content = String::from_utf8(bytes.to_vec()).map_err(|e| {
        tracing::error!("Request body is not valid UTF-8: {:?}", e);
        StatusCode::BAD_REQUEST
    })?;

    let mut imported_count = 0;

    match dhv::parse_sites_from_xml(&xml_content) {
        Ok(sites) => {
            tracing::info!("Parsed {} sites from XML", sites.len());
            let provider = CachedParaglidingSiteProvider::new(state.db);
            for site in sites {
                if let Err(e) = provider.save_site(site).await {
                    tracing::warn!("Failed to save site: {}", e);
                } else {
                    imported_count += 1;
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to parse XML: {:?}", e);
        }
    }

    tracing::info!("Import complete: {} sites imported.", imported_count);
    Ok(Json(ImportResponse {
        imported: imported_count,
    }))
}

async fn get_decision_graph(
    Extension(state): Extension<ApiState>,
) -> Result<Json<Value>, StatusCode> {
    let cached: Option<String> = database::get(&state.db, CACHE_KEY)
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

async fn save_decision_graph(
    Extension(state): Extension<ApiState>,
    Json(payload): Json<Value>,
) -> Result<StatusCode, StatusCode> {
    let graph = serde_json::to_string(&payload).map_err(|_| StatusCode::BAD_REQUEST)?;

    database::save(&state.db, CACHE_KEY, graph)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(StatusCode::OK)
}

#[derive(Serialize)]
struct WeatherModelsResponse {
    models: Vec<WeatherModel>,
}

async fn get_weather_models() -> Json<WeatherModelsResponse> {
    Json(WeatherModelsResponse {
        models: weather::get_available_weather_models(),
    })
}
