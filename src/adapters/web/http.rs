use axum::{
    Router,
    body::Body,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post, put},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::Ordering;
use tower_http::limit::RequestBodyLimitLayer;
use tracing::instrument;

use std::collections::BTreeMap;

use crate::{
    adapters::{embedding::clip::ClipEmbedder, ingest::dhv},
    app_state::AppState,
    application::{
        calendar_job, flight_analytics,
        preferences::{CandidatePair, ComparisonMatrix, VoteOutcome},
    },
    domain::{
        activities::kind_from_category,
        location::Location,
        paragliding::{self, ParaglidingSite, flight::Track},
        preference_fit::ValidationMetrics,
        preferences::{PreferenceCandidate, display_score},
        settings::UserSettings,
        weather::WeatherModel,
    },
};

#[derive(Serialize)]
struct ActivityMapItem {
    id: String,
    kind: String,
    title: String,
    latitude: f64,
    longitude: f64,
    description: String,
    image_urls: Vec<String>,
    launches: Vec<paragliding::ParaglidingLaunch>,
    landings: Vec<paragliding::ParaglidingLanding>,
    country: Option<String>,
    data_source: String,
    parking_location: Option<Location>,
    mute_alerts: Option<bool>,
    rating: Option<u8>,
    preferred_weather_model: Option<String>,
}

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
    let calendars = state
        .calendar
        .get_calendar_names()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut settings: UserSettingsResponse = match state
        .settings_repo
        .get()
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
        .settings_repo
        .save(&settings)
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
        .route("/calendar/refresh", post(trigger_calendar_job))
        .route("/preferences", get(get_preferences_summary))
        .route("/preferences/compare", get(get_preferences_compare))
        .route("/preferences/vote", post(post_preferences_vote))
        .route("/preferences/like-both", post(post_preferences_like_both))
        .route("/preferences/dislike-both", post(post_preferences_dislike_both))
        .route("/preferences/rate", post(post_preferences_rate))
        .route("/preferences/matrix", get(get_preferences_matrix))
        .route("/preferences/re-embed", post(post_preferences_reembed))
        .route("/images/{hash}", get(get_image))
}

/// Serve a downloaded activity image by its content hash. Content-addressed, so
/// the bytes never change — cache aggressively. The `ImageStore` validates the
/// hash (rejecting path traversal). The MIME type is sniffed from the bytes.
#[instrument(skip(state))]
async fn get_image(State(state): State<AppState>, Path(hash): Path<String>) -> Response {
    match state.image_store.get(&hash).await {
        Ok(bytes) => {
            let mime = image::guess_format(&bytes)
                .map(|f| f.to_mime_type())
                .unwrap_or("application/octet-stream");
            (
                [
                    (header::CONTENT_TYPE, mime),
                    (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                ],
                bytes,
            )
                .into_response()
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[instrument(skip(state))]
async fn trigger_calendar_job(State(state): State<AppState>) -> StatusCode {
    tokio::spawn(async move {
        if let Err(e) = calendar_job::run(&state).await {
            tracing::error!(error = ?e, "Manual calendar job trigger failed");
        }
    });
    StatusCode::ACCEPTED
}

#[instrument(skip(state))]
async fn get_sites(
    State(state): State<AppState>,
) -> Result<Json<Vec<ActivityMapItem>>, StatusCode> {
    let sites = state
        .site_repo
        .find_all()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let tours = state
        .outdoor_repo
        .find_all()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut items: Vec<ActivityMapItem> = Vec::with_capacity(sites.len() + tours.len());

    for site in sites {
        let (lat, lon) = site
            .launches
            .first()
            .map(|l| (l.location.latitude, l.location.longitude))
            .unwrap_or((0.0, 0.0));

        items.push(ActivityMapItem {
            id: site.name.clone(),
            kind: "paragliding".to_string(),
            title: site.name.clone(),
            latitude: lat,
            longitude: lon,
            description: String::new(),
            image_urls: vec![],
            launches: site.launches,
            landings: site.landings,
            country: site.country,
            data_source: site.data_source,
            parking_location: site.parking_location,
            mute_alerts: site.mute_alerts,
            rating: site.rating,
            preferred_weather_model: site.preferred_weather_model,
        });
    }

    for tour in tours {
        let Some(kind) = kind_from_category(&tour.category) else {
            continue;
        };

        items.push(ActivityMapItem {
            id: tour.id,
            kind: kind.as_str().to_string(),
            title: tour.title,
            latitude: tour.location.latitude,
            longitude: tour.location.longitude,
            description: tour.description,
            image_urls: tour.image_urls,
            launches: vec![],
            landings: vec![],
            country: None,
            data_source: String::new(),
            parking_location: None,
            mute_alerts: None,
            rating: None,
            preferred_weather_model: None,
        });
    }

    Ok(Json(items))
}

#[instrument(skip(state, site), fields(site = %site.name))]
async fn update_site(
    State(state): State<AppState>,
    Json(site): Json<ParaglidingSite>,
) -> Result<StatusCode, StatusCode> {
    state
        .site_repo
        .save(site)
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
        .delete(&site_name)
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
                if let Err(e) = state.site_repo.save(site).await {
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

    let analysis = flight_analytics::analyse_flight(&track).map_err(|e| {
        tracing::warn!(error = ?e, "Flight analysis failed");
        StatusCode::BAD_REQUEST
    })?;
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

// ---- Preference learning (PLAN.md Phase 2b) --------------------------------

#[derive(Serialize)]
struct ActivityCardDto {
    id: String,
    kind: String,
    title: String,
    description: String,
    stats: BTreeMap<String, String>,
    /// Content hashes of the activity's images; the UI resolves each to
    /// `/api/images/{hash}` (position order, 0 = primary).
    image_hashes: Vec<String>,
    current_score: f64,
}

impl From<&PreferenceCandidate> for ActivityCardDto {
    fn from(c: &PreferenceCandidate) -> Self {
        ActivityCardDto {
            id: c.id.clone(),
            kind: c.kind.as_str().to_string(),
            title: c.title.clone(),
            description: c.description.clone(),
            stats: c.stats.iter().cloned().collect(),
            image_hashes: c.image_hashes.clone(),
            // Raw score is log-odds; the UI shows the 0–100 display mapping.
            current_score: display_score(c.score),
        }
    }
}

#[derive(Serialize)]
struct PairDto {
    pair_id: String,
    a: ActivityCardDto,
    b: ActivityCardDto,
}

impl From<&CandidatePair> for PairDto {
    fn from((a, b): &CandidatePair) -> Self {
        PairDto {
            pair_id: encode_pair(&a.id, &b.id),
            a: a.as_ref().into(),
            b: b.as_ref().into(),
        }
    }
}

/// `pair_id` is a stateless JSON encoding of the two activity ids — restart-safe
/// and free of server-side session state (resolves PLAN.md's PairID open
/// decision toward "encode the pair" rather than a stored UUID).
fn encode_pair(a: &str, b: &str) -> String {
    serde_json::to_string(&(a, b)).unwrap_or_default()
}

fn decode_pair(s: &str) -> Option<(String, String)> {
    serde_json::from_str(s).ok()
}

#[instrument(skip(state))]
async fn get_preferences_compare(
    State(state): State<AppState>,
) -> Result<Json<PairDto>, StatusCode> {
    match state.preferences.compare().await {
        Ok(Some(pair)) => Ok(Json((&pair).into())),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::error!(error = ?e, "preferences compare failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Deserialize)]
struct VoteRequest {
    pair_id: String,
    winner_id: String,
}

#[derive(Serialize)]
struct ProgressDto {
    comparisons_done: i64,
}

#[derive(Serialize)]
struct VoteResponseDto {
    next: Option<PairDto>,
    progress: ProgressDto,
}

#[instrument(skip(state, req), fields(winner = %req.winner_id))]
async fn post_preferences_vote(
    State(state): State<AppState>,
    Json(req): Json<VoteRequest>,
) -> Result<Json<VoteResponseDto>, StatusCode> {
    // The loser is whichever half of the encoded pair isn't the winner.
    let (a, b) = decode_pair(&req.pair_id).ok_or(StatusCode::BAD_REQUEST)?;
    let loser = if req.winner_id == a {
        b
    } else if req.winner_id == b {
        a
    } else {
        return Err(StatusCode::BAD_REQUEST);
    };

    let VoteOutcome {
        next,
        comparisons_done,
    } = state
        .preferences
        .vote(&req.winner_id, &loser)
        .await
        .map_err(|e| {
            tracing::error!(error = ?e, "preferences vote failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(VoteResponseDto {
        next: next.as_ref().map(PairDto::from),
        progress: ProgressDto { comparisons_done },
    }))
}

#[derive(Deserialize)]
struct RateRequest {
    activity_id: String,
    rating: i16,
}

#[derive(Serialize)]
struct RateResponse {
    ok: bool,
}

#[derive(Deserialize)]
struct BothRequest {
    pair_id: String,
}

#[instrument(skip(state, req))]
async fn post_preferences_like_both(
    State(state): State<AppState>,
    Json(req): Json<BothRequest>,
) -> Result<Json<VoteResponseDto>, StatusCode> {
    let (a, b) = decode_pair(&req.pair_id).ok_or(StatusCode::BAD_REQUEST)?;
    let VoteOutcome {
        next,
        comparisons_done,
    } = state
        .preferences
        .like_both(&a, &b)
        .await
        .map_err(|e| {
            tracing::error!(error = ?e, "preferences like_both failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(VoteResponseDto {
        next: next.as_ref().map(PairDto::from),
        progress: ProgressDto { comparisons_done },
    }))
}

#[instrument(skip(state, req))]
async fn post_preferences_dislike_both(
    State(state): State<AppState>,
    Json(req): Json<BothRequest>,
) -> Result<Json<VoteResponseDto>, StatusCode> {
    let (a, b) = decode_pair(&req.pair_id).ok_or(StatusCode::BAD_REQUEST)?;
    let VoteOutcome {
        next,
        comparisons_done,
    } = state
        .preferences
        .dislike_both(&a, &b)
        .await
        .map_err(|e| {
            tracing::error!(error = ?e, "preferences dislike_both failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(VoteResponseDto {
        next: next.as_ref().map(PairDto::from),
        progress: ProgressDto { comparisons_done },
    }))
}

#[instrument(skip(state, req), fields(activity = %req.activity_id, rating = req.rating))]
async fn post_preferences_rate(
    State(state): State<AppState>,
    Json(req): Json<RateRequest>,
) -> Result<Json<RateResponse>, StatusCode> {
    if !(1..=5).contains(&req.rating) {
        return Err(StatusCode::BAD_REQUEST);
    }
    state
        .preferences
        .rate(&req.activity_id, req.rating)
        .await
        .map_err(|e| {
            tracing::error!(error = ?e, "preferences rate failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(RateResponse { ok: true }))
}

#[derive(Serialize)]
struct FeatureSummaryDto {
    feature: String,
    weight: f64,
    direction: String,
}

#[derive(Serialize)]
struct KindSummaryDto {
    base_pref: f64,
    activity_count: usize,
    features: Vec<FeatureSummaryDto>,
}

#[derive(Serialize)]
struct ValidationDto {
    pairwise_accuracy: Option<f64>,
    pairwise_count: usize,
    rating_mse: Option<f64>,
    rating_count: usize,
    k: usize,
}

impl From<ValidationMetrics> for ValidationDto {
    fn from(m: ValidationMetrics) -> Self {
        Self {
            pairwise_accuracy: m.pairwise_accuracy,
            pairwise_count: m.pairwise_count,
            rating_mse: m.rating_mse,
            rating_count: m.rating_count,
            k: m.k,
        }
    }
}

#[derive(Serialize)]
struct SummaryDto {
    comparisons_done: i64,
    ratings_done: i64,
    kinds: BTreeMap<String, KindSummaryDto>,
    validation: ValidationDto,
}

#[instrument(skip(state))]
async fn get_preferences_summary(
    State(state): State<AppState>,
) -> Result<Json<SummaryDto>, StatusCode> {
    let summary = state.preferences.summary().await.map_err(|e| {
        tracing::error!(error = ?e, "preferences summary failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let kinds = summary
        .kinds
        .into_iter()
        .map(|k| {
            let features = k
                .features
                .into_iter()
                .map(|f| FeatureSummaryDto {
                    feature: f.feature,
                    weight: f.weight,
                    direction: if f.higher_is_better {
                        "higher is better".to_string()
                    } else {
                        "lower is better".to_string()
                    },
                })
                .collect();
            (
                k.kind.as_str().to_string(),
                KindSummaryDto {
                    base_pref: k.base_pref,
                    activity_count: k.activity_count,
                    features,
                },
            )
        })
        .collect();

    Ok(Json(SummaryDto {
        comparisons_done: summary.comparisons_done,
        ratings_done: summary.ratings_done,
        kinds,
        validation: summary.validation.into(),
    }))
}

#[instrument(skip(state))]
async fn get_preferences_matrix(
    State(state): State<AppState>,
) -> Result<Json<ComparisonMatrix>, StatusCode> {
    state.preferences.matrix().await.map(Json).map_err(|e| {
        tracing::error!(error = ?e, "preferences matrix failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

#[instrument(skip(state))]
async fn post_preferences_reembed(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    if state.embedding_running.swap(true, Ordering::AcqRel) {
        return Json(json!({ "status": "already_running" }));
    }

    let state = state.clone();
    tokio::spawn(async move {
        let embedder = match ClipEmbedder::new(&state.embedding_cache_dir, state.embedding_batch_size).await {
            Ok(e) => e,
            Err(e) => {
                tracing::error!(error = ?e, "failed to construct embedder for re-embed");
                state.embedding_running.store(false, Ordering::Release);
                return;
            }
        };
        if let Err(e) = state.preferences.re_embed(&embedder, state.embedding_batch_size).await {
            tracing::error!(error = ?e, "re-embed failed");
        }
        state.embedding_running.store(false, Ordering::Release);
    });

    Json(json!({ "status": "started" }))
}
