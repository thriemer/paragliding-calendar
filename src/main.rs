use std::{env, sync::LazyLock};

use anyhow::Result;
use futures::StreamExt;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use tokio::time;
use tracing::instrument;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    application::ParaglidingCalendarService,
    calendar::{CalendarProvider, google::GoogleCalendar},
    database::{Database, Db},
    email::GmailEmailProvider,
    location::Location,
    paragliding::{
        ParaglidingSite, ParaglidingSiteProvider,
        database::{CachedParaglidingSiteProvider, UserSettings},
        site_evaluator::SiteEvaluationResult,
    },
    routing::GraphHopperRoutingProvider,
    weather::open_meteo::OpenMeteoWeatherProvider,
};

mod api;
mod application;
mod calendar;
mod config;
mod database;
mod email;
mod location;
mod paragliding;
mod routing;
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

// Create calendar entries for paragliding based on settings from database
async fn create_calender_entries(db: Db) -> Result<()> {
    let settings = match CachedParaglidingSiteProvider::new(db.clone())
        .get_settings()
        .await?
    {
        Some(s) => s,
        None => {
            tracing::warn!("No settings found in database, using defaults");
            UserSettings::default()
        }
    };

    let location = Location::new(
        settings.location_latitude,
        settings.location_longitude,
        settings.location_name.clone(),
        "".to_string(),
    );

    let email_provider = GmailEmailProvider::new().expect("Failed to create email provider");
    let mut cal = match GoogleCalendar::new(db.clone(), email_provider).await {
        Ok(cal) => cal,
        Err(e) => {
            tracing::error!("Failed to create Google Calendar: {}", e);
            return Err(e);
        }
    };

    let provider = CachedParaglidingSiteProvider::new(db.clone());
    let weather_provider = OpenMeteoWeatherProvider::new();
    let routing_provider = GraphHopperRoutingProvider::new();
    let service = ParaglidingCalendarService::new(db.clone());
    let config = crate::application::CalendarConfig {
        search_radius_km: settings.search_radius_km,
        minimum_flyable_duration: chrono::Duration::hours(settings.minimum_flyable_hours as i64),
    };

    let events = service
        .create_events_for_location(
            &provider,
            &weather_provider,
            &routing_provider,
            &location,
            &mut cal,
            &settings.calendar_name,
            config,
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
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tracing::info!("Starting travelai application");

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let db = std::sync::Arc::new(Database::new(
        env::var("XDG_DATA_HOME")
            .ok()
            .or(env::var("DATA_DIRECTORY").ok())
            .expect("Data environment variable not set."),
    )?);

    tokio::join!(async { web::run(db.clone()).await }, async {
        let db = db.clone();
        let mut interval = time::interval(time::Duration::from_hours(24));
        loop {
            interval.tick().await;
            if let Err(e) = create_calender_entries(db.clone()).await {
                tracing::error!("Failed to create calendar entries: {}", e);
            }
        }
    });
    Ok(())
}
