use std::env;

use anyhow::Result;
use tokio::time;

use crate::{
    app_state::AppState,
    calendar::{CalendarProvider, google::GoogleCalendar},
    location::Location,
    paragliding::repository::UserSettings,
};

mod api;
mod app_state;
mod application;
mod cache;
mod calendar;
mod config;
mod email;
mod location;
mod paragliding;
mod routing;
mod store;
mod telemetry;
mod weather;
mod web;

async fn create_calender_entries(state: &AppState) -> Result<()> {
    let settings = match state.site_repo.get_settings().await? {
        Some(s) => s,
        None => {
            tracing::warn!("No settings found, using defaults");
            UserSettings::default()
        }
    };

    let location = Location::new(
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

    let events = state
        .service
        .create_events_for_location(state.site_repo.as_ref(), &location, &mut cal, &settings)
        .await?;

    if let Err(e) = cal.clear_calendar(&settings.calendar_name).await {
        tracing::error!("Failed to clear calendar {}: {}", settings.calendar_name, e);
        return Err(e);
    }

    let mut event_counter = 0;
    for event in events {
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

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init_telemetry()?;

    tracing::info!("Starting travelai application");

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let db_path = env::var("XDG_CACHE_HOME")
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
