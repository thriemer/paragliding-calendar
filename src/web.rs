use axum::{Router, extract::Query, extract::State, routing::get};
#[cfg(feature = "tls")]
use axum_server::tls_rustls::RustlsConfig;
use std::collections::HashMap;
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::ServeDir;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::{adapters::http, app_state::AppState, config};

async fn oauth_callback(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<String, String> {
    let code = params.get("code").ok_or("Missing code parameter")?;

    match state.auth.exchange_code(code).await {
        Ok(_token) => {
            tracing::info!("Successfully exchanged code for token");
            Ok("Authentication successful! You can close this window.".to_string())
        }
        Err(e) => {
            tracing::error!(error = ?e, "Failed to exchange code");
            Err("Authentication failed".to_string())
        }
    }
}

pub async fn run(state: AppState) {
    let config = config::WebConfig::load().unwrap();
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/oauth/callback", get(oauth_callback))
        .nest("/api", http::router())
        .fallback_service(ServeDir::new("frontend/dist"))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            std::time::Duration::from_secs(300),
        ))
        .layer(RequestBodyLimitLayer::new(50 * 1024 * 1024))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!(addr = %addr, "Starting HTTP server");

    #[cfg(feature = "tls")]
    {
        let (cert_path, key_path) = &config.tls_config_path;
        if std::path::Path::new(cert_path).exists() && std::path::Path::new(key_path).exists() {
            let config = RustlsConfig::from_pem_file(cert_path, key_path)
                .await
                .expect("Failed to load TLS config");

            axum_server::bind_rustls(addr.parse().unwrap(), config)
                .serve(app.into_make_service())
                .await
                .expect("HTTPS server error");
            return;
        }
    }

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
