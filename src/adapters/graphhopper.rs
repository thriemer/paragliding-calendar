use std::{env, sync::Arc, time::Duration as StdDuration};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::Duration;
use rand::RngExt;
use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;
use tracing::instrument;

use crate::{
    adapters::cache::PersistentCache,
    domain::{location::Location, ports::RoutingProvider},
};

pub struct Routing {
    cache: Arc<PersistentCache>,
    http: ClientWithMiddleware,
}

impl Routing {
    pub fn new(cache: Arc<PersistentCache>, http: ClientWithMiddleware) -> Self {
        Self { cache, http }
    }

    async fn get_travel_time_call(
        &self,
        source: &Location,
        destination: &Location,
    ) -> Result<u64> {
        tracing::debug!("Calling the API");
        let url = format!(
            "https://graphhopper.com/api/1/route?point={},{}&point={},{}&profile=car&points_encoded=false&calc_points=false&key={}",
            source.latitude,
            source.longitude,
            destination.latitude,
            destination.longitude,
            env::var("GRAPHHOPPER_API_KEY").context("Missing GRAPHHOPPER_API_KEY env var")?
        );
        let response = self.http.get(url).send().await?;
        let response: ApiResponse = response.json().await?;

        response
            .paths
            .get(0)
            .map(|path| path.time / 1000)
            .ok_or(anyhow!("No paths in response"))
    }
}

#[async_trait]
impl RoutingProvider for Routing {
    #[instrument(skip(self))]
    async fn get_travel_time(
        &self,
        source: &Location,
        destination: &Location,
    ) -> Result<Duration> {
        let key = source.to_key() + "-" + &destination.to_key();

        if let Some(cached) = self.cache.get::<u64>(&key).await? {
            return Ok(Duration::seconds(cached as i64));
        }

        let seconds = self.get_travel_time_call(source, destination).await?;

        let jitter: f32 = rand::rng().random_range(0.9..1.1);
        self.cache
            .put(
                &key,
                seconds,
                StdDuration::from_hours((24f32 * 7f32 * jitter) as u64),
            )
            .await?;
        Ok(Duration::seconds(seconds as i64))
    }
}

#[derive(Debug, Deserialize)]
struct PathResponse {
    time: u64,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    paths: Vec<PathResponse>,
}
