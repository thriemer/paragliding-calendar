use std::{env, time::Duration};

use anyhow::{Context, Result, anyhow};
use cached::proc_macro::cached;
use chrono::{DateTime, Datelike, NaiveTime, Utc};
use google_calendar3::{
    CalendarHub,
    api::{CalendarList, FreeBusyRequestItem, Scope},
};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use std::hash::{DefaultHasher, Hash, Hasher};
use tracing::instrument;

use crate::calendar::{
    CalendarEvent, CalendarProvider, web_flow_authenticator::WebFlowAuthenticator,
};

pub type CalendarHubType =
    CalendarHub<HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>>;

pub struct GoogleCalendar {
    hub: CalendarHubType,
}

#[cached(time = 259200, result, key = "String", convert = r#"{ name.clone() }"#)]
async fn get_calendar_id_for_name(hub: CalendarHubType, name: String) -> Result<String> {
    let (_, lists) = hub
        .calendar_list()
        .list()
        .add_scope(Scope::CalendarlistReadonly)
        .doit()
        .await?;

    let lists = lists.items.ok_or(anyhow!("Empty calendar list"))?;
    let result = lists
        .iter()
        .filter(|l| {
            if let Some(desc) = &l.summary {
                desc == &name
            } else {
                false
            }
        })
        .map(|l| l.id.clone().unwrap())
        .collect::<Vec<String>>()
        .first()
        .cloned();

    result.ok_or_else(|| anyhow!("Calendar id not found for name {}", name))
}

#[cached(
    time = 28800,
    result,
    key = "String",
    convert = r#"{ cache_key.clone() }"#
)]
async fn get_free_busy_by_key(
    _hub: CalendarHubType,
    _calendars: Vec<String>,
    _week_start: DateTime<Utc>,
    _week_end: DateTime<Utc>,
    cache_key: String,
) -> Result<google_calendar3::api::FreeBusyResponse> {
    Err(anyhow!("Should not be called directly"))
}

async fn do_get_free_busy(
    hub: &CalendarHubType,
    calendars: &[String],
    week_start: DateTime<Utc>,
    week_end: DateTime<Utc>,
) -> Result<google_calendar3::api::FreeBusyResponse> {
    let calendars_vec = calendars.to_vec();

    let items: Vec<FreeBusyRequestItem> = futures::future::join_all(
        calendars_vec
            .iter()
            .map(|n| {
                let name = n.clone();
                async move {
                    let id = match get_calendar_id_for_name(hub.clone(), name).await {
                        Ok(id) => Some(id),
                        Err(err) => {
                            tracing::warn!("Cant get id for name {}. Error {:?}", n, err);
                            None
                        }
                    };
                    FreeBusyRequestItem { id }
                }
            })
            .collect::<Vec<_>>(),
    )
    .await;

    let response = hub
        .freebusy()
        .query(google_calendar3::api::FreeBusyRequest {
            items: Some(items),
            time_min: Some(week_start),
            time_max: Some(week_end),
            group_expansion_max: None,
            calendar_expansion_max: None,
            time_zone: None,
        })
        .add_scope(Scope::Freebusy)
        .doit()
        .await?
        .1;

    Ok(response)
}

impl GoogleCalendar {
    pub async fn new(
        db: crate::database::Db,
        email_provider: crate::email::GmailEmailProvider,
    ) -> Result<Self> {
        let connector = HttpsConnectorBuilder::new()
            .with_native_roots()
            .context("Failed to build HTTPS connector")?
            .https_only()
            .enable_http2()
            .build();

        let hyper_client = Client::builder(TokioExecutor::new()).build(connector);
        let auth = WebFlowAuthenticator::new(
            env::var("GOOGLE_CLIENT_ID").expect("Missing GOOGLE_CLIENT_ID"),
            env::var("GOOGLE_CLIENT_SECRET").expect("Missing GOOGLE_CLIENT_SECRET"),
            env::var("OAUTH_REDIRECT_URL").unwrap_or_else(|_| {
                "https://linus-x1.bangus-firefighter.ts.net/oauth/callback".to_string()
            }),
            db.clone(),
            email_provider,
        );
        let hub = CalendarHub::new(hyper_client, auth);
        Ok(GoogleCalendar { hub })
    }

    async fn get_id_for_name(&self, name: &str) -> Result<String> {
        get_calendar_id_for_name(self.hub.clone(), name.to_string()).await
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
        let cache_key = format!("freebusy_{}", hasher.finish());

        let busy = get_free_busy_by_key(
            self.hub.clone(),
            calendars.clone(),
            week_start_datetime,
            week_end_datetime,
            cache_key,
        )
        .await;

        let busy = match busy {
            Ok(b) => b,
            Err(_) => {
                do_get_free_busy(&self.hub, calendars, week_start_datetime, week_end_datetime)
                    .await?
            }
        };

        let items: Vec<FreeBusyRequestItem> = futures::future::join_all(
            calendars
                .iter()
                .map(|n| {
                    let name = n.clone();
                    async move {
                        let id = match self.get_id_for_name(&name).await {
                            Ok(id) => Some(id),
                            Err(err) => {
                                tracing::warn!("Cant get id for name {}. Error {:?}", n, err);
                                None
                            }
                        };
                        FreeBusyRequestItem { id }
                    }
                })
                .collect::<Vec<_>>(),
        )
        .await;

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
        let (_, _) = self
            .hub
            .calendars()
            .insert(cal)
            .add_scope(Scope::AppCreated)
            .doit()
            .await?;
        Ok(())
    }
}
