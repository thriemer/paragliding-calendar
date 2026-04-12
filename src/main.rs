use std::{env, sync::LazyLock};

use anyhow::Result;
use futures::StreamExt;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use tokio::time;
use tracing::instrument;

use crate::{
    application::ParaglidingCalendarService,
    calendar::{CalendarProvider, google::GoogleCalendar},
    location::Location,
    paragliding::{
        ParaglidingSite, ParaglidingSiteProvider,
        cache::{CachedParaglidingSiteProvider, UserSettings},
        site_evaluator::SiteEvaluationResult,
    },
};

mod api;
mod application;
mod cache;
mod calendar;
mod config;
mod email;
mod location;
mod paragliding;
mod routing;
mod telemetry;
mod weather;
mod web;

static API_CLIENT: LazyLock<ClientWithMiddleware> = LazyLock::new(|| {
    let retry_policy = ExponentialBackoff::builder()
        .base(3)
        .retry_bounds(
            std::time::Duration::from_secs(10),
            std::time::Duration::from_mins(30),
        )
        .build_with_max_retries(5);
    let client = ClientBuilder::new(reqwest::Client::new())
        .with(RetryTransientMiddleware::new_with_policy(retry_policy))
        .build();
    client
});

// Create calendar entries for paragliding based on settings from cache
async fn create_calender_entries() -> Result<()> {
    let settings = match CachedParaglidingSiteProvider::get_settings().await? {
        Some(s) => s,
        None => {
            tracing::warn!("No settings found in cache, using defaults");
            UserSettings::default()
        }
    };

    let location = Location::new(
        settings.location_latitude,
        settings.location_longitude,
        settings.location_name.clone(),
        "".to_string(),
    );

    let mut cal = match GoogleCalendar::new().await {
        Ok(cal) => cal,
        Err(e) => {
            tracing::error!("Failed to create Google Calendar: {}", e);
            return Err(e);
        }
    };

    let provider = CachedParaglidingSiteProvider::new();
    let service = ParaglidingCalendarService::new();

    let events = service
        .create_events_for_location(
            &provider,
            &location,
            &mut cal,
            &settings,
        )
        .await?;

    // Clear and recreate all events
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
    // Initialize telemetry (OTLP to Alloy in production, stdout in development)
    telemetry::init_telemetry()?;

    tracing::info!("Starting travelai application");

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    cache::init(
        env::var("XDG_CACHE_HOME")
            .ok()
            .or(env::var("CACHE_DIRECTORY").ok())
            .expect("Cache environment variable not set."),
    )?;

    tokio::join!(async { web::run().await }, async {
        let mut interval = time::interval(time::Duration::from_hours(8));
        loop {
            interval.tick().await;
            if let Err(e) = create_calender_entries().await {
                tracing::error!("Failed to create calendar entries: {:?}", e);
            }
        }
    });
    Ok(())
}
