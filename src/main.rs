use std::env;

use anyhow::Result;
use chrono::{Duration, Utc};
use tokio::time;

use crate::{
    adapters::google_calendar::GoogleCalendar,
    app_state::AppState,
    domain::{
        activities::{ActivitySuggestion, PlanningContext, TimeWindow, Timing},
        calendar::CalendarEvent,
        location::Location,
        paragliding::UserSettings,
        ports::CalendarProvider,
    },
};

mod adapters;
mod app_state;
mod application;
mod config;
mod domain;
mod telemetry;
mod web;

async fn create_calender_entries(state: &AppState) -> Result<()> {
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

    let mut cal = match GoogleCalendar::new(state.auth.clone(), state.cache.clone()).await {
        Ok(cal) => cal,
        Err(e) => {
            tracing::error!("Failed to create Google Calendar: {}", e);
            return Err(e);
        }
    };

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
        tracing::error!("Failed to clear calendar {}: {}", settings.calendar_name, e);
        return Err(e);
    }

    let mut event_counter = 0;
    for s in suggestions {
        let event = suggestion_to_event(s);
        if let Err(e) = cal.create_event(&settings.calendar_name, event).await {
            tracing::error!("Failed to create event: {}", e);
            return Err(e);
        }
        event_counter += 1;
    }

    tracing::info!(
        event_count = event_counter,
        calendar = %settings.calendar_name,
        "Created events in calendar"
    );

    Ok(())
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

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init_telemetry()?;

    tracing::info!("Starting travelai application");

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let db_path = env::var("XDG_DATA_HOME")
        .ok()
        .or(env::var("CACHE_DIRECTORY").ok())
        .expect("Cache environment variable not set.");
    let db = fjall::Database::builder(&db_path).open()?;
    let state = AppState::new(&db)?;

    let job_state = state.clone();
    tokio::join!(
        async { web::run(state).await },
        async move {
            let mut interval = time::interval(time::Duration::from_hours(8));
            loop {
                interval.tick().await;
                if let Err(e) = create_calender_entries(&job_state).await {
                    tracing::error!("Failed to create calendar entries: {:?}", e);
                }
            }
        }
    );
    Ok(())
}
