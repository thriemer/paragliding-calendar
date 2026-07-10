#![allow(dead_code)] // ponytail: routing stack not wired into AppState yet (CrowFlies stands in); kept per owner's call.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use tracing::instrument;

use crate::{
    adapters::routing::routing_error::RoutingError,
    domain::{location::Location, ports::RoutingProvider},
};

pub struct FallbackRoutingProvider {
    primary: Arc<dyn RoutingProvider>,
    fallback: Arc<dyn RoutingProvider>,
    cooldown_until: AtomicU64,
}

impl FallbackRoutingProvider {
    pub fn new(primary: Arc<dyn RoutingProvider>, fallback: Arc<dyn RoutingProvider>) -> Self {
        Self {
            primary,
            fallback,
            cooldown_until: AtomicU64::new(0),
        }
    }

    fn is_in_cooldown(&self) -> bool {
        let now = Utc::now().timestamp() as u64;
        now < self.cooldown_until.load(Ordering::Relaxed)
    }

    fn set_cooldown_until_midnight(&self) {
        let now = Utc::now().timestamp() as u64;
        let seconds_until_midnight = 86400 - (now % 86400);
        let cooldown = now + seconds_until_midnight;
        self.cooldown_until.store(cooldown, Ordering::Relaxed);
        tracing::warn!(
            "GraphHopper daily quota exhausted, falling back to Valhalla until UTC midnight"
        );
    }
}

#[async_trait]
impl RoutingProvider for FallbackRoutingProvider {
    #[instrument(skip(self))]
    async fn get_travel_time(
        &self,
        source: &Location,
        destination: &Location,
    ) -> Result<chrono::Duration> {
        if self.is_in_cooldown() {
            tracing::debug!("GraphHopper in cooldown, using Valhalla directly");
            return self.fallback.get_travel_time(source, destination).await;
        }

        match self.primary.get_travel_time(source, destination).await {
            Ok(duration) => Ok(duration),
            Err(err) => {
                if err.downcast_ref::<RoutingError>().is_some() {
                    if let Some(RoutingError::DailyQuotaExhausted(_)) =
                        err.downcast_ref::<RoutingError>()
                    {
                        self.set_cooldown_until_midnight();
                    }
                    tracing::warn!(
                        error = ?err,
                        "GraphHopper routing failed, falling back to Valhalla"
                    );
                    self.fallback.get_travel_time(source, destination).await
                } else {
                    Err(err)
                }
            }
        }
    }

    #[instrument(skip(self, locations))]
    async fn travel_time_matrix(&self, locations: &[Location]) -> Result<Vec<Vec<chrono::Duration>>> {
        if self.is_in_cooldown() {
            tracing::debug!("GraphHopper in cooldown, using Valhalla matrix directly");
            return self.fallback.travel_time_matrix(locations).await;
        }

        match self.primary.travel_time_matrix(locations).await {
            Ok(matrix) => Ok(matrix),
            Err(err) => {
                if err.downcast_ref::<RoutingError>().is_some() {
                    // Quota exhaustion and matrix-unavailable are both effectively "GraphHopper
                    // won't serve this today" — cool down so we don't re-hit it every solve.
                    if matches!(
                        err.downcast_ref::<RoutingError>(),
                        Some(RoutingError::DailyQuotaExhausted(_) | RoutingError::MatrixUnavailable(_))
                    ) {
                        self.set_cooldown_until_midnight();
                    }
                    tracing::warn!(
                        error = ?err,
                        "GraphHopper matrix failed, falling back to Valhalla"
                    );
                    self.fallback.travel_time_matrix(locations).await
                } else {
                    Err(err)
                }
            }
        }
    }
}
