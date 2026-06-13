use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Datelike, NaiveTime, Utc};
use google_apis_common::GetToken;
use google_calendar3::{
    CalendarHub,
    api::{
        CalendarList, Event, EventDateTime, FreeBusyRequest, FreeBusyRequestItem,
        Scope,
    },
};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, RedirectUrl,
    Scope as OAuthScope, TokenResponse, TokenUrl, basic::BasicClient,
};
use tracing::instrument;

use crate::{
    adapters::{cache::PersistentCache, email},
    domain::{calendar::CalendarEvent, ports::CalendarProvider},
};

const TOKEN_CACHE_KEY: &str = "calendar_token";

const SCOPES: [&str; 3] = [
    "https://www.googleapis.com/auth/calendar.calendarlist.readonly",
    "https://www.googleapis.com/auth/calendar.app.created",
    "https://www.googleapis.com/auth/calendar.freebusy",
];

pub struct WebFlowAuthenticator {
    client: BasicClient,
    redirect_uri: String,
    cache: Arc<PersistentCache>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expiry: i64,
}

impl WebFlowAuthenticator {
    pub fn new(
        client_id: String,
        client_secret: String,
        redirect_uri: String,
        cache: Arc<PersistentCache>,
    ) -> Self {
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
            cache,
        }
    }

    pub fn build_authorization_url(&self) -> (String, String) {
        let (auth_url, csrf_token) = self
            .client
            .authorize_url(CsrfToken::new_random)
            .add_scope(OAuthScope::new(SCOPES[0].to_string()))
            .add_scope(OAuthScope::new(SCOPES[1].to_string()))
            .add_scope(OAuthScope::new(SCOPES[2].to_string()))
            .add_extra_param("access_type", "offline")
            .add_extra_param("prompt", "consent")
            .url();

        (auth_url.to_string(), csrf_token.secret().clone())
    }

    pub async fn wait_for_authentication(&self) -> Result<String> {
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

                if let Ok(Some(token)) = self.cache.get::<StoredToken>(TOKEN_CACHE_KEY).await {
                    if token.expiry > Utc::now().timestamp() {
                        tracing::info!("User authenticated successfully");
                        return Ok(token.access_token);
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

        let token_response = self
            .client
            .exchange_code(AuthorizationCode::new(code.to_string()))
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

        self.cache
            .put(
                TOKEN_CACHE_KEY,
                stored_token.clone(),
                Duration::from_secs(365 * 24 * 60 * 60),
            )
            .await
            .context("Failed to store token in cache")?;

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

        self.cache
            .put(
                TOKEN_CACHE_KEY,
                stored_token.clone(),
                Duration::from_secs(365 * 24 * 60 * 60),
            )
            .await
            .context("Failed to store refreshed token in cache")?;

        Ok(stored_token)
    }

    async fn get_token_internal(&self) -> Result<Option<String>> {
        let token = self
            .cache
            .get::<StoredToken>(TOKEN_CACHE_KEY)
            .await
            .ok()
            .flatten();

        if let Some(ref token) = token {
            if token.expiry > Utc::now().timestamp() + 300 {
                return Ok(Some(token.access_token.clone()));
            }

            if let Some(ref refresh_token) = token.refresh_token {
                match self.refresh_token(&refresh_token).await {
                    Ok(new_token) => {
                        let access_token = new_token.access_token.clone();
                        self.cache
                            .put(TOKEN_CACHE_KEY, new_token, Duration::from_hours(24 * 30))
                            .await?;
                        return Ok(Some(access_token));
                    }
                    Err(e) => {
                        tracing::error!("Failed to refresh token: {}", e);
                    }
                }
            }
        }

        Ok(Some(self.wait_for_authentication().await?))
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
            cache: self.cache.clone(),
        }
    }
}

pub type CalendarHubType =
    CalendarHub<HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>>;

pub struct GoogleCalendar {
    hub: CalendarHubType,
    cache: Arc<PersistentCache>,
}

impl GoogleCalendar {
    pub async fn new(
        auth: Arc<WebFlowAuthenticator>,
        cache: Arc<PersistentCache>,
    ) -> Result<Self> {
        let connector = HttpsConnectorBuilder::new()
            .with_native_roots()
            .context("Failed to build HTTPS connector")?
            .https_only()
            .enable_http2()
            .build();

        let hyper_client = Client::builder(TokioExecutor::new()).build(connector);
        let auth = (*auth).clone();
        let hub = CalendarHub::new(hyper_client, auth);
        Ok(GoogleCalendar { hub, cache })
    }

    async fn get_id_for_name(&self, name: &str) -> Result<String> {
        let key = format!("calendar_name_id_map_{}", name);

        if let Some(id) = self.cache.get(&key).await? {
            return Ok(id);
        }

        let list = self.get_calendar_list().await?;
        let lists = list.items.ok_or(anyhow!("Empty calendar list"))?;
        let result = lists
            .iter()
            .filter(|l| {
                if let Some(desc) = &l.summary {
                    desc == name
                } else {
                    false
                }
            })
            .map(|l| l.id.clone().unwrap())
            .collect::<Vec<String>>()
            .first()
            .cloned();

        if let Some(id) = result {
            self.cache
                .put(&key, id.clone(), Duration::from_hours(72))
                .await?;
            Ok(id.to_owned())
        } else {
            Err(anyhow!("Calendar id not found for name {}", name))
        }
    }

    async fn get_calendar_list(&self) -> Result<CalendarList> {
        let (_, lists) = self
            .hub
            .calendar_list()
            .list()
            .add_scope(Scope::CalendarlistReadonly)
            .doit()
            .await?;
        Ok(lists)
    }
}

#[async_trait]
impl CalendarProvider for GoogleCalendar {
    #[instrument(skip(self))]
    async fn is_busy(
        &self,
        calendars: &Vec<String>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<bool> {
        let items = futures::future::join_all(
            calendars
                .iter()
                .map(async |n| {
                    let id = match self.get_id_for_name(n).await {
                        Ok(id) => Some(id),
                        Err(err) => {
                            tracing::warn!("Cant get id for name {}. Error {:?}", n, err);
                            None
                        }
                    };
                    FreeBusyRequestItem { id }
                })
                .collect::<Vec<_>>(),
        )
        .await;

        let start_weekday = start.weekday().num_days_from_monday() as u64;
        let end_weekday = end.weekday().num_days_from_monday() as u64;
        let week_start_datetime = start.date_naive().and_time(NaiveTime::MIN).and_utc()
            - Duration::from_hours(24u64 * start_weekday);
        let week_end_datetime = end
            .date_naive()
            .and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
            .and_utc()
            + Duration::from_hours(24u64 * (7u64 - end_weekday));

        let mut hasher = DefaultHasher::new();
        calendars.hash(&mut hasher);
        week_start_datetime.hash(&mut hasher);
        week_end_datetime.hash(&mut hasher);
        let cache_key = format!("Calendar_free_busy_hash_{}", hasher.finish());

        let busy = {
            if let Some(busy) = self.cache.get(&cache_key).await? {
                busy
            } else {
                let (_, busy) = self
                    .hub
                    .freebusy()
                    .query(FreeBusyRequest {
                        items: Some(items.clone()),
                        time_min: Some(week_start_datetime),
                        time_max: Some(week_end_datetime),
                        group_expansion_max: None,
                        calendar_expansion_max: None,
                        time_zone: None,
                    })
                    .add_scope(Scope::Freebusy)
                    .doit()
                    .await?;

                self.cache
                    .put(&cache_key, busy.clone(), Duration::from_hours(8))
                    .await?;
                busy
            }
        };

        let mut b: bool = false;

        if let Some(freebusy) = busy.calendars {
            b = items
                .iter()
                .filter_map(|i| i.id.clone())
                .filter_map(|i| {
                    if let Some(fb) = freebusy.get(&i) {
                        return fb.busy.clone();
                    }
                    None
                })
                .flatten()
                .any(|tp| start < tp.end.unwrap() && end > tp.start.unwrap());
        }
        tracing::debug!(
            "Range from {} - {} is {}",
            start,
            end,
            if b { "busy" } else { "free" }
        );
        Ok(b)
    }

    #[instrument(skip(self), fields(calendar = %name))]
    async fn clear_calendar(&mut self, name: &str) -> anyhow::Result<()> {
        let calendar_id = self.get_id_for_name(name).await?;
        let mut page_token: Option<String> = None;
        let mut counter = 0;

        loop {
            let mut request = self
                .hub
                .events()
                .list(&calendar_id)
                .add_scope(Scope::AppCreated);

            if let Some(ref token) = page_token {
                request = request.page_token(token);
            }

            let (_, list) = request.doit().await?;

            if let Some(events) = list.items {
                for e in events {
                    if let Some(event_id) = e.id {
                        self.hub
                            .events()
                            .delete(&calendar_id, &event_id)
                            .add_scope(Scope::AppCreated)
                            .doit()
                            .await?;
                        counter += 1;
                    } else {
                        tracing::warn!("Event {:#?} has no event_id", e);
                    }
                }
            }

            page_token = list.next_page_token;
            if page_token.is_none() {
                break;
            }
        }

        tracing::info!("Cleared {} events", counter);
        Ok(())
    }

    #[instrument(skip(self), fields(calendar = %calendar))]
    async fn create_event(&mut self, calendar: &str, event: CalendarEvent) -> Result<()> {
        let id = self.get_id_for_name(calendar).await?;
        self.hub
            .events()
            .insert(event.into(), &id)
            .add_scope(Scope::AppCreated)
            .doit()
            .await?;
        Ok(())
    }

    #[instrument(skip(self))]
    async fn get_calendar_names(&self) -> Result<Vec<String>> {
        let lists = self.get_calendar_list().await?;
        let mut names = vec![];
        if let Some(lists) = lists.items {
            for l in lists {
                if let Some(name) = l.summary {
                    names.push(name);
                }
            }
        }
        Ok(names)
    }

    #[instrument(skip(self), fields(calendar = %name))]
    async fn create_calendar(&mut self, name: &str) -> Result<()> {
        if self.get_calendar_names().await?.contains(&name.to_owned()) {
            tracing::info!("Calendar {} already exists, Skipping creation", name);
            return Ok(());
        }
        let mut cal = google_calendar3::api::Calendar::default();
        cal.summary = Some(name.into());
        let (_, cal) = self
            .hub
            .calendars()
            .insert(cal)
            .add_scope(Scope::AppCreated)
            .doit()
            .await?;

        if let Some(id) = cal.id {
            let key = format!("calendar_name_id_map_{}", name);
            self.cache
                .put(&key, id, Duration::from_hours(24))
                .await?;
        }
        Ok(())
    }
}

impl From<CalendarEvent> for Event {
    fn from(value: CalendarEvent) -> Self {
        let mut event = Event::default();
        event.summary = Some(value.title);
        event.start = Some(to_event_time(value.start_time));
        event.end = Some(to_event_time(value.end_time));
        event.location = value.location;
        event.description = value.body;
        event
    }
}

fn to_event_time(time: DateTime<Utc>) -> EventDateTime {
    EventDateTime {
        date: None,
        date_time: Some(time),
        time_zone: None,
    }
}
