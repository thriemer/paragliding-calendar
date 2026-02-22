use axum::{
    Router,
    response::{Html, IntoResponse},
    routing::get,
};
use tower_http::cors::{Any, CorsLayer};

use crate::api;

pub async fn run(port: u16) {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(editor))
        .route("/index.html", get(editor))
        .nest("/api", api::router())
        .layer(cors);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("Web server running at http://localhost:{}", port);
    axum::serve(listener, app).await.unwrap();
}
