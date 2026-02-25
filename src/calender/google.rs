use std::{env, time::Duration};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Datelike, NaiveTime, TimeZone, Utc};
use google_calendar3::{
    CalendarHub,
    api::{CalendarList, Event, Events, FreeBusyRequest, FreeBusyRequestItem, Scope},
};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use std::hash::{DefaultHasher, Hash, Hasher};
use tracing::instrument;
use yup_oauth2::{ApplicationSecret, InstalledFlowAuthenticator, InstalledFlowReturnMethod};

use async_trait::async_trait;
use yup_oauth2::storage::{TokenInfo, TokenStorage, TokenStorageError};

use crate::{
    cache,
    calender::{CalendarEvent, CalendarProvider},
};
pub type CalendarHubType =
    CalendarHub<HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>>;

struct CalendarTokenStorage;

impl CalendarTokenStorage {
    fn cache_key(&self, scopes: &[&str]) -> String {
        let mut hasher = DefaultHasher::new();
        for scope in scopes {
            scope.hash(&mut hasher);
        }
        //format!("calendar_token:{:x}", hasher.finish())
        "calendar_token".into()
    }
}

#[async_trait]
impl TokenStorage for CalendarTokenStorage {
    async fn set(&self, scopes: &[&str], token: TokenInfo) -> Result<(), TokenStorageError> {
        let key = self.cache_key(scopes);

        cache::put(&key, token, chrono::Duration::days(365).to_std().unwrap())
            .await
            .map_err(|e| TokenStorageError::Other(e.to_string().into()))?;

        Ok(())
    }

    async fn get(&self, scopes: &[&str]) -> Option<TokenInfo> {
        let key = self.cache_key(scopes);

        let token: Option<TokenInfo> = match cache::get(&key).await {
            Ok(data) => data,
            Err(e) => {
                tracing::warn!("Failed to get calendar token from cache: {}", e);
                return None;
            }
        };
        token
    }
}

pub struct GoogleCalendar {
    hub: CalendarHubType,
}

impl GoogleCalendar {
    pub async fn new() -> Result<Self> {
        let secret = load_credentials().await?;
        // Build an HTTP client with TLS
        let connector = HttpsConnectorBuilder::new()
            .with_native_roots()
            .context("Failed to build HTTPS connector")?
            .https_only()
            .enable_http2()
            .build();

        let hyper_client = Client::builder(TokioExecutor::new()).build(connector);

        // Build the authenticator with cache-based token storage
        let token_storage = CalendarTokenStorage {};
        let auth = InstalledFlowAuthenticator::builder(
            secret.clone(),
            InstalledFlowReturnMethod::HTTPRedirect,
        )
        .with_storage(Box::new(token_storage))
        .force_account_selection(true)
        .build()
        .await
        .context("Failed to create authenticator")?;
        let _token = auth
            .token(&[
                "https://www.googleapis.com/auth/calendar.calendarlist.readonly",
                "https://www.googleapis.com/auth/calendar.app.created",
                "https://www.googleapis.com/auth/calendar.freebusy",
            ])
            .await
            .context("Failed to acquire token with required scopes")?;

        // Create Calendar Hub with the hyper_client and the authenticator
        let hub = CalendarHub::new(hyper_client, auth);
        Ok(GoogleCalendar { hub })
    }

    async fn get_id_for_name(&self, name: &str) -> Result<String> {
        let key = format!("calendar_name_id_map_{}", name);

        if let Some(id) = cache::get(&key).await? {
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
            cache::put(&key, id.clone(), Duration::from_hours(72)).await?;
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

        // snap start and finish to start/end of the week to reduce requests
        let start_weekday = start.weekday().num_days_from_monday() as u64;
        let end_weekday = end.weekday().num_days_from_monday() as u64;
        let start = start.date_naive().and_time(NaiveTime::MIN).and_utc()
            - Duration::from_hours(24u64 * start_weekday);
        let end = end
            .date_naive()
            .and_time(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
            .and_utc()
            + Duration::from_hours(24u64 * (7u64 - end_weekday));

        let mut hasher = DefaultHasher::new();
        calendars.hash(&mut hasher);
        start.hash(&mut hasher);
        end.hash(&mut hasher);
        let cache_key = format!("Calendar_free_busy_hash_{}", hasher.finish());

        let busy = {
            if let Some(busy) = cache::get(&cache_key).await? {
                busy
            } else {
                let (_, busy) = self
                    .hub
                    .freebusy()
                    .query(FreeBusyRequest {
                        items: Some(items),
                        time_min: Some(start),
                        time_max: Some(end),
                        group_expansion_max: None,
                        calendar_expansion_max: None,
                        time_zone: None,
                    })
                    .add_scope(Scope::Freebusy)
                    .doit()
                    .await?;

                cache::put(&cache_key, busy.clone(), Duration::from_hours(8)).await?;
                busy
            }
        };

        let b: bool = busy
            .calendars
            .and_then(|calendars| {
                if let Some(fb) = calendars.get("busy") {
                    return Some(fb.clone());
                }
                None
            })
            .and_then(|m| m.busy.clone())
            .and_then(|v| {
                Some(
                    v.iter()
                        .any(|tp| start < tp.end.unwrap() && end > tp.start.unwrap()),
                )
            })
            .unwrap_or(false);
        Ok(b)
    }

    #[instrument(skip(self), fields(calendar = %name))]
    async fn clear_calendar(&mut self, name: &str) -> anyhow::Result<()> {
        let calendar_id = self.get_id_for_name(name).await?;
        let (_, list) = self
            .hub
            .events()
            .list(&calendar_id)
            .add_scope(Scope::AppCreated)
            .doit()
            .await?;
        if let Some(events) = list.items {
            for e in events {
                if let Some(event_id) = e.id {
                    self.hub
                        .events()
                        .delete(&calendar_id, &event_id)
                        .add_scope(Scope::AppCreated)
                        .doit()
                        .await?;
                }
            }
        }
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
        let (_, _cal) = self
            .hub
            .calendars()
            .insert(cal)
            .add_scope(Scope::AppCreated)
            .doit()
            .await?;
        Ok(())
    }
}

/// Load Google API credentials from environment variables
async fn load_credentials() -> Result<ApplicationSecret> {
    let client_id = env::var("GOOGLE_CLIENT_ID")
        .or_else(|_| env::var("GOOGLE_CALENDAR_CLIENT_ID"))
        .context("Missing GOOGLE_CLIENT_ID environment variable")?;

    let client_secret = env::var("GOOGLE_CLIENT_SECRET")
        .or_else(|_| env::var("GOOGLE_CALENDAR_CLIENT_SECRET"))
        .context("Missing GOOGLE_CLIENT_SECRET environment variable")?;

    Ok(ApplicationSecret {
        client_id,
        client_secret,
        token_uri: "https://oauth2.googleapis.com/token".to_string(),
        auth_uri: "https://accounts.google.com/o/oauth2/auth".to_string(),
        ..ApplicationSecret::default()
    })
}
