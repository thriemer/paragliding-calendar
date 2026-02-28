use std::{env, fs::File, io::BufReader, sync::Arc};

use axum::{Router, extract::Query, routing::get};
#[cfg(feature = "tls")]
use axum_server::tls_rustls::RustlsConfig;
use std::collections::HashMap;
use std::sync::LazyLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use crate::api;
use crate::auth::get_redirect_uri;

static PORT: LazyLock<u16> = LazyLock::new(|| {
    env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080)
});

static CERT_PATH: LazyLock<Option<String>> = LazyLock::new(|| {
    env::var("TLS_CERT_PATH").ok()
});

static KEY_PATH: LazyLock<Option<String>> = LazyLock::new(|| {
    env::var("TLS_KEY_PATH").ok()
});

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

pub async fn run(_port: u16) {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/oauth/callback", get(oauth_callback))
        .nest("/api", api::router())
        .fallback_service(ServeDir::new("frontend/dist"))
        .layer(cors);

    let addr = format!("0.0.0.0:{}", *PORT);

    #[cfg(feature = "tls")]
    {
        if let (Some(cert_path), Some(key_path)) = (CERT_PATH.as_ref(), KEY_PATH.as_ref()) {
            if std::path::Path::new(cert_path).exists() && std::path::Path::new(key_path).exists() {
                tracing::info!("Starting HTTPS server on port {}", *PORT);

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
    }

    tracing::info!("Starting HTTP server on port {}", *PORT);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("Web server running at http://localhost:{}", *PORT);
    axum::serve(listener, app).await.unwrap();
}
