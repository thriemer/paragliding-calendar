use crate::models::{Location, ParaglidingSite};
use crate::paragliding::dhv::load_sites;
use crate::paragliding::site_evaluator::evaluate_site;
use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};
use google_calendar3::{
    CalendarHub,
    api::{Event, Events},
};
use http_body_util::combinators::BoxBody;
use hyper::body::Bytes;
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::Error;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use std::env;
use yup_oauth2::{ApplicationSecret, InstalledFlowAuthenticator, InstalledFlowReturnMethod};

mod models;
mod paragliding;
mod weather;

fn find_sites_within_radius(
    center: &Location,
    radius_km: f64,
    sites: &[ParaglidingSite],
) -> Vec<(ParaglidingSite, f64)> {
    let mut results = Vec::new();

    for site in sites {
        // Find the closest launch to the center point
        let mut min_distance = f64::INFINITY;

        for launch in &site.launches {
            let distance = center.distance_to(&launch.location);
            if distance < min_distance {
                min_distance = distance;
            }
        }

        // Include site if any launch is within radius
        if min_distance <= radius_km {
            results.push((site.clone(), min_distance));
        }
    }

    // Sort by distance (closest first)
    results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    results
}

fn flying_sites() {
    let location = weather::geocode("Gornau/Erz").unwrap();
    let _weather = weather::get_forecast(location[0].clone()).unwrap();

    let mut sites = load_sites("dhvgelaende_dhvxml_de.xml");
    sites.append(&mut load_sites("dhvgelaende_dhvxml_cz.xml"));

    // Search for sites within 50km of the location
    let search_center = &location[0];
    let radius_km = 150.0;
    let nearby_sites = find_sites_within_radius(search_center, radius_km, &sites);

    println!(
        "Found {} paragliding sites within {}km of {}:",
        nearby_sites.len(),
        radius_km,
        search_center.name
    );

    for (site, distance) in nearby_sites.iter() {
        println!(
            "  - {} ({:.1}km away) - {} launches",
            site.name,
            distance,
            site.launches.len()
        );

        // Get weather forecast for the site's first launch location
        if let Some(launch) = site.launches.first() {
            match weather::get_forecast(launch.location.clone()) {
                Ok(forecast) => {
                    let evaluation = evaluate_site(site, &forecast);

                    // Display results for the first two days
                    for (i, daily_summary) in evaluation.daily_summaries.iter().enumerate() {
                        println!(
                            "    {}: {}/100 - {} flyable hours",
                            daily_summary.date.weekday(),
                            daily_summary.overall_score,
                            daily_summary.total_flyable_hours
                        );
                    }
                }
                Err(_) => {
                    println!("    Weather unavailable");
                }
            }
        } else {
            println!("    No launch locations available");
        }
        println!();
    }
}

#[derive(Debug)]
pub struct CalendarEvent {
    pub id: String,
    pub summary: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub is_all_day: bool,
    pub location: Option<String>,
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

type CalendarHubType =
    CalendarHub<HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>>;

/// Create authenticated calendar client
async fn create_calendar_client() -> Result<CalendarHubType> {
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

    // Create Calendar Hub with the hyper_client and the authenticator
    let hub = CalendarHub::new(hyper_client, auth);

    Ok(hub)
}

/// Fetch events from primary calendar within time range
pub async fn fetch_calendar_events(
    hub: &CalendarHubType,
    time_min: DateTime<Utc>,
    time_max: DateTime<Utc>,
) -> Result<Vec<CalendarEvent>> {
    // Get events from primary calendar
    let result = hub
        .events()
        .list("primary")
        .time_min(time_min)
        .time_max(time_max)
        .time_min(chrono::Utc::now())
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
                id: event.id.context("Event missing ID")?,
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

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
    println!("🚀 Starting Google Calendar event fetcher...");

    // Create authenticated client
    let hub = create_calendar_client().await?;
    println!("✅ Successfully authenticated with Google Calendar");

    // Define time range (e.g., next 7 days)
    let now = Utc::now();
    let time_min = now;
    let time_max = now + Duration::days(7);

    println!("📅 Fetching events from {} to {}", time_min, time_max);

    // Fetch events
    let events = fetch_calendar_events(&hub, time_min, time_max).await?;

    println!("\n📋 Found {} event(s):", events.len());

    // Display events
    for (i, event) in events.iter().enumerate() {
        println!("\n{}. {}", i + 1, event.summary);
        println!("   📍 ID: {}", event.id);

        if event.is_all_day {
            println!("   📅 All-day event");
        } else {
            println!("   ⏰ {} - {}", event.start_time, event.end_time);
        }

        if let Some(location) = &event.location {
            println!("   🗺️ Location: {}", location);
        }
    }

    // Get an ApplicationSecret instance by some means. It contains the `client_id` and
    // `client_secret`, among other things.
    let secret: yup_oauth2::ApplicationSecret = Default::default();
    // Instantiate the authenticator. It will choose a suitable authentication flow for you,
    // unless you replace  `None` with the desired Flow.
    // Provide your own `AuthenticatorDelegate` to adjust the way it operates and get feedback about
    // what's going on. You probably want to bring in your own `TokenStorage` to persist tokens and
    // retrieve them from storage.
    let connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .unwrap()
        .https_only()
        .enable_http2()
        .build();

    let executor = hyper_util::rt::TokioExecutor::new();
    let auth = yup_oauth2::InstalledFlowAuthenticator::with_client(
        secret,
        yup_oauth2::InstalledFlowReturnMethod::HTTPRedirect,
        yup_oauth2::client::CustomHyperClientBuilder::from(
            hyper_util::client::legacy::Client::builder(executor).build(connector),
        ),
    )
    .build()
    .await
    .unwrap();

    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(
            hyper_rustls::HttpsConnectorBuilder::new()
                .with_native_roots()
                .unwrap()
                .https_or_http()
                .enable_http2()
                .build(),
        );
    let mut hub = CalendarHub::new(client, auth);

    Ok(())
}
