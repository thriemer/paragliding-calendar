use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use futures::future;

use crate::{
    adapters::{
        google_calendar::GoogleCalendar, microsoft_calendar::MicrosoftCalendar,
    },
    app_state::AppState,
    domain::{
        activities::{ActivitySuggestion, PlanningContext, TimeWindow, Timing},
        calendar::CalendarEvent,
        location::Location,
        paragliding::UserSettings,
        ports::CalendarProvider,
    },
};

#[tracing::instrument(skip_all, fields(event_count = tracing::field::Empty))]
pub async fn run(state: &AppState) -> Result<()> {
    let settings = match state.site_repo.get_settings().await? {
        Some(s) => s,
        None => {
            tracing::warn!("No settings found, using defaults");
            UserSettings::default()
        }
    };

    let home = Location::new(
        settings.location_latitude,
        settings.location_longitude,
        settings.location_name.clone(),
        "".to_string(),
    );

    let google = match GoogleCalendar::new(state.auth.clone(), state.cache.clone()).await {
        Ok(cal) => cal,
        Err(e) => {
            tracing::error!(error = ?e, "Failed to create Google Calendar");
            return Err(e);
        }
    };

    let microsoft = state
        .microsoft_auth
        .as_ref()
        .map(|auth| MicrosoftCalendar::new(auth.clone(), state.cache.clone()));

    let mut cal = CombinedCalendar { google, microsoft };

    cal.create_calendar(&settings.calendar_name).await?;

    let mut conflict_calendars = cal.get_calendar_names().await?;
    conflict_calendars.retain(|n| !settings.excluded_calendar_names.contains(n));

    let now = Utc::now();
    let ctx = PlanningContext {
        home,
        horizon: TimeWindow {
            start: now,
            end: now + Duration::days(14),
        },
        conflict_calendars,
    };

    let suggestions = state.planner.plan(&ctx, &cal).await?;

    if let Err(e) = cal.clear_calendar(&settings.calendar_name).await {
        tracing::error!(
            calendar = %settings.calendar_name,
            error = ?e,
            "Failed to clear calendar"
        );
        return Err(e);
    }

    let mut event_counter = 0;
    for s in suggestions {
        let event = suggestion_to_event(s);
        if let Err(e) = cal.create_event(&settings.calendar_name, event).await {
            tracing::error!(error = ?e, "Failed to create event");
            return Err(e);
        }
        event_counter += 1;
    }

    tracing::Span::current().record("event_count", event_counter);
    tracing::info!(
        event_count = event_counter,
        calendar = %settings.calendar_name,
        "Created events in calendar"
    );

    Ok(())
}

struct CombinedCalendar {
    google: GoogleCalendar,
    microsoft: Option<MicrosoftCalendar>,
}

#[async_trait]
impl CalendarProvider for CombinedCalendar {
    async fn is_busy(
        &self,
        calendars: &Vec<String>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<bool> {
        let google_fut = self.google.is_busy(calendars, start, end);
        let microsoft_fut = async {
            if let Some(ms) = self.microsoft.as_ref() {
                match ms.is_busy(calendars, start, end).await {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!(error = ?e, "Microsoft is_busy failed; treating slot as free");
                        false
                    }
                }
            } else {
                false
            }
        };
        let (google_busy, microsoft_busy) = future::join(google_fut, microsoft_fut).await;
        Ok(google_busy? || microsoft_busy)
    }

    async fn get_calendar_names(&self) -> Result<Vec<String>> {
        self.google.get_calendar_names().await
    }

    async fn clear_calendar(&mut self, name: &str) -> Result<()> {
        self.google.clear_calendar(name).await
    }

    async fn create_event(&mut self, calendar: &str, event: CalendarEvent) -> Result<()> {
        self.google.create_event(calendar, event).await
    }

    async fn create_calendar(&mut self, name: &str) -> Result<()> {
        self.google.create_calendar(name).await
    }
}

fn suggestion_to_event(s: ActivitySuggestion) -> CalendarEvent {
    let (start, end) = match s.timing {
        Timing::Flexible { window, .. } => (window.start, window.end),
        Timing::Fixed { start, end } => (start, end),
    };
    CalendarEvent {
        title: s.title.clone(),
        start_time: start,
        end_time: end,
        is_all_day: false,
        location: Some(s.title),
        body: Some(format!("Last updated (Utc): {}", Utc::now())),
    }
}
