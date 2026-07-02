use std::{
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Datelike, NaiveTime, TimeDelta, TimeZone, Utc};
use graph_rs_sdk::{GraphClient, ODataQuery};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, RedirectUrl, RefreshToken,
    Scope as OAuthScope, TokenResponse, TokenUrl, basic::BasicClient,
};
use serde::{Deserialize, Serialize};
use tracing::instrument;

use crate::{
    adapters::{cache::PersistentCache, email},
    domain::{calendar::CalendarEvent, ports::CalendarProvider},
};

const TOKEN_CACHE_KEY: &str = "microsoft_calendar_token";
/// Latest CSRF `state` sent out by `wait_for_authentication`; only the most recent auth email
/// is valid. Verified by the OAuth callback before exchanging the code.
const CSRF_STATE_KEY: &str = "microsoft_oauth_csrf_state";

const SCOPES: [&str; 2] = ["offline_access", "https://graph.microsoft.com/Calendars.Read"];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expiry: i64,
}

pub struct O365Authenticator {
    client: BasicClient,
    cache: Arc<PersistentCache>,
    auth_lock: tokio::sync::Mutex<()>,
}

impl O365Authenticator {
    pub fn new(
        client_id: String,
        client_secret: String,
        tenant_id: String,
        redirect_uri: String,
        cache: Arc<PersistentCache>,
    ) -> Self {
        let auth_url = AuthUrl::new(format!(
            "https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/authorize"
        ))
        .expect("Invalid Microsoft auth URL");
        let token_url = TokenUrl::new(format!(
            "https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/token"
        ))
        .expect("Invalid Microsoft token URL");

        let client = BasicClient::new(
            ClientId::new(client_id),
            Some(ClientSecret::new(client_secret)),
            auth_url,
            Some(token_url),
        )
        .set_redirect_uri(RedirectUrl::new(redirect_uri).expect("Invalid redirect URL"));

        Self {
            client,
            cache,
            auth_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub fn build_authorization_url(&self) -> (String, String) {
        let (auth_url, csrf_token) = self
            .client
            .authorize_url(CsrfToken::new_random)
            .add_scope(OAuthScope::new(SCOPES[0].to_string()))
            .add_scope(OAuthScope::new(SCOPES[1].to_string()))
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
                .put(CSRF_STATE_KEY, csrf_state, Duration::from_secs(two_days_secs))
                .await?;

            tracing::info!("Sending Microsoft authentication URL via email");
            email::send_microsoft_auth_link(&auth_url)
                .await
                .context("Failed to send Microsoft auth email")?;

            for _ in 0..max_attempts {
                tokio::time::sleep(Duration::from_secs(check_interval_secs)).await;

                if let Ok(Some(token)) = self.cache.get::<StoredToken>(TOKEN_CACHE_KEY).await
                    && token.expiry > Utc::now().timestamp()
                {
                    tracing::info!("Microsoft user authenticated successfully");
                    return Ok(token.access_token);
                }
            }

            tracing::warn!("Microsoft user did not authenticate within 2 days, sending new email");
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
            .context("Failed to exchange Microsoft code for token")?;

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
            .context("Failed to store Microsoft token in cache")?;

        tracing::info!("Successfully stored Microsoft token in cache");

        Ok(stored_token)
    }

    pub async fn refresh_token(&self, refresh_token: &str) -> Result<StoredToken> {
        let token_response = self
            .client
            .exchange_refresh_token(&RefreshToken::new(refresh_token.to_string()))
            .request_async(oauth2::reqwest::async_http_client)
            .await
            .context("Failed to refresh Microsoft token")?;

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
            .context("Failed to store refreshed Microsoft token in cache")?;

        Ok(stored_token)
    }

    pub async fn get_access_token(&self) -> Result<String> {
        let token = self
            .cache
            .get::<StoredToken>(TOKEN_CACHE_KEY)
            .await
            .ok()
            .flatten();

        if let Some(ref token) = token {
            if token.expiry > Utc::now().timestamp() + 300 {
                return Ok(token.access_token.clone());
            }

            if let Some(ref refresh_token) = token.refresh_token {
                match self.refresh_token(refresh_token).await {
                    Ok(new_token) => return Ok(new_token.access_token),
                    Err(e) => {
                        tracing::error!(error = ?e, "Failed to refresh Microsoft token");
                    }
                }
            }
        }

        self.wait_for_authentication().await
    }
}

pub struct MicrosoftCalendar {
    auth: Arc<O365Authenticator>,
    cache: Arc<PersistentCache>,
}

impl MicrosoftCalendar {
    pub fn new(auth: Arc<O365Authenticator>, cache: Arc<PersistentCache>) -> Self {
        Self { auth, cache }
    }

    /// Fetch (and cache for 5 min) the full calendarView for the week(s) covering `[start, end]`.
    /// Shared by `is_busy` and `get_events` so both see identical events.
    async fn fetch_week_events(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<GraphEvent>> {
        let start_weekday = start.weekday().num_days_from_monday() as i64;
        let end_weekday = end.weekday().num_days_from_monday() as i64;
        let week_start =
            start.date_naive().and_time(NaiveTime::MIN).and_utc() - TimeDelta::days(start_weekday);
        let week_end = end
            .date_naive()
            .and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
            .and_utc()
            + TimeDelta::days(6 - end_weekday);

        let mut hasher = DefaultHasher::new();
        week_start.hash(&mut hasher);
        week_end.hash(&mut hasher);
        let cache_key = format!("microsoft_calendar_events_hash_{}", hasher.finish());

        if let Some(cached) = self.cache.get(&cache_key).await? {
            return Ok(cached);
        }

        let token = self.auth.get_access_token().await?;
        let client = GraphClient::new(token);

        let start_iso = week_start.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let end_iso = week_end.format("%Y-%m-%dT%H:%M:%SZ").to_string();

        let response = client
            .me()
            .calendar_views()
            .list_calendar_view()
            .append_query_pair("startDateTime", &start_iso)
            .append_query_pair("endDateTime", &end_iso)
            .select(&[
                "start",
                "end",
                "subject",
                "location",
                "isAllDay",
                "responseStatus",
                "isCancelled",
                "showAs",
            ])
            .send()
            .await
            .context("Failed to query Microsoft calendar view")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Microsoft Graph calendarView returned {status}: {body}"
            ));
        }

        let body: CalendarViewResponse = response
            .json()
            .await
            .context("Failed to parse Microsoft calendar view response")?;

        self.cache
            .put(&cache_key, body.value.clone(), Duration::from_secs(5 * 60))
            .await?;
        Ok(body.value)
    }
}

#[derive(Debug, Deserialize)]
struct CalendarViewResponse {
    value: Vec<GraphEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphEvent {
    start: GraphDateTime,
    end: GraphDateTime,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    location: Option<GraphLocation>,
    #[serde(default)]
    is_all_day: bool,
    #[serde(default)]
    response_status: Option<ResponseStatus>,
    #[serde(default)]
    is_cancelled: bool,
    #[serde(default)]
    show_as: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphLocation {
    #[serde(default)]
    display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphDateTime {
    date_time: String,
    time_zone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResponseStatus {
    response: String,
}

impl GraphEvent {
    fn is_user_accepted(&self) -> bool {
        matches!(
            self.response_status.as_ref().map(|r| r.response.as_str()),
            Some("accepted") | Some("organizer")
        )
    }

    /// A GraphEvent that survived the busy filters, mapped to the domain type. `None` if either
    /// endpoint is unparseable.
    fn into_calendar_event(self) -> Option<CalendarEvent> {
        Some(CalendarEvent {
            title: self.subject.clone().unwrap_or_default(),
            start_time: parse_graph_datetime(&self.start)?,
            end_time: parse_graph_datetime(&self.end)?,
            is_all_day: self.is_all_day,
            location: self.location.and_then(|l| l.display_name),
            body: None,
            color_id: None,
        })
    }

    fn overlaps(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
        let event_start = match parse_graph_datetime(&self.start) {
            Some(t) => t,
            None => return false,
        };
        let event_end = match parse_graph_datetime(&self.end) {
            Some(t) => t,
            None => return false,
        };
        event_start < end && event_end > start
    }
}

fn parse_graph_datetime(dt: &GraphDateTime) -> Option<DateTime<Utc>> {
    let truncated = dt.date_time.split('.').next().unwrap_or(&dt.date_time);
    let naive = chrono::NaiveDateTime::parse_from_str(truncated, "%Y-%m-%dT%H:%M:%S").ok()?;
    let tz: chrono_tz::Tz = dt.time_zone.parse().ok().or_else(|| {
        if dt.time_zone.eq_ignore_ascii_case("utc") {
            Some(chrono_tz::UTC)
        } else {
            None
        }
    })?;
    let zoned = tz.from_local_datetime(&naive).single()?;
    Some(zoned.with_timezone(&Utc))
}

#[async_trait]
impl CalendarProvider for MicrosoftCalendar {
    #[instrument(skip(self))]
    async fn get_events(
        &self,
        _calendars: &[String],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<CalendarEvent>> {
        // `_calendars` is ignored (calendarView is the default calendar).
        let events = self
            .fetch_week_events(start, end)
            .await?
            .into_iter()
            .filter(|e| !e.is_cancelled)
            .filter(|e| e.show_as.as_deref() != Some("free"))
            .filter(|e| e.is_user_accepted())
            .filter(|e| e.overlaps(start, end))
            .filter_map(|e| e.into_calendar_event())
            .collect();

        Ok(events)
    }

    async fn get_calendar_names(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    async fn clear_calendar(&self, _name: &str) -> Result<()> {
        Err(anyhow!("MicrosoftCalendar is read-only"))
    }

    async fn create_event(&self, _calendar: &str, _event: CalendarEvent) -> Result<()> {
        Err(anyhow!("MicrosoftCalendar is read-only"))
    }

    async fn create_calendar(&self, _name: &str) -> Result<()> {
        Err(anyhow!("MicrosoftCalendar is read-only"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evt(start: &str, end: &str, response: &str, cancelled: bool, show_as: Option<&str>) -> GraphEvent {
        GraphEvent {
            start: GraphDateTime {
                date_time: start.to_string(),
                time_zone: "UTC".to_string(),
            },
            end: GraphDateTime {
                date_time: end.to_string(),
                time_zone: "UTC".to_string(),
            },
            subject: None,
            location: None,
            is_all_day: false,
            response_status: Some(ResponseStatus {
                response: response.to_string(),
            }),
            is_cancelled: cancelled,
            show_as: show_as.map(str::to_string),
        }
    }

    fn ts(h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 15, h, 0, 0).unwrap()
    }

    #[test]
    fn accepted_event_in_window_is_busy() {
        let e = evt("2026-06-15T10:00:00", "2026-06-15T11:00:00", "accepted", false, None);
        assert!(e.is_user_accepted());
        assert!(e.overlaps(ts(9), ts(12)));
    }

    #[test]
    fn declined_event_is_not_counted() {
        let e = evt("2026-06-15T10:00:00", "2026-06-15T11:00:00", "declined", false, None);
        assert!(!e.is_user_accepted());
    }

    #[test]
    fn tentative_event_is_not_counted() {
        let e = evt("2026-06-15T10:00:00", "2026-06-15T11:00:00", "tentativelyAccepted", false, None);
        assert!(!e.is_user_accepted());
    }

    #[test]
    fn organizer_event_is_counted() {
        let e = evt("2026-06-15T10:00:00", "2026-06-15T11:00:00", "organizer", false, None);
        assert!(e.is_user_accepted());
    }

    #[test]
    fn non_overlapping_event_is_not_busy() {
        let e = evt("2026-06-15T14:00:00", "2026-06-15T15:00:00", "accepted", false, None);
        assert!(!e.overlaps(ts(9), ts(12)));
    }

    #[test]
    fn parse_datetime_handles_microseconds() {
        let dt = GraphDateTime {
            date_time: "2026-06-15T10:00:00.0000000".to_string(),
            time_zone: "UTC".to_string(),
        };
        assert!(parse_graph_datetime(&dt).is_some());
    }
}
