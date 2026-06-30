use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use tracing::instrument;

use crate::{
    adapters::routing_error::RoutingError,
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
            "GraphHopper daily quota exhausted, falling back to BRouter until UTC midnight"
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
            tracing::debug!("GraphHopper in cooldown, using BRouter directly");
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
                        "GraphHopper routing failed, falling back to BRouter"
                    );
                    self.fallback.get_travel_time(source, destination).await
                } else {
                    Err(err)
                }
            }
        }
    }
}
