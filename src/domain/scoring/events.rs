//! Scoring policy for generic dated events. Pure — no I/O, no ports.

use chrono::Duration;

/// Longest slice of an event we schedule. Events are `Fixed`-timed, so without a cap a multi-day
/// festival would occupy the whole schedule as one block. ponytail: attend the first few hours;
/// widen or make configurable if real events need it.
pub const MAX_ATTEND: Duration = Duration::hours(4);
