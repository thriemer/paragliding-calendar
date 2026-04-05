use axum::{
    Router,
    body::Body,
    extract::{Path, Query},
    http::StatusCode,
    response::Json,
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tower_http::limit::RequestBodyLimitLayer;

use crate::{
    cache,
    calendar::{CalendarProvider, google::GoogleCalendar},
    location::Location,
    paragliding::{
        ParaglidingSite, ParaglidingSiteProvider,
        cache::{CachedParaglidingSiteProvider, UserSettings},
        dhv,
    },
    weather::{self, WeatherModel, open_meteo},
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

#[derive(Serialize)]
struct UserSettingsResponse {
    pub location_name: String,
    pub location_latitude: f64,
    pub location_longitude: f64,
    pub search_radius_km: f64,
    pub calendar_name: String,
    pub minimum_flyable_hours: u32,
    pub excluded_calendar_names: Vec<String>,
    pub all_calendar_names: Vec<String>,
}

impl From<UserSettings> for UserSettingsResponse {
    fn from(value: UserSettings) -> Self {
        UserSettingsResponse {
            location_name: value.location_name,
            location_latitude: value.location_latitude,
            location_longitude: value.location_longitude,
            search_radius_km: value.search_radius_km,
            calendar_name: value.calendar_name,
            minimum_flyable_hours: value.minimum_flyable_hours,
            excluded_calendar_names: value.excluded_calendar_names,
            all_calendar_names: vec![],
        }
    }
}

async fn get_elevation(
    Query(query): Query<ElevationQuery>,
) -> Result<Json<ElevationResponse>, StatusCode> {
    let elevation = open_meteo::fetch_elevation(query.latitude, query.longitude)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ElevationResponse { elevation }))
}

async fn geocode(Query(query): Query<GeocodeQuery>) -> Result<Json<GeocodeResponse>, StatusCode> {
    let locations = open_meteo::geocode(&query.name)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(GeocodeResponse { results: locations }))
}

async fn get_settings() -> Result<Json<UserSettingsResponse>, StatusCode> {
    //TODO: replace with generic calendar
    let cal = GoogleCalendar::new()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let calendars = cal
        .get_calendar_names()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut settings: UserSettingsResponse = match CachedParaglidingSiteProvider::get_settings()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        Some(s) => s.into(),
        None => UserSettings::default().into(),
    };
    settings.all_calendar_names = calendars;
    Ok(Json(settings))
}

async fn save_settings(Json(settings): Json<UserSettings>) -> Result<StatusCode, StatusCode> {
    CachedParaglidingSiteProvider::save_settings(&settings)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

pub fn router() -> Router {
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
}

async fn get_sites() -> Result<Json<Vec<ParaglidingSite>>, StatusCode> {
    let provider = CachedParaglidingSiteProvider::new();
    let sites = provider.fetch_all_sites().await;
    Ok(Json(sites))
}

async fn update_site(Json(site): Json<ParaglidingSite>) -> Result<StatusCode, StatusCode> {
    let provider = CachedParaglidingSiteProvider::new();
    provider
        .save_site(site)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

async fn delete_site(Path(site_name): Path<String>) -> Result<StatusCode, StatusCode> {
    let provider = CachedParaglidingSiteProvider::new();
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

async fn import_sites(body: Body) -> Result<Json<ImportResponse>, StatusCode> {
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
            let provider = CachedParaglidingSiteProvider::new();
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

#[derive(Serialize)]
struct WeatherModelsResponse {
    models: Vec<WeatherModel>,
}

async fn get_weather_models() -> Json<WeatherModelsResponse> {
    Json(WeatherModelsResponse {
        models: weather::get_available_weather_models(),
    })
}
