use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, NaiveTime, Utc};
use google_apis_common::GetToken;
use google_calendar3::{
    CalendarHub,
    api::{CalendarList, Event, EventDateTime, Scope},
};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, RedirectUrl,
    Scope as OAuthScope, TokenResponse, TokenUrl, basic::BasicClient,
};
use tracing::instrument;

use crate::{
    adapters::{email, persistence::cache::PersistentCache},
    domain::{calendar::CalendarEvent, ports::CalendarProvider},
};

// v2: bumped when `calendar.events.readonly` was added — cached v1 tokens lack the scope and
// would 403 on events.list forever, so we ignore them and force the prompt=consent re-auth.
const TOKEN_CACHE_KEY: &str = "calendar_token_v2";
/// Latest CSRF `state` sent out by `wait_for_authentication`; only the most recent auth email
/// is valid. Verified by the OAuth callback before exchanging the code.
const CSRF_STATE_KEY: &str = "calendar_oauth_csrf_state";

const SCOPES: [&str; 4] = [
    "https://www.googleapis.com/auth/calendar.calendarlist.readonly",
    "https://www.googleapis.com/auth/calendar.app.created",
    "https://www.googleapis.com/auth/calendar.freebusy",
    "https://www.googleapis.com/auth/calendar.events.readonly",
];

pub struct WebFlowAuthenticator {
    client: BasicClient,
    redirect_uri: String,
    cache: Arc<PersistentCache>,
    auth_lock: Arc<tokio::sync::Mutex<()>>,
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
            auth_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn build_authorization_url(&self) -> (String, String) {
        let mut request = self.client.authorize_url(CsrfToken::new_random);
        for scope in SCOPES {
            request = request.add_scope(OAuthScope::new(scope.to_string()));
        }
        let (auth_url, csrf_token) = request
            .add_extra_param("access_type", "offline")
            .add_extra_param("prompt", "consent")
            .url();

        (auth_url.to_string(), csrf_token.secret().clone())
    }

    pub async fn wait_for_authentication(&self) -> Result<String> {
        let _guard = self.auth_lock.lock().await;

        if let Ok(Some(token)) = self.cache.get::<StoredToken>(TOKEN_CACHE_KEY).await
            && token.expiry > Utc::now().timestamp()
        {
            return Ok(token.access_token);
        }

        let two_days_secs = 2 * 24 * 60 * 60;
        let check_interval_secs = 10u64;
        let max_attempts = two_days_secs / check_interval_secs;

        loop {
            let (auth_url, csrf_state) = self.build_authorization_url();
            self.cache
                .put(
                    CSRF_STATE_KEY,
                    csrf_state,
                    Duration::from_secs(two_days_secs),
                )
                .await?;

            tracing::info!("Sending authentication URL via email");
            email::send_auth_link(&auth_url)
                .await
                .context("Failed to send auth email")?;

            for _ in 0..max_attempts {
                tokio::time::sleep(Duration::from_secs(check_interval_secs)).await;

                if let Ok(Some(token)) = self.cache.get::<StoredToken>(TOKEN_CACHE_KEY).await
                    && token.expiry > Utc::now().timestamp()
                {
                    tracing::info!("User authenticated successfully");
                    return Ok(token.access_token);
                }
            }

            tracing::warn!("User did not authenticate within 2 days, sending new email");
        }
    }

    /// True iff `state` matches the most recently issued auth link (older emails go stale).
    pub async fn verify_csrf_state(&self, state: &str) -> bool {
        matches!(
            self.cache.get::<String>(CSRF_STATE_KEY).await,
            Ok(Some(expected)) if expected == state
        )
    }

    pub async fn exchange_code(&self, code: &str) -> Result<StoredToken> {
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
                match self.refresh_token(refresh_token).await {
                    Ok(new_token) => {
                        let access_token = new_token.access_token.clone();
                        self.cache
                            .put(TOKEN_CACHE_KEY, new_token, Duration::from_hours(24 * 30))
                            .await?;
                        return Ok(Some(access_token));
                    }
                    Err(e) => {
                        tracing::error!(error = ?e, "Failed to refresh token");
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
                Err(e) => Err(Box::new(std::io::Error::other(e.to_string()))
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
            auth_lock: self.auth_lock.clone(),
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
    pub fn new(auth: Arc<WebFlowAuthenticator>, cache: Arc<PersistentCache>) -> Result<Self> {
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
    async fn get_events(
        &self,
        calendars: &[String],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<CalendarEvent>> {
        let mut out = Vec::new();
        for name in calendars {
            let calendar_id = match self.get_id_for_name(name).await {
                Ok(id) => id,
                Err(err) => {
                    tracing::warn!(name = %name, error = ?err, "Cant get id for calendar");
                    continue;
                }
            };

            let mut page_token: Option<String> = None;
            loop {
                let mut request = self
                    .hub
                    .events()
                    .list(&calendar_id)
                    .add_scope(SCOPES[3]) // calendar.events.readonly
                    .single_events(true) // expand recurring events into concrete instances
                    .time_min(start)
                    .time_max(end);
                if let Some(ref token) = page_token {
                    request = request.page_token(token);
                }

                // A calendar shared as free/busy-only (or otherwise not events-readable) returns
                // 404 here. Skip it with a warning instead of aborting the whole fetch — one
                // unreadable calendar must not wipe out every other calendar's commitments.
                let list = match request.doit().await {
                    Ok((_, list)) => list,
                    Err(err) => {
                        tracing::warn!(name = %name, id = %calendar_id, error = ?err, "events.list failed; skipping calendar");
                        break;
                    }
                };
                if let Some(events) = list.items {
                    out.extend(events.into_iter().filter_map(to_calendar_event));
                }

                page_token = list.next_page_token;
                if page_token.is_none() {
                    break;
                }
            }
        }
        Ok(out)
    }

    #[instrument(skip(self), fields(calendar = %name))]
    async fn clear_calendar(&self, name: &str) -> anyhow::Result<()> {
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
                        tracing::warn!(event = ?e, "Event has no event_id");
                    }
                }
            }

            page_token = list.next_page_token;
            if page_token.is_none() {
                break;
            }
        }

        tracing::info!(cleared = counter, "Cleared events");
        Ok(())
    }

    #[instrument(skip(self), fields(calendar = %calendar))]
    async fn create_event(&self, calendar: &str, event: CalendarEvent) -> Result<()> {
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
    async fn create_calendar(&self, name: &str) -> Result<()> {
        if self.get_calendar_names().await?.contains(&name.to_owned()) {
            tracing::info!(name = %name, "Calendar already exists, skipping creation");
            return Ok(());
        }
        let cal = google_calendar3::api::Calendar {
            summary: Some(name.into()),
            ..Default::default()
        };
        let (_, cal) = self
            .hub
            .calendars()
            .insert(cal)
            .add_scope(Scope::AppCreated)
            .doit()
            .await?;

        if let Some(id) = cal.id {
            let key = format!("calendar_name_id_map_{}", name);
            self.cache.put(&key, id, Duration::from_hours(24)).await?;
        }
        Ok(())
    }
}

impl From<CalendarEvent> for Event {
    fn from(value: CalendarEvent) -> Self {
        Event {
            summary: Some(value.title),
            start: Some(to_event_time(value.start_time)),
            end: Some(to_event_time(value.end_time)),
            location: value.location,
            description: value.body,
            color_id: value.color_id,
            ..Default::default()
        }
    }
}

fn to_event_time(time: DateTime<Utc>) -> EventDateTime {
    EventDateTime {
        date: None,
        date_time: Some(time),
        time_zone: None,
    }
}

/// Inverse of `From<CalendarEvent> for Event`. Returns `None` for events that don't block time:
/// cancelled instances and ones marked free (`transparency == "transparent"`).
fn to_calendar_event(e: Event) -> Option<CalendarEvent> {
    if e.status.as_deref() == Some("cancelled") || e.transparency.as_deref() == Some("transparent")
    {
        return None;
    }

    let start = e.start.as_ref()?;
    let end = e.end.as_ref()?;
    let (start_time, end_time, is_all_day) = match (start.date_time, end.date_time) {
        (Some(s), Some(en)) => (s, en, false),
        // All-day events carry `date` (end exclusive), so [start 00:00, end 00:00) is the span.
        _ => {
            let s = start.date?.and_time(NaiveTime::MIN).and_utc();
            let en = end.date?.and_time(NaiveTime::MIN).and_utc();
            (s, en, true)
        }
    };

    Some(CalendarEvent {
        title: e.summary.unwrap_or_default(),
        start_time,
        end_time,
        is_all_day,
        location: e.location,
        body: e.description,
        color_id: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone};

    fn timed(summary: &str) -> Event {
        Event {
            summary: Some(summary.into()),
            start: Some(to_event_time(
                Utc.with_ymd_and_hms(2026, 6, 13, 10, 0, 0).unwrap(),
            )),
            end: Some(to_event_time(
                Utc.with_ymd_and_hms(2026, 6, 13, 11, 0, 0).unwrap(),
            )),
            ..Default::default()
        }
    }

    #[test]
    fn timed_event_maps_through() {
        let ce = to_calendar_event(timed("standup")).unwrap();
        assert_eq!(ce.title, "standup");
        assert!(!ce.is_all_day);
    }

    #[test]
    fn cancelled_event_is_skipped() {
        let mut e = timed("x");
        e.status = Some("cancelled".into());
        assert!(to_calendar_event(e).is_none());
    }

    #[test]
    fn transparent_event_is_skipped() {
        let mut e = timed("x");
        e.transparency = Some("transparent".into());
        assert!(to_calendar_event(e).is_none());
    }

    #[test]
    fn all_day_event_expands_to_full_day_span() {
        let e = Event {
            summary: Some("holiday".into()),
            start: Some(EventDateTime {
                date: Some(NaiveDate::from_ymd_opt(2026, 6, 13).unwrap()),
                date_time: None,
                time_zone: None,
            }),
            end: Some(EventDateTime {
                date: Some(NaiveDate::from_ymd_opt(2026, 6, 14).unwrap()),
                date_time: None,
                time_zone: None,
            }),
            ..Default::default()
        };
        let ce = to_calendar_event(e).unwrap();
        assert!(ce.is_all_day);
        assert_eq!(
            ce.start_time,
            Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap()
        );
        assert_eq!(
            ce.end_time,
            Utc.with_ymd_and_hms(2026, 6, 14, 0, 0, 0).unwrap()
        );
    }
}
