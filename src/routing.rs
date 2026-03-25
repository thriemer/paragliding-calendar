use anyhow::{Context, Result, anyhow};
use cached::proc_macro::cached;
use serde::Deserialize;

use crate::{API_CLIENT, location::Location};

pub trait RoutingProvider: Send + Sync {
    async fn get_travel_time(&self, source: &Location, destination: &Location) -> Result<u64>;
}

#[derive(Debug, Deserialize)]
struct PathResponse {
    time: u64,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    paths: Vec<PathResponse>,
}

#[cached(
    time = 604800,
    result,
    key = "String",
    convert = r#"{ format!("{}-{}", source.to_key(), destination.to_key()) }"#
)]
async fn fetch_travel_time(source: Location, destination: Location) -> Result<u64> {
    tracing::debug!("Calling the GraphHopper API");
    let url = format!(
        "https://graphhopper.com/api/1/route?point={},{}&point={},{}&profile=car&points_encoded=false&calc_points=false&key={}",
        source.latitude,
        source.longitude,
        destination.latitude,
        destination.longitude,
        std::env::var("GRAPHHOPPER_API_KEY").context("Missing GRAPHHOPPER_API_KEY env var")?
    );
    let response = API_CLIENT.get(url).send().await?;
    let response: ApiResponse = response.json().await?;

    response
        .paths
        .get(0)
        .map(|path| path.time / 1000)
        .ok_or(anyhow!("No paths in response"))
}

#[derive(Clone, Default)]
pub struct GraphHopperRoutingProvider;

impl GraphHopperRoutingProvider {
    pub fn new() -> Self {
        Self
    }
}

impl RoutingProvider for GraphHopperRoutingProvider {
    async fn get_travel_time(&self, source: &Location, destination: &Location) -> Result<u64> {
        fetch_travel_time(source.clone(), destination.clone()).await
    }
}
