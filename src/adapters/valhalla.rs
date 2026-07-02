use std::{sync::Arc, time::Duration as StdDuration};

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::Duration;
use rand::RngExt;
use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use crate::{
    adapters::{
        cache::PersistentCache,
        routing_matrix::{
            FetchPlan, assemble_from_cache, cache_pairs, fill_from_blocks, fill_unroutable,
            finalize, plan_fetch,
        },
    },
    domain::{location::Location, ports::RoutingProvider},
};

pub struct Valhalla {
    base_url: String,
    cache: Arc<PersistentCache>,
    http: ClientWithMiddleware,
}

impl Valhalla {
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
        let url = format!("{}/route", self.base_url.trim_end_matches('/'));
        let body = json!({
            "locations": [
                {"lat": source.latitude, "lon": source.longitude},
                {"lat": destination.latitude, "lon": destination.longitude},
            ],
            "costing": "auto",
        });
        tracing::debug!(url = %url, "Calling the Valhalla API");

        let response = self.http.post(&url).json(&body).send().await.map_err(|e| {
            anyhow!("Failed to send Valhalla request to {url}: {e}")
        })?;
        let status = response.status();
        let text = response.text().await.map_err(|e| {
            anyhow!("Failed to read Valhalla response body from {url}: {e}")
        })?;

        if !status.is_success() {
            return Err(anyhow!("Valhalla returned HTTP {status} for {url}: {text}"));
        }

        let parsed: ApiResponse = serde_json::from_str(&text).map_err(|e| {
            anyhow!(
                "Valhalla returned non-JSON response (HTTP {status}): {e}. Body: {body}",
                body = &text[..text.len().min(500)]
            )
        })?;

        Ok(parsed.trip.summary.time.round() as u64)
    }

    /// One `/sources_to_targets` request for a `sources × targets` block. Result is indexed
    /// `[source][target]`; cells may be null (unroutable) — represented as `None`.
    async fn matrix_call(
        &self,
        sources: &[Location],
        targets: &[Location],
    ) -> Result<Vec<Vec<Option<u64>>>> {
        let url = format!("{}/sources_to_targets", self.base_url.trim_end_matches('/'));
        let point = |l: &Location| json!({"lat": l.latitude, "lon": l.longitude});
        let body = json!({
            "sources": sources.iter().map(point).collect::<Vec<_>>(),
            "targets": targets.iter().map(point).collect::<Vec<_>>(),
            "costing": "auto",
        });
        tracing::debug!(url = %url, s = sources.len(), t = targets.len(), "Calling the Valhalla matrix API");

        let response = self.http.post(&url).json(&body).send().await.map_err(|e| {
            anyhow!("Failed to send Valhalla matrix request to {url}: {e}")
        })?;
        let status = response.status();
        let text = response.text().await.map_err(|e| {
            anyhow!("Failed to read Valhalla matrix response body from {url}: {e}")
        })?;

        if !status.is_success() {
            return Err(anyhow!("Valhalla matrix returned HTTP {status} for {url}: {text}"));
        }

        let parsed: MatrixResponse = serde_json::from_str(&text).map_err(|e| {
            anyhow!(
                "Valhalla matrix returned non-JSON response (HTTP {status}): {e}. Body: {body}",
                body = &text[..text.len().min(500)]
            )
        })?;

        Ok(parsed
            .sources_to_targets
            .into_iter()
            .map(|row| row.into_iter().map(|c| c.time.map(|t| t.round() as u64)).collect())
            .collect())
    }
}

#[async_trait]
impl RoutingProvider for Valhalla {
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

        // ponytail: no snap-retry; Valhalla snaps to the road network itself. Add a retry
        // loop if we see routing-failure errors in practice (BRouter's car-vario needs it).
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

    #[instrument(skip(self, locations))]
    async fn travel_time_matrix(&self, locations: &[Location]) -> Result<Vec<Vec<Duration>>> {
        let (mut secs, missing) = assemble_from_cache(&self.cache, locations).await?;
        match plan_fetch(&missing, locations.len()) {
            FetchPlan::None => {}
            FetchPlan::Full => {
                let block = self.matrix_call(locations, locations).await?;
                for &(i, j) in &missing {
                    secs[i][j] = block[i][j];
                }
            }
            FetchPlan::Incremental { cover } => {
                let cover_locs: Vec<Location> = cover.iter().map(|&i| locations[i].clone()).collect();
                let block_cover_all = self.matrix_call(&cover_locs, locations).await?;
                let block_all_cover = self.matrix_call(locations, &cover_locs).await?;
                fill_from_blocks(&mut secs, &missing, &cover, &block_cover_all, &block_all_cover);
            }
        }
        fill_unroutable(self, locations, &mut secs, &missing).await?;
        cache_pairs(&self.cache, locations, &missing, &secs).await?;
        Ok(finalize(&secs))
    }
}

#[derive(Debug, Deserialize)]
struct MatrixCell {
    time: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct MatrixResponse {
    sources_to_targets: Vec<Vec<MatrixCell>>,
}

#[derive(Debug, Deserialize)]
struct SummaryResponse {
    time: f64,
}

#[derive(Debug, Deserialize)]
struct TripResponse {
    summary: SummaryResponse,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    trip: TripResponse,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_time_from_trip_summary() {
        // Trimmed Valhalla /route response — only the field we read.
        let body = r#"{
            "trip": {
                "summary": { "time": 1834.6, "length": 12.3 },
                "status": 0
            }
        }"#;
        let parsed: ApiResponse = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.trip.summary.time.round() as u64, 1835);
    }

    #[test]
    fn parses_sources_to_targets_matrix() {
        // Trimmed /sources_to_targets response: 2x2 with one unroutable cell (null time).
        let body = r#"{
            "sources_to_targets": [
                [{"time": 0, "from_index": 0, "to_index": 0},
                 {"time": 600.4, "from_index": 0, "to_index": 1}],
                [{"time": null, "from_index": 1, "to_index": 0},
                 {"time": 0, "from_index": 1, "to_index": 1}]
            ]
        }"#;
        let parsed: MatrixResponse = serde_json::from_str(body).unwrap();
        let secs: Vec<Vec<Option<u64>>> = parsed
            .sources_to_targets
            .into_iter()
            .map(|row| row.into_iter().map(|c| c.time.map(|t| t.round() as u64)).collect())
            .collect();
        assert_eq!(secs[0][1], Some(600));
        assert_eq!(secs[1][0], None);
    }
}
