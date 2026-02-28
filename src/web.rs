use axum::{Router, extract::Query, routing::get};
#[cfg(feature = "tls")]
use axum_server::tls_rustls::RustlsConfig;
use std::collections::HashMap;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use crate::calender::google;
use crate::{api, config};

async fn oauth_callback(Query(params): Query<HashMap<String, String>>) -> Result<String, String> {
    let code = params.get("code").ok_or("Missing code parameter")?;

    match google::AUTH.exchange_code(code).await {
        Ok(_token) => {
            tracing::info!("Successfully exchanged code for token and stored in cache");
            Ok("Authentication successful! You can close this window.".to_string())
        }
        Err(e) => {
            tracing::error!("Failed to exchange code: {}", e);
            Err(format!("Failed to exchange code: {}", e))
        }
    }
}

pub async fn run() {
    let config = config::WebConfig::load().unwrap();
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/oauth/callback", get(oauth_callback))
        .nest("/api", api::router())
        .fallback_service(ServeDir::new("frontend/dist"))
        .layer(cors);

    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!("Starting HTTP server on {}", addr);

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
