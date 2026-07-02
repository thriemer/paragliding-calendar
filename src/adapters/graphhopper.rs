#![allow(dead_code)] // ponytail: routing stack not wired into AppState yet (CrowFlies stands in); kept per owner's call.

use std::{env, sync::Arc, time::Duration as StdDuration};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::Duration;
use rand::RngExt;
use reqwest::{StatusCode, Client};
use serde::Deserialize;
use serde_json::json;
use tracing::instrument;

use crate::{
    adapters::{
        cache::PersistentCache,
        routing_error::RoutingError,
        routing_matrix::{
            FetchPlan, assemble_from_cache, cache_pairs, fill_from_blocks, fill_unroutable,
            finalize, plan_fetch,
        },
    },
    domain::{location::Location, ports::RoutingProvider},
};

const MAX_RETRIES: u32 = 3;
/// GraphHopper's matrix add-on caps points per request (free tier: 5). Larger matrices are
/// tiled into ≤5-per-side blocks so we stay under the limit instead of getting a 400.
const MAX_MATRIX_POINTS: usize = 5;

pub struct Routing {
    cache: Arc<PersistentCache>,
    http: Client,
}

impl Routing {
    pub fn new(cache: Arc<PersistentCache>, http: Client) -> Self {
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
                            let wait = parse_retry_after(&headers)
                                .unwrap_or(StdDuration::from_secs(60));
                            tracing::warn!(
                                attempt,
                                wait_ms = wait.as_millis(),
                                "GraphHopper rate limited, retrying after Retry-After"
                            );
                            tokio::time::sleep(wait).await;
                            last_error = Some(RoutingError::RateLimitExceeded(body).into());
                            continue;
                        }
                        return Err(RoutingError::DailyQuotaExhausted(body).into());
                    }
                    if !status.is_success() {
                        let body = response.text().await.unwrap_or_default();
                        return Err(anyhow!("GraphHopper returned {}: {}", status, body));
                    }
                    let parsed: ApiResponse = response.json().await?;
                    return parsed
                        .paths.first()
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

        Err(last_error
            .unwrap_or(anyhow!("GraphHopper request failed after {MAX_RETRIES} retries")))
    }

    /// One `/matrix` request for a `sources × targets` block. Result is indexed
    /// `[source][target]`; cells may be null (unroutable) — represented as `None`. Maps
    /// 429/quota bodies to `RoutingError` so the caller can fall back, mirroring
    /// `get_travel_time_call`.
    async fn matrix_call(
        &self,
        sources: &[Location],
        targets: &[Location],
    ) -> Result<Vec<Vec<Option<u64>>>> {
        let key = env::var("GRAPHHOPPER_API_KEY").context("Missing GRAPHHOPPER_API_KEY env var")?;
        let url = format!("https://graphhopper.com/api/1/matrix?key={key}");
        // GraphHopper points are [lon, lat].
        let pts = |ls: &[Location]| ls.iter().map(|l| [l.longitude, l.latitude]).collect::<Vec<_>>();
        let body = json!({
            "from_points": pts(sources),
            "to_points": pts(targets),
            "out_arrays": ["times"],
            "profile": "car",
            "fail_fast": false,
        });
        tracing::debug!(s = sources.len(), t = targets.len(), "Calling the GraphHopper matrix API");

        let response = self.http.post(&url).json(&body).send().await?;
        let status = response.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            let body = response.text().await.unwrap_or_default();
            if body.contains("Minutely") {
                return Err(RoutingError::RateLimitExceeded(body).into());
            }
            return Err(RoutingError::DailyQuotaExhausted(body).into());
        }
        if !status.is_success() {
            // Matrix is a paid add-on with per-plan point limits (e.g. free tier caps at 5
            // points → 400). Treat any matrix failure as "GraphHopper can't serve this" so
            // the fallback provider routes to Valhalla instead of failing the plan.
            let body = response.text().await.unwrap_or_default();
            return Err(RoutingError::MatrixUnavailable(format!("HTTP {status}: {body}")).into());
        }

        let parsed: MatrixResponse = response.json().await?;
        Ok(parsed.times)
    }

    /// `matrix_call` split into ≤`MAX_MATRIX_POINTS`-per-side blocks and stitched back into
    /// the full `sources × targets` grid, keeping every request within GraphHopper's point cap.
    async fn matrix_tiled(
        &self,
        sources: &[Location],
        targets: &[Location],
    ) -> Result<Vec<Vec<Option<u64>>>> {
        let mut out = vec![vec![None; targets.len()]; sources.len()];
        for (sb, s_chunk) in sources.chunks(MAX_MATRIX_POINTS).enumerate() {
            for (tb, t_chunk) in targets.chunks(MAX_MATRIX_POINTS).enumerate() {
                let block = self.matrix_call(s_chunk, t_chunk).await?;
                let (s_off, t_off) = (sb * MAX_MATRIX_POINTS, tb * MAX_MATRIX_POINTS);
                for (r, row) in block.into_iter().enumerate() {
                    for (c, cell) in row.into_iter().enumerate() {
                        out[s_off + r][t_off + c] = cell;
                    }
                }
            }
        }
        Ok(out)
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

    #[instrument(skip(self, locations))]
    async fn travel_time_matrix(&self, locations: &[Location]) -> Result<Vec<Vec<Duration>>> {
        let (mut secs, missing) = assemble_from_cache(&self.cache, locations).await?;
        match plan_fetch(&missing, locations.len()) {
            FetchPlan::None => {}
            FetchPlan::Full => {
                let block = self.matrix_tiled(locations, locations).await?;
                for &(i, j) in &missing {
                    secs[i][j] = block[i][j];
                }
            }
            FetchPlan::Incremental { cover } => {
                let cover_locs: Vec<Location> = cover.iter().map(|&i| locations[i].clone()).collect();
                let block_cover_all = self.matrix_tiled(&cover_locs, locations).await?;
                let block_all_cover = self.matrix_tiled(locations, &cover_locs).await?;
                fill_from_blocks(&mut secs, &missing, &cover, &block_cover_all, &block_all_cover);
            }
        }
        fill_unroutable(self, locations, &mut secs, &missing).await?;
        cache_pairs(&self.cache, locations, &missing, &secs).await?;
        Ok(finalize(&secs))
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

#[derive(Debug, Deserialize)]
struct MatrixResponse {
    times: Vec<Vec<Option<u64>>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_matrix_times() {
        // Trimmed /matrix response: 2x2 times (seconds), one unroutable cell (null).
        let body = r#"{ "times": [[0, 600], [null, 0]] }"#;
        let parsed: MatrixResponse = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.times[0][1], Some(600));
        assert_eq!(parsed.times[1][0], None);
    }
}
