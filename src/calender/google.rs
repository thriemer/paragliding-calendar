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

use crate::{
    auth::{StoredToken, WebFlowAuthenticator, get_redirect_uri},
    cache,
    calender::{CalendarEvent, CalendarProvider},
};

pub type CalendarHubType =
    CalendarHub<HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>>;

pub struct GoogleCalendar {
    hub: CalendarHubType,
}

impl GoogleCalendar {
    pub async fn new() -> Result<Self> {
        // Wait for authentication if needed
        Self::wait_for_authentication().await?;

        // Build HTTP client
        let connector = HttpsConnectorBuilder::new()
            .with_native_roots()
            .context("Failed to build HTTPS connector")?
            .https_only()
            .enable_http2()
            .build();

        let hyper_client = Client::builder(TokioExecutor::new()).build(connector);

        // Get stored token from cache
        let stored_token = cache::get::<StoredToken>("calendar_token")
            .await
            .context("Failed to get token from cache")?
            .context("No token found after authentication")?;

        // Create WebFlowAuthenticator and set the stored token
        let client_id = env::var("GOOGLE_CLIENT_ID")
            .or_else(|_| env::var("GOOGLE_CALENDAR_CLIENT_ID"))
            .context("Missing GOOGLE_CLIENT_ID")?;
        let client_secret = env::var("GOOGLE_CLIENT_SECRET")
            .or_else(|_| env::var("GOOGLE_CALENDAR_CLIENT_SECRET"))
            .context("Missing GOOGLE_CLIENT_SECRET")?;
        let redirect_uri = get_redirect_uri();

        let auth = WebFlowAuthenticator::new(client_id, client_secret, redirect_uri);
        auth.set_stored_token(stored_token);

        let hub = CalendarHub::new(hyper_client, auth);
        Ok(GoogleCalendar { hub })
    }

    async fn wait_for_authentication() -> Result<()> {
        // Check if valid token exists in cache
        let token: Option<crate::auth::web_flow_authenticator::StoredToken> =
            match cache::get("calendar_token").await {
                Ok(Some(token)) => Some(token),
                _ => None,
            };

        if let Some(token) = token {
            if token.expiry > chrono::Utc::now().timestamp() {
                tracing::info!("Found valid token in cache");
                return Ok(());
            }
        }

        // Need to authenticate - use WebFlowAuthenticator
        let client_id = env::var("GOOGLE_CLIENT_ID")
            .or_else(|_| env::var("GOOGLE_CALENDAR_CLIENT_ID"))
            .context("Missing GOOGLE_CLIENT_ID")?;
        let client_secret = env::var("GOOGLE_CLIENT_SECRET")
            .or_else(|_| env::var("GOOGLE_CALENDAR_CLIENT_SECRET"))
            .context("Missing GOOGLE_CLIENT_SECRET")?;
        let redirect_uri = get_redirect_uri();

        let auth = crate::auth::WebFlowAuthenticator::new(client_id, client_secret, redirect_uri);

        // This will loop: send email every 2 days until authenticated
        auth.wait_for_authentication().await?;

        Ok(())
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
            if let Some(busy) = cache::get(&cache_key).await? {
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

                cache::put(&cache_key, busy.clone(), Duration::from_hours(8)).await?;
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
        let (_, list) = self
            .hub
            .events()
            .list(&calendar_id)
            .add_scope(Scope::AppCreated)
            .doit()
            .await?;
        let mut counter = 0;
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
                }
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
            cache::put(&key, id, Duration::from_hours(24)).await?;
        }
        Ok(())
    }
}
