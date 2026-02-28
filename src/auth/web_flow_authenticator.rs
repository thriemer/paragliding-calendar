use std::{env, sync::Arc, sync::Mutex, time::Duration};

use anyhow::{Context, Result};
use chrono::Utc;
use google_apis_common::GetToken;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse, TokenUrl, basic::BasicClient,
};
use reqwest::Client;
use tokio::sync::Mutex as AsyncMutex;

use crate::cache;
use crate::email;

const SCOPES: [&str; 3] = [
    "https://www.googleapis.com/auth/calendar.calendarlist.readonly",
    "https://www.googleapis.com/auth/calendar.app.created",
    "https://www.googleapis.com/auth/calendar.freebusy",
];

pub fn get_redirect_uri() -> String {
    env::var("OAUTH_REDIRECT_URL")
        .unwrap_or_else(|_| "https://linus-x1.bangus-firefighter.ts.net/oauth/callback".to_string())
}

static PKCE_VERIFIER: std::sync::LazyLock<std::sync::Mutex<Option<String>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(None));

pub struct WebFlowAuthenticator {
    client: BasicClient,
    redirect_uri: String,
    http_client: Client,
    pkce_verifier: Mutex<Option<PkceCodeVerifier>>,
    stored_token: Arc<Mutex<Option<StoredToken>>>,
    authenticated: Arc<Mutex<bool>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expiry: i64,
}

impl WebFlowAuthenticator {
    pub fn new(client_id: String, client_secret: String, redirect_uri: String) -> Self {
        let auth_url = AuthUrl::new("https://accounts.google.com/o/oauth2/auth".to_string())
            .expect("Invalid auth URL");
        let token_url = TokenUrl::new("https://oauth2.googleapis.com/token".to_string())
            .expect("Invalid token URL");

        let client = BasicClient::new(
            ClientId::new(client_id),
            Some(ClientSecret::new(client_secret)),
            auth_url,
            Some(token_url),
        )
        .set_redirect_uri(RedirectUrl::new(redirect_uri.clone()).expect("Invalid redirect URL"));

        Self {
            client,
            redirect_uri,
            http_client: Client::new(),
            pkce_verifier: Mutex::new(None),
            stored_token: Arc::new(Mutex::new(None)),
            authenticated: Arc::new(Mutex::new(false)),
        }
    }

    pub fn set_stored_token(&self, token: StoredToken) {
        *self.stored_token.lock().unwrap() = Some(token);
    }

    pub fn build_authorization_url(&self) -> (String, String) {
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        let (auth_url, csrf_token) = self
            .client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new(SCOPES[0].to_string()))
            .add_scope(Scope::new(SCOPES[1].to_string()))
            .add_scope(Scope::new(SCOPES[2].to_string()))
            .add_extra_param("access_type", "offline")
            .add_extra_param("prompt", "consent")
            .set_pkce_challenge(pkce_challenge)
            .url();

        // Store verifier in static so callback can access it
        *PKCE_VERIFIER.lock().unwrap() = Some(pkce_verifier.secret().clone());

        (auth_url.to_string(), csrf_token.secret().clone())
    }

    pub async fn wait_for_authentication(&self) -> Result<()> {
        let two_days_secs = 2 * 24 * 60 * 60;
        let check_interval_secs = 10u64;
        let max_attempts = two_days_secs / check_interval_secs;

        loop {
            let (auth_url, csrf_state) = self.build_authorization_url();

            tracing::info!("Sending authentication URL via email");
            email::send_auth_link(&auth_url)
                .await
                .context("Failed to send auth email")?;

            tracing::info!("CSRF state for this auth session: {}", csrf_state);

            for _ in 0..max_attempts {
                tokio::time::sleep(Duration::from_secs(check_interval_secs)).await;

                if let Ok(Some(token)) = cache::get::<StoredToken>("calendar_token").await {
                    if token.expiry > Utc::now().timestamp() {
                        let authenticated = self.authenticated.clone();
                        tokio::task::spawn_blocking(move || {
                            *authenticated.lock().unwrap() = true;
                        })
                        .await
                        .unwrap();
                        tracing::info!("User authenticated successfully");
                        return Ok(());
                    }
                }
            }

            tracing::warn!("User did not authenticate within 2 days, sending new email");
        }
    }

    pub async fn exchange_code(&self, code: &str) -> Result<StoredToken> {
        tracing::info!(
            "Exchanging code for token with redirect_uri: {}",
            self.redirect_uri
        );

        let pkce_verifier_str = PKCE_VERIFIER
            .lock()
            .unwrap()
            .take()
            .context("No PKCE verifier found - authentication flow may have restarted")?;
        let pkce_verifier = PkceCodeVerifier::new(pkce_verifier_str);

        let token_response = self
            .client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .set_pkce_verifier(pkce_verifier)
            .request_async(oauth2::reqwest::async_http_client)
            .await
            .context("Failed to exchange code for token")?;

        let access_token = token_response.access_token().secret().clone();
        let refresh_token = token_response.refresh_token().map(|t| t.secret().clone());
        let expires_in = token_response
            .expires_in()
            .map(|d| d.as_secs() as i64)
            .unwrap_or(3600);

        let expiry = Utc::now().timestamp() + expires_in;

        let stored_token = StoredToken {
            access_token,
            refresh_token,
            expiry,
        };

        cache::put(
            "calendar_token",
            stored_token.clone(),
            Duration::from_secs(365 * 24 * 60 * 60),
        )
        .await
        .context("Failed to store token in cache")?;

        let authenticated = self.authenticated.clone();
        tokio::task::spawn_blocking(move || {
            *authenticated.lock().unwrap() = true;
        })
        .await
        .unwrap();

        tracing::info!("Successfully stored token in cache");

        Ok(stored_token)
    }

    pub async fn refresh_token(&self, refresh_token: &str) -> Result<StoredToken> {
        let token_response = self
            .client
            .exchange_refresh_token(&oauth2::RefreshToken::new(refresh_token.to_string()))
            .request_async(oauth2::reqwest::async_http_client)
            .await
            .context("Failed to refresh token")?;

        let access_token = token_response.access_token().secret().clone();
        let new_refresh_token = token_response
            .refresh_token()
            .map(|t| t.secret().clone())
            .unwrap_or_else(|| refresh_token.to_string());
        let expires_in = token_response
            .expires_in()
            .map(|d| d.as_secs() as i64)
            .unwrap_or(3600);

        let expiry = Utc::now().timestamp() + expires_in;

        let stored_token = StoredToken {
            access_token,
            refresh_token: Some(new_refresh_token),
            expiry,
        };

        cache::put(
            "calendar_token",
            stored_token.clone(),
            Duration::from_secs(365 * 24 * 60 * 60),
        )
        .await
        .context("Failed to store refreshed token in cache")?;

        Ok(stored_token)
    }

    async fn get_token_internal(&self) -> Result<Option<String>> {
        let stored_token = self.stored_token.clone();
        let cached_token = cache::get::<StoredToken>("calendar_token")
            .await
            .ok()
            .flatten();

        let token = tokio::task::spawn_blocking(move || stored_token.lock().unwrap().clone())
            .await
            .unwrap()
            .or(cached_token);

        if let Some(ref token) = token {
            if token.expiry > Utc::now().timestamp() + 300 {
                return Ok(Some(token.access_token.clone()));
            }

            if let Some(ref refresh_token) = token.refresh_token {
                let refresh_token = refresh_token.clone();

                match self.refresh_token(&refresh_token).await {
                    Ok(new_token) => {
                        let access_token = new_token.access_token.clone();
                        let stored = self.stored_token.clone();
                        tokio::task::spawn_blocking(move || {
                            *stored.lock().unwrap() = Some(new_token);
                        })
                        .await
                        .unwrap();
                        return Ok(Some(access_token));
                    }
                    Err(e) => {
                        tracing::error!("Failed to refresh token: {}", e);
                    }
                }
            }
        }

        Ok(None)
    }
}

impl GetToken for WebFlowAuthenticator {
    fn get_token<'a>(
        &'a self,
        _scopes: &'a [&str],
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<Option<String>, Box<dyn std::error::Error + Send + Sync>>,
                > + Send
                + 'a,
        >,
    > {
        let this = self.clone();
        Box::pin(async move {
            match this.get_token_internal().await {
                Ok(token) => Ok(token),
                Err(e) => Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
                    as Box<dyn std::error::Error + Send + Sync>),
            }
        })
    }
}

impl Clone for WebFlowAuthenticator {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            redirect_uri: self.redirect_uri.clone(),
            http_client: Client::new(),
            pkce_verifier: Mutex::new(None),
            stored_token: Arc::new(Mutex::new(None)),
            authenticated: Arc::new(Mutex::new(false)),
        }
    }
}
