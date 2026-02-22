use axum::{
    Router,
    http::StatusCode,
    response::Json,
    routing::{get, post},
};
use serde_json::Value;

use crate::cache;

const CACHE_KEY: &str = "decision_graph";

pub fn router() -> Router {
    Router::new()
        .route("/decision-graph", get(get_decision_graph))
        .route("/decision-graph", post(save_decision_graph))
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
