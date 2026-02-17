use crate::calender::google_backend::GoogleCalendar;
use crate::calender::{CalendarEvent, CalendarProvider};
use crate::location::Location;
use crate::paragliding::site_evaluator::{HourlyScore, SiteEvaluationResult, evaluate_site};
use crate::paragliding::{ParaglidingSite, ParaglidingSiteProvider, dhv};
use anyhow::Result;
use chrono::{Duration, Utc};
use futures::{StreamExt, future, stream};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use std::sync::LazyLock;
use tracing::{Instrument, instrument};
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod cache;
mod calender;
mod location;
mod paragliding;
mod routing;
mod weather;

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

#[instrument()]
async fn flying_sites_with_weather(
    location: &Location,
) -> Vec<(SiteEvaluationResult, ParaglidingSite)> {
    let provider = dhv::DhvParaglidingSiteProvider::new("dhv_sites".into()).unwrap();
    // Search for sites within 50km of the location
    let radius_km = 150.0;
    let nearby_sites = provider
        .fetch_launches_within_radius(location, radius_km)
        .await;
    tracing::info!(
        "Found {} nearby sites. Fetching weather for all of them",
        nearby_sites.len()
    );

    let mut result = vec![];

    for (site, distance) in nearby_sites.iter() {
        // Get weather forecast for the site's first launch location
        if let Some(launch) = site.launches.first() {
            if let Ok(forecast) = weather::open_meteo::get_forecast(launch.location.clone()).await {
                let evaluation = evaluate_site(site, &forecast);
                result.push((evaluation, site.clone()));
            }
        }
    }
    result
}

async fn create_calender_entries() -> Result<()> {
    let location = weather::open_meteo::geocode("Gornau/Erz").await.unwrap()[0].clone();
    let calender_name = "Paragliding";

    let mut cal = GoogleCalendar::new().await?;
    cal.create_calendar(calender_name).await?;

    let mut others_name = cal.get_calendar_names().await?;
    others_name.retain(|n| n != calender_name);
    tracing::info!("Found calendars {:?}", others_name);

    let sites = flying_sites_with_weather(&location).await;

    cal.clear_calendar(calender_name).await?;
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
                        let recommended = h.score > 50
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
                            summary: format!("{} - {}", site.name, r.avg_score),
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
        "Created {} events in calendar {}",
        event_counter,
        calender_name
    );

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    cache::init("./cache")?;
    let span = tracing::info_span!("Creating calendar entries");
    create_calender_entries().instrument(span).await?;
    Ok(())
}
