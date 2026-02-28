use std::{env, fs::File, io::BufReader, sync::Arc};

use axum::{Router, extract::Query, routing::get};
use axum_server::tls_rustls::RustlsConfig;
use std::collections::HashMap;
use std::sync::LazyLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use crate::api;
use crate::auth::get_redirect_uri;

static AUTHENTICATOR: LazyLock<Arc<tokio::sync::Mutex<Option<crate::auth::WebFlowAuthenticator>>>> =
    LazyLock::new(|| {
        let client_id = env::var("GOOGLE_CLIENT_ID")
            .or_else(|_| env::var("GOOGLE_CALENDAR_CLIENT_ID"))
            .expect("Missing GOOGLE_CLIENT_ID");
        let client_secret = env::var("GOOGLE_CLIENT_SECRET")
            .or_else(|_| env::var("GOOGLE_CALENDAR_CLIENT_SECRET"))
            .expect("Missing GOOGLE_CLIENT_SECRET");

        let auth =
            crate::auth::WebFlowAuthenticator::new(client_id, client_secret, get_redirect_uri());
        Arc::new(tokio::sync::Mutex::new(Some(auth)))
    });

async fn oauth_callback(Query(params): Query<HashMap<String, String>>) -> Result<String, String> {
    let code = params.get("code").ok_or("Missing code parameter")?;

    let mut auth_guard = AUTHENTICATOR.lock().await;
    if let Some(ref auth) = *auth_guard {
        match auth.exchange_code(code).await {
            Ok(_token) => {
                tracing::info!("Successfully exchanged code for token and stored in cache");
                Ok("Authentication successful! You can close this window.".to_string())
            }
            Err(e) => {
                tracing::error!("Failed to exchange code: {}", e);
                Err(format!("Failed to exchange code: {}", e))
            }
        }
    } else {
        Err("Authenticator not initialized".to_string())
    }
}

pub async fn run(port: u16) {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/oauth/callback", get(oauth_callback))
        .nest("/api", api::router())
        .fallback_service(ServeDir::new("frontend/dist"))
        .layer(cors);

    let addr = format!("0.0.0.0:{}", port);

    // Check if TLS certs exist, if so use HTTPS
    let cert_path = "certs/cert.pem";
    let key_path = "certs/key.pem";

    if std::path::Path::new(cert_path).exists() && std::path::Path::new(key_path).exists() {
        tracing::info!("Starting HTTPS server on port {}", port);

        let config = RustlsConfig::from_pem_file(cert_path, key_path)
            .await
            .expect("Failed to load TLS config");

        axum_server::bind_rustls(addr.parse().unwrap(), config)
            .serve(app.into_make_service())
            .await
            .expect("HTTPS server error");
    } else {
        tracing::info!("No TLS certs found, starting HTTP server on port {}", port);
        let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
        tracing::info!("Web server running at http://localhost:{}", port);
        axum::serve(listener, app).await.unwrap();
    }
}
