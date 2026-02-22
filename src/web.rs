use axum::{Router, routing::get};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use crate::api;

pub async fn run(port: u16) {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .nest("/api", api::router())
        .fallback_service(ServeDir::new("frontend/dist"))
        .layer(cors);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("Web server running at http://localhost:{}", port);
    axum::serve(listener, app).await.unwrap();
}
