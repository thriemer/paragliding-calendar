use axum::{
    Router,
    body::Body,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use tower_http::limit::RequestBodyLimitLayer;
use tracing::instrument;

use crate::{
    adapters::{
        activities::paragliding::dhv,
        google_calendar::GoogleCalendar,
    },
    app_state::AppState,
    application::flight_analytics,
    domain::{
        location::Location,
        paragliding::{ParaglidingSite, ParaglidingSiteProvider, UserSettings, flight::Track},
        ports::CalendarProvider,
        weather::WeatherModel,
    },
};

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

#[instrument(skip(state, query), fields(lat = query.latitude, lon = query.longitude))]
async fn get_elevation(
    State(state): State<AppState>,
    Query(query): Query<ElevationQuery>,
) -> Result<Json<ElevationResponse>, StatusCode> {
    let elevation = state
        .geo
        .fetch_elevation(query.latitude, query.longitude)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ElevationResponse { elevation }))
}

#[instrument(skip(state, query), fields(name = %query.name))]
async fn geocode(
    State(state): State<AppState>,
    Query(query): Query<GeocodeQuery>,
) -> Result<Json<GeocodeResponse>, StatusCode> {
    let locations = state
        .geo
        .geocode(&query.name)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(GeocodeResponse { results: locations }))
}

#[instrument(skip(state))]
async fn get_settings(
    State(state): State<AppState>,
) -> Result<Json<UserSettingsResponse>, StatusCode> {
    let cal = GoogleCalendar::new(state.auth.clone(), state.cache.clone())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let calendars = cal
        .get_calendar_names()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut settings: UserSettingsResponse = match state
        .site_repo
        .get_settings()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        Some(s) => s.into(),
        None => UserSettings::default().into(),
    };
    settings.all_calendar_names = calendars;
    Ok(Json(settings))
}

#[instrument(skip(state, settings))]
async fn save_settings(
    State(state): State<AppState>,
    Json(settings): Json<UserSettings>,
) -> Result<StatusCode, StatusCode> {
    state
        .site_repo
        .save_settings(&settings)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/sites", get(get_sites))
        .route("/sites", put(update_site))
        .route("/sites/{site_name}", delete(delete_site))
        .route(
            "/sites/import",
            post(import_sites).layer(RequestBodyLimitLayer::new(50 * 1024 * 1024)),
        )
        .route(
            "/flights/analyze",
            post(analyze_flight).layer(RequestBodyLimitLayer::new(50 * 1024 * 1024)),
        )
        .route("/elevation", get(get_elevation))
        .route("/geocode", get(geocode))
        .route("/settings", get(get_settings))
        .route("/settings", put(save_settings))
        .route("/weather-models", get(get_weather_models))
}

#[instrument(skip(state))]
async fn get_sites(State(state): State<AppState>) -> Result<Json<Vec<ParaglidingSite>>, StatusCode> {
    let sites = state.site_repo.fetch_all_sites().await;
    Ok(Json(sites))
}

#[instrument(skip(state, site), fields(site = %site.name))]
async fn update_site(
    State(state): State<AppState>,
    Json(site): Json<ParaglidingSite>,
) -> Result<StatusCode, StatusCode> {
    state
        .site_repo
        .save_site(site)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

#[instrument(skip(state), fields(site = %site_name))]
async fn delete_site(
    State(state): State<AppState>,
    Path(site_name): Path<String>,
) -> Result<StatusCode, StatusCode> {
    state
        .site_repo
        .delete_site(&site_name)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}

#[derive(Serialize, Deserialize)]
pub struct ImportResponse {
    pub imported: usize,
}

#[instrument(skip(state, body))]
async fn import_sites(
    State(state): State<AppState>,
    body: Body,
) -> Result<Json<ImportResponse>, StatusCode> {
    tracing::info!("Starting DHV file import");

    let bytes = axum::body::to_bytes(body, 50 * 1024 * 1024)
        .await
        .map_err(|e| {
            tracing::error!(error = ?e, "Failed to read request body");
            StatusCode::BAD_REQUEST
        })?;

    tracing::info!(bytes = bytes.len(), "Read request body");

    let xml_content = String::from_utf8(bytes.to_vec()).map_err(|e| {
        tracing::error!(error = ?e, "Request body is not valid UTF-8");
        StatusCode::BAD_REQUEST
    })?;

    let mut imported_count = 0;

    match dhv::parse_sites_from_xml(&xml_content) {
        Ok(sites) => {
            tracing::info!(parsed_sites = sites.len(), "Parsed sites from XML");
            for site in sites {
                if let Err(e) = state.site_repo.save_site(site).await {
                    tracing::warn!(error = ?e, "Failed to save site");
                } else {
                    imported_count += 1;
                }
            }
        }
        Err(e) => {
            tracing::error!(error = ?e, "Failed to parse XML");
        }
    }

    tracing::info!(imported = imported_count, "Import complete");
    Ok(Json(ImportResponse {
        imported: imported_count,
    }))
}

#[instrument(skip(body))]
async fn analyze_flight(body: Body) -> Result<Json<flight_analytics::FlightAnalysis>, StatusCode> {
    tracing::info!("Starting flight analysis");

    let bytes = axum::body::to_bytes(body, 50 * 1024 * 1024)
        .await
        .map_err(|e| {
            tracing::error!(error = ?e, "Failed to read request body");
            StatusCode::BAD_REQUEST
        })?;

    tracing::info!(bytes = bytes.len(), "Read request body");

    let kml_content = String::from_utf8(bytes.to_vec()).map_err(|e| {
        tracing::error!(error = ?e, "Request body is not valid UTF-8");
        StatusCode::BAD_REQUEST
    })?;

    let track = Track::from_kml(&kml_content).map_err(|e| {
        tracing::error!(error = ?e, "Failed to parse KML");
        StatusCode::BAD_REQUEST
    })?;

    tracing::info!(points = track.points.len(), "Parsed track");

    let analysis = flight_analytics::analyse_flight(&track);
    tracing::info!("Flight analysis complete");

    Ok(Json(analysis))
}

#[derive(Serialize)]
struct WeatherModelsResponse {
    models: Vec<WeatherModel>,
}

#[instrument(skip(state))]
async fn get_weather_models(State(state): State<AppState>) -> Json<WeatherModelsResponse> {
    Json(WeatherModelsResponse {
        models: state.weather.available_models(),
    })
}
