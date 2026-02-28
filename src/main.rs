use std::sync::LazyLock;

use anyhow::Result;
use chrono::Duration;
use futures::{StreamExt, future};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use tokio::time;
use tracing::instrument;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    calender::{CalendarEvent, CalendarProvider, google::GoogleCalendar},
    location::Location,
    paragliding::{
        ParaglidingSite, ParaglidingSiteProvider, dhv,
        site_evaluator::{HourlyScore, SiteEvaluationResult, evaluate_site},
    },
};

mod api;
mod auth;
mod cache;
mod calender;
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

#[instrument(skip_all, fields(location = %location.name))]
async fn flying_sites_with_weather(
    location: &Location,
) -> Vec<(SiteEvaluationResult, ParaglidingSite)> {
    let radius_km = 150.0;
    let provider = dhv::DhvParaglidingSiteProvider::new("dhv_sites".into()).unwrap();
    let nearby_sites = provider
        .fetch_launches_within_radius(location, radius_km)
        .await;
    tracing::info!(
        location = %location.name,
        count = nearby_sites.len(),
        "Found nearby sites, fetching weather"
    );

    let mut result = vec![];

    for (site, _distance) in nearby_sites.iter() {
        if let Some(launch) = site.launches.first() {
            match weather::open_meteo::get_forecast(launch.location.clone()).await {
                Ok(forecast) => {
                    let evaluation = evaluate_site(site, &forecast).await;
                    result.push((evaluation, site.clone()));
                }
                Err(e) => {
                    tracing::warn!(
                        site = %site.name,
                        lat = %launch.location.latitude,
                        lon = %launch.location.longitude,
                        error = %e,
                        "Failed to get weather forecast"
                    );
                }
            }
        }
    }
    result
}

#[instrument()]
async fn create_calender_entries() -> Result<()> {
    let location = match weather::open_meteo::geocode("Gornau/Erz").await {
        Ok(loc) => loc,
        Err(e) => {
            tracing::error!("Failed to geocode location: {}", e);
            return Err(e.into());
        }
    };
    let location = location.into_iter().next().expect("No location found");
    let calender_name = "Paragliding";

    let mut cal = match GoogleCalendar::new().await {
        Ok(cal) => cal,
        Err(e) => {
            tracing::error!("Failed to create Google Calendar: {}", e);
            return Err(e);
        }
    };
    if let Err(e) = cal.create_calendar(calender_name).await {
        tracing::error!("Failed to create calendar {}: {}", calender_name, e);
        return Err(e);
    }

    let mut others_name = match cal.get_calendar_names().await {
        Ok(names) => names,
        Err(e) => {
            tracing::error!("Failed to get calendar names: {}", e);
            return Err(e);
        }
    };
    others_name.retain(|n| n != calender_name);
    tracing::info!(calendars = ?others_name, "Found calendars");

    let sites = flying_sites_with_weather(&location).await;

    if let Err(e) = cal.clear_calendar(calender_name).await {
        tracing::error!("Failed to clear calendar {}: {}", calender_name, e);
        return Err(e);
    }
    let mut event_counter = 0;

    for (eval, site) in sites {
        let drive_to_site = Duration::seconds(
            routing::get_travel_time(&location, &site.launches[0].location).await? as i64,
        );
        for mut e in eval.daily_summaries {
            let mut scores: Vec<(HourlyScore, bool)> = future::join_all(
                e.hourly_scores
                    .iter()
                    .map(async |h| {
                        let h = h.clone();
                        let recommended = h.is_flyable
                            && !cal
                                .is_busy(
                                    &others_name,
                                    h.timestamp - Duration::minutes(30),
                                    h.timestamp + Duration::minutes(30),
                                )
                                .await
                                .unwrap();
                        (h, recommended)
                    })
                    .collect::<Vec<_>>(),
            )
            .await;

            scores.retain(|(_, recommendation)| *recommendation);
            e.hourly_scores = scores.into_iter().map(|(score, _)| score).collect();

            e.calculate_flyable_time_ranges();
            for mut r in e.ranges {
                //Assumption that driving time is symmetrical
                r.start += drive_to_site;
                r.end -= drive_to_site;
                if r.is_longer_than(drive_to_site + drive_to_site) {
                    event_counter += 1;
                    cal.create_event(
                        calender_name,
                        CalendarEvent {
                            summary: site.name.clone(),
                            start_time: r.start,
                            end_time: r.end,
                            is_all_day: false,
                            location: None,
                        },
                    )
                    .await?;
                }
            }
        }
    }

    tracing::info!(
        event_count = event_counter,
        calendar = %calender_name,
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

    cache::init("./cache")?;

    tokio::join!(async { web::run(443).await }, async {
        let mut interval = time::interval(time::Duration::from_secs(86400));
        loop {
            interval.tick().await;
            if let Err(e) = create_calender_entries().await {
                tracing::error!("Failed to create calendar entries: {}", e);
            }
        }
    });
    Ok(())
}
