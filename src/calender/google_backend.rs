use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, TimeZone, Utc, Weekday};
use google_calendar3::{
    CalendarHub,
    api::{CalendarList, Event, Events, FreeBusyRequest, FreeBusyRequestItem, Scope},
};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use std::{
    env,
    hash::{DefaultHasher, Hash},
    time::Duration,
};
use tracing::instrument;
use yup_oauth2::{ApplicationSecret, InstalledFlowAuthenticator, InstalledFlowReturnMethod};

use crate::{
    cache,
    calender::{CalendarEvent, CalendarProvider},
};
pub type CalendarHubType =
    CalendarHub<HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>>;
use std::hash::Hasher;

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

        // Build the authenticator
        let auth = InstalledFlowAuthenticator::builder(
            secret.clone(),
            InstalledFlowReturnMethod::HTTPRedirect,
        )
        .persist_tokens_to_disk("tokens.json")
        .build()
        .await
        .context("Failed to create authenticator")?;
        let _token = auth
            .token(&["https://www.googleapis.com/auth/calendar"])
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
            .add_scope(Scope::Full)
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

    async fn clear_calendar(&mut self, name: &str) -> anyhow::Result<()> {
        let calendar_id = self.get_id_for_name(name).await?;
        let (_, list) = self.hub.events().list(&calendar_id).doit().await?;
        if let Some(events) = list.items {
            for e in events {
                if let Some(event_id) = e.id {
                    self.hub
                        .events()
                        .delete(&calendar_id, &event_id)
                        .doit()
                        .await?;
                }
            }
        }
        Ok(())
    }

    async fn create_event(&mut self, calendar: &str, event: CalendarEvent) -> Result<()> {
        let id = self.get_id_for_name(calendar).await?;
        self.hub.events().insert(event.into(), &id).doit().await?;
        Ok(())
    }

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
            .add_scope(Scope::Full)
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

/// Fetch events from primary calendar within time range
pub async fn fetch_calendar_events(
    hub: &CalendarHubType,
    time_min: &DateTime<Utc>,
    time_max: &DateTime<Utc>,
    list: &str,
) -> Result<Vec<CalendarEvent>> {
    // Get events from primary calendar
    let result = hub
        .events()
        .list(list)
        .add_scopes(&[
            Scope::EventReadonly,
            Scope::Readonly,
            Scope::EventPublicReadonly,
        ])
        .time_min(*time_min)
        .time_max(*time_max)
        .single_events(true)
        .doit()
        .await?;

    let events_response: Events = result.1;
    let mut events = Vec::new();

    if let Some(items) = events_response.items {
        for event in items {
            // Parse start and end times
            let (start_time, end_time, is_all_day) = parse_event_times(&event)?;

            // Extract summary (title)
            let summary = event.summary.unwrap_or_else(|| "No title".to_string());

            // Extract location
            let location = event.location;

            events.push(CalendarEvent {
                summary,
                start_time,
                end_time,
                is_all_day,
                location,
            });
        }
    }

    Ok(events)
}

/// Parse event start and end times
fn parse_event_times(event: &Event) -> Result<(DateTime<Utc>, DateTime<Utc>, bool)> {
    let (start_time, is_all_day) = if let Some(ref start) = event.start {
        if let Some(ref date_time) = start.date_time {
            (*date_time, false)
        } else if let Some(ref naive_date) = start.date {
            let dt = Utc
                .with_ymd_and_hms(
                    naive_date.year(),
                    naive_date.month(),
                    naive_date.day(),
                    0,
                    0,
                    0,
                )
                .single()
                .context("Invalid start date")?;
            (dt, true)
        } else {
            anyhow::bail!("Event has no start time");
        }
    } else {
        anyhow::bail!("Event missing start field");
    };

    let end_time = if let Some(ref end) = event.end {
        if let Some(ref date_time_str) = end.date_time {
            *date_time_str
        } else if let Some(ref naive_date) = end.date {
            // All-day event
            let dt = Utc
                .with_ymd_and_hms(
                    naive_date.year(),
                    naive_date.month(),
                    naive_date.day(),
                    23,
                    59,
                    59,
                )
                .single()
                .context("Invalid end date")?;
            dt
        } else {
            anyhow::bail!("Event has no end time");
        }
    } else {
        anyhow::bail!("Event missing end field");
    };

    Ok((start_time, end_time, is_all_day))
}
