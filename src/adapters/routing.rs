use std::{env, sync::Arc, time::Duration as StdDuration};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::Duration;
use rand::RngExt;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use tracing::instrument;

use crate::{
    adapters::persistence::cache::PersistentCache,
    domain::{location::Location, ports::RoutingProvider},
};

const MAX_RETRIES: u32 = 3;

pub struct Graphhopper {
    cache: Arc<PersistentCache>,
    http: Client,
}

impl Graphhopper {
    pub fn new(cache: Arc<PersistentCache>, http: Client) -> Self {
        Self { cache, http }
    }

    async fn get_travel_time_call(&self, source: &Location, destination: &Location) -> Result<u64> {
        tracing::debug!("Calling the API");
        let url = format!(
            "https://graphhopper.com/api/1/route?point={},{}&point={},{}&profile=car&points_encoded=false&calc_points=false&key={}",
            source.latitude,
            source.longitude,
            destination.latitude,
            destination.longitude,
            env::var("GRAPHHOPPER_API_KEY").context("Missing GRAPHHOPPER_API_KEY env var")?
        );

        let mut last_error: Option<anyhow::Error> = None;
        for attempt in 0..MAX_RETRIES {
            let result = self.http.get(&url).send().await;
            match result {
                Ok(response) => {
                    let status = response.status();
                    if status == StatusCode::TOO_MANY_REQUESTS {
                        let headers = response.headers().clone();
                        let body = response.text().await.unwrap_or_default();
                        if body.contains("Minutely") {
                            let wait =
                                parse_retry_after(&headers).unwrap_or(StdDuration::from_secs(60));
                            tracing::warn!(
                                attempt,
                                wait_ms = wait.as_millis(),
                                "GraphHopper rate limited, retrying after Retry-After"
                            );
                            tokio::time::sleep(wait).await;
                            last_error = Some(anyhow!("GraphHopper rate limited: {body}"));
                            continue;
                        }
                        return Err(anyhow!("GraphHopper daily quota exhausted: {body}"));
                    }
                    if !status.is_success() {
                        let body = response.text().await.unwrap_or_default();
                        return Err(anyhow!("GraphHopper returned {}: {}", status, body));
                    }
                    let parsed: ApiResponse = response.json().await?;
                    return parsed
                        .paths
                        .first()
                        .map(|path| path.time / 1000)
                        .ok_or(anyhow!("No paths in response"));
                }
                Err(err) => {
                    tracing::warn!(attempt, error = ?err, "GraphHopper request failed");
                    last_error = Some(err.into());
                    tokio::time::sleep(StdDuration::from_secs(2u64.saturating_pow(attempt + 1)))
                        .await;
                }
            }
        }

        Err(last_error.unwrap_or(anyhow!(
            "GraphHopper request failed after {MAX_RETRIES} retries"
        )))
    }
}

fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<StdDuration> {
    let value = headers.get("Retry-After")?.to_str().ok()?;
    if let Ok(secs) = value.parse::<u64>() {
        return Some(StdDuration::from_secs(secs));
    }
    if let Ok(when) = chrono::DateTime::parse_from_rfc2822(value) {
        let now = chrono::Utc::now();
        let target = when.with_timezone(&chrono::Utc);
        let secs = (target - now).num_seconds().max(0) as u64;
        return Some(StdDuration::from_secs(secs));
    }
    None
}

#[async_trait]
impl RoutingProvider for Graphhopper {
    #[instrument(skip(self))]
    async fn get_travel_time(&self, source: &Location, destination: &Location) -> Result<Duration> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_route_time_seconds() {
        // Trimmed /route response: `time` is milliseconds; the adapter divides to seconds.
        let body = r#"{ "paths": [{ "time": 600000 }] }"#;
        let parsed: ApiResponse = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.paths[0].time / 1000, 600);
    }
}
