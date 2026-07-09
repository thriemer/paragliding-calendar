//! Offline routing estimator: travel time from straight-line ("as the crow flies")
//! distance, no external service. For testing the planner without GraphHopper/BRouter/Valhalla.
#![allow(dead_code)] // ponytail: Valhalla is wired now; kept as the offline fallback.

use anyhow::Result;
use async_trait::async_trait;
use chrono::Duration;
use tracing::instrument;

use crate::domain::{location::Location, ports::RoutingProvider};

// ponytail: calibration knobs. 1.3 = standard road circuity factor (crow-flies → road
// distance); 70 km/h = realistic true road speed for mixed German/Czech driving. Bump
// DETOUR_FACTOR for mountainous approaches.
const DETOUR_FACTOR: f64 = 1.3;
const SPEED_KMH: f64 = 70.0;

pub struct CrowFlies;

impl CrowFlies {
    pub fn new() -> Self {
        Self
    }
}

/// `km` straight-line → estimated drive time. Pure so it's testable without a `Location`.
fn estimate_duration(km: f64) -> Duration {
    Duration::seconds(((km * DETOUR_FACTOR / SPEED_KMH) * 3600.0) as i64)
}

#[async_trait]
impl RoutingProvider for CrowFlies {
    #[instrument(skip(self))]
    async fn get_travel_time(
        &self,
        source: &Location,
        destination: &Location,
    ) -> Result<Duration> {
        Ok(estimate_duration(source.distance_to(destination)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_time_from_distance() {
        // 100 km straight-line × 1.3 ÷ 70 km/h = 1.857 h = 6685 s.
        assert_eq!(estimate_duration(100.0).num_seconds(), 6685);
    }
}
