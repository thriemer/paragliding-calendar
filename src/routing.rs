use std::env;

use anyhow::{Context, Result, anyhow};
use cached::proc_macro::cached;
use serde::Deserialize;
use tracing::instrument;

use crate::{API_CLIENT, location::Location};

#[cached(
    time = 604800,
    result,
    key = "String",
    convert = r#"{ format!("{}-{}", source.to_key(), destination.to_key()) }"#
)]
async fn get_travel_time_cached(source: Location, destination: Location) -> Result<u64> {
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

#[instrument]
pub async fn get_travel_time(source: &Location, destination: &Location) -> Result<u64> {
    get_travel_time_cached(source.clone(), destination.clone()).await
}

#[derive(Debug, Deserialize)]
struct PathResponse {
    time: u64,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    paths: Vec<PathResponse>,
}
