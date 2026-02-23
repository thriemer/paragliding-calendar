use std::{env, time::Duration};

use anyhow::{Context, Ok, Result, anyhow};
use rand::RngExt;
use serde::Deserialize;
use tracing::instrument;

use crate::{API_CLIENT, cache, location::Location};

#[instrument()]
pub async fn get_travel_time(source: &Location, destination: &Location) -> Result<u64> {
    let key = source.to_key() + "-" + &destination.to_key();

    if let Some(cached) = cache::get::<u64>(&key).await? {
        return Ok(cached);
    }

    let seconds = get_travel_time_call(source, destination).await?;

    let jitter: f32 = rand::rng().random_range(0.9..1.1);
    cache::put(
        &key,
        seconds,
        Duration::from_hours((24f32 * 7f32 * jitter) as u64),
    )
    .await?;
    Ok(seconds)
}

async fn get_travel_time_call(source: &Location, destination: &Location) -> Result<u64> {
    tracing::debug!("Calling the API");
    let url = format!(
        "https://graphhopper.com/api/1/route?point={},{}&point={},{}&profile=car&points_encoded=false&calc_points=false&key={}",
        source.latitude,
        source.longitude,
        destination.latitude,
        destination.longitude,
        env::var("GRAPHHOPPER_API_KEY").context("Missing GRAPHHOPPER_API_KEY env var")?
    );
    let response = API_CLIENT.get(url).send().await?;
    let response: ApiResponse = response.json().await?;

    response
        .paths
        .get(0)
        .map(|path| path.time / 1000)
        .ok_or(anyhow!("No paths in response"))
}

#[derive(Debug, Deserialize)]
struct PathResponse {
    time: u64,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    paths: Vec<PathResponse>,
}
