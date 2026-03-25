use std::sync::Arc;

use axum::{
    Router,
    extract::{Extension, Query},
    routing::get,
};
#[cfg(feature = "tls")]
use axum_server::tls_rustls::RustlsConfig;
use std::collections::HashMap;
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::ServeDir;
use tower_http::timeout::TimeoutLayer;

use crate::calendar::web_flow_authenticator::WebFlowAuthenticator;
use crate::email::GmailEmailProvider;
use crate::location::open_meteo::OpenMeteoLocationProvider;
use crate::paragliding::database::CachedParaglidingSiteProvider;
use crate::{api, config};
use crate::{api::ApiState, database::DbProvider};

async fn oauth_callback(
    Query(params): Query<HashMap<String, String>>,
    Extension(state): Extension<ApiState>,
) -> Result<String, String> {
    let code = params.get("code").ok_or("Missing code parameter")?;

    let auth = WebFlowAuthenticator::new(
        std::env::var("GOOGLE_CLIENT_ID").expect("Missing GOOGLE_CLIENT_ID"),
        std::env::var("GOOGLE_CLIENT_SECRET").expect("Missing GOOGLE_CLIENT_SECRET"),
        std::env::var("OAUTH_REDIRECT_URL").unwrap_or_else(|_| {
            "https://linus-x1.bangus-firefighter.ts.net/oauth/callback".to_string()
        }),
        state.db.clone(),
        state.email_provider.clone(),
    );
    match auth.exchange_code(code).await {
        Ok(_token) => {
            tracing::info!("Successfully exchanged code for token and stored in database");
            Ok("Authentication successful! You can close this window.".to_string())
        }
        Err(e) => {
            tracing::error!("Failed to exchange code: {}", e);
            Err(format!("Failed to exchange code: {}", e))
        }
    }
}

pub async fn run(db: Arc<dyn DbProvider>) {
    let config = config::WebConfig::load().unwrap();
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api_state = ApiState {
        db: db.clone(),
        location_provider: OpenMeteoLocationProvider::new(),
        email_provider: GmailEmailProvider::new().expect("Failed to create email provider"),
        site_provider: CachedParaglidingSiteProvider::new(db),
    };
    let api_router = api::router(api_state);
    let app = Router::new()
        .route("/oauth/callback", get(oauth_callback))
        .merge(api_router)
        .fallback_service(ServeDir::new("frontend/dist"))
        .layer(cors)
        .layer(TimeoutLayer::with_status_code(
            axum::http::StatusCode::REQUEST_TIMEOUT,
            std::time::Duration::from_secs(300),
        )) // 5 min timeout
        .layer(RequestBodyLimitLayer::new(50 * 1024 * 1024)); // 50MB limit

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
