use std::{sync::Arc, time::Duration as StdDuration};

use anyhow::{Result, anyhow};
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

/// Extra attempts (beyond the exact-coordinate first try) with jittered waypoints.
const MAX_SNAP_RETRIES: usize = 3;

pub struct BRouter {
    base_url: String,
    cache: Arc<PersistentCache>,
    http: ClientWithMiddleware,
}

impl BRouter {
    pub fn new(
        base_url: String,
        cache: Arc<PersistentCache>,
        http: ClientWithMiddleware,
    ) -> Self {
        Self {
            base_url,
            cache,
            http,
        }
    }

    async fn get_travel_time_call(
        &self,
        source: &Location,
        destination: &Location,
    ) -> Result<u64> {
        let url = format!(
            "{}/brouter?lonlats={},{}|{},{}&profile=car-vario&alternativeidx=0&format=geojson",
            self.base_url.trim_end_matches('/'),
            source.longitude,
            source.latitude,
            destination.longitude,
            destination.latitude,
        );
        tracing::info!(url = %url, "Calling the BRouter API");

        let response = self.http.get(&url).send().await.map_err(|e| {
            anyhow!("Failed to send BRouter request to {url}: {e}")
        })?;
        let status = response.status();
        let text = response.text().await.map_err(|e| {
            anyhow!("Failed to read BRouter response body from {url}: {e}")
        })?;

        if !status.is_success() {
            return Err(anyhow!(
                "BRouter returned HTTP {status} for {url}: {text}"
            ));
        }

        let parsed: ApiResponse = serde_json::from_str(&text).map_err(|e| {
            anyhow!(
                "BRouter returned non-JSON response (HTTP {status}): {e}. Body: {body}",
                body = &text[..text.len().min(500)]
            )
        })?;

        let total_time = parsed
            .features
            .get(0)
            .ok_or(anyhow!("No features in BRouter response for {url}"))?
            .properties
            .total_time
            .as_str();
        parse_total_time(total_time)
    }
}

#[async_trait]
impl RoutingProvider for BRouter {
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

        // car-vario occasionally snaps a waypoint to a node it can't route from
        // ("no track found at pass=0"). A ~500m nudge lands on a routable node, so
        // the first attempt uses exact coords and retries jitter both endpoints.
        let mut last_err = None;
        for attempt in 0..=MAX_SNAP_RETRIES {
            let (src, dst) = if attempt == 0 {
                (source.clone(), destination.clone())
            } else {
                (jittered(source), jittered(destination))
            };
            match self.get_travel_time_call(&src, &dst).await {
                Ok(seconds) => {
                    let jitter: f32 = rand::rng().random_range(0.9..1.1);
                    self.cache
                        .put(
                            &key,
                            seconds,
                            StdDuration::from_hours((24f32 * 7f32 * jitter) as u64),
                        )
                        .await?;
                    return Ok(Duration::seconds(seconds as i64));
                }
                Err(e) => {
                    tracing::debug!(attempt, error = %e, "BRouter call failed, retrying with jittered coordinates");
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.expect("loop runs at least once and only exits via Ok or a recorded Err"))
    }
}

/// Nudge a waypoint by up to ~500m to escape a node BRouter can't route from.
fn jittered(loc: &Location) -> Location {
    let mut rng = rand::rng();
    // 0.0045° ≈ 500m of latitude; lon offset isn't cos-scaled — close enough for a nudge.
    Location::new(
        loc.latitude + rng.random_range(-0.0045..0.0045),
        loc.longitude + rng.random_range(-0.0045..0.0045),
        loc.name.clone(),
        loc.country.clone(),
    )
}

fn parse_total_time(raw: &str) -> Result<u64> {
    raw.parse::<u64>()
        .map_err(|e| anyhow!("BRouter total-time {raw:?} is not an integer: {e}"))
}

#[derive(Debug, Deserialize)]
struct PropertiesResponse {
    #[serde(rename = "total-time")]
    total_time: String,
}

#[derive(Debug, Deserialize)]
struct FeatureResponse {
    properties: PropertiesResponse,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    features: Vec<FeatureResponse>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_total_time_from_geojson() {
        // Trimmed BRouter response — only the field we read.
        let body = r#"{
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "properties": {
                    "creator": "BRouter-1.7.5",
                    "name": "brouter",
                    "track-length": "12345",
                    "total-time": "1834"
                },
                "geometry": { "type": "LineString", "coordinates": [] }
            }]
        }"#;
        let parsed: ApiResponse = serde_json::from_str(body).unwrap();
        let secs = parse_total_time(&parsed.features[0].properties.total_time).unwrap();
        assert_eq!(secs, 1834);
    }

    #[test]
    fn rejects_non_integer_total_time() {
        assert!(parse_total_time("not-a-number").is_err());
    }
}
