#![allow(dead_code)] // ponytail: routing stack not wired into AppState yet (CrowFlies stands in); kept per owner's call.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RoutingError {
    #[error("GraphHopper per-minute rate limit exceeded: {0}")]
    RateLimitExceeded(String),

    #[error("GraphHopper daily quota exhausted: {0}")]
    DailyQuotaExhausted(String),

    #[error("GraphHopper matrix unavailable on this subscription: {0}")]
    MatrixUnavailable(String),
}
