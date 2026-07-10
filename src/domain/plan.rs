//! The planner's request and result vocabulary: the context a plan is solved in, the
//! scheduled items it produces, and the overnight spots it chooses between. Depends on the
//! candidate vocabulary in [`crate::domain::activities`]; nothing here flows back the other way.

use chrono::{DateTime, Duration, Utc};

use crate::domain::activities::{ActivityKind, TimeWindow};
use crate::domain::location::Location;

/// Where a day ends: the location you sleep at. Today only `Home`; a future version adds campable
/// spots (with facilities, cost, notes) as `Camp`, which is why the solver references these by
/// `Arc<OvernightSpot>` rather than a bare `Location` or an id — the struct is meant to grow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OvernightKind {
    Home,
    // ponytail: constructed once `overnight_candidates` sources campable spots; the whole
    // overnight-in-genome machinery is wired for it, only the data source is deferred.
    #[allow(dead_code)]
    Camp,
}

#[derive(Debug, Clone)]
pub struct OvernightSpot {
    pub location: Location,
    pub kind: OvernightKind,
}

impl OvernightSpot {
    pub fn home(location: Location) -> Self {
        Self {
            location,
            kind: OvernightKind::Home,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlanningContext {
    pub home: Location,
    pub horizon: TimeWindow,
    pub conflict_calendars: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ScheduledActivity {
    pub kind: ActivityKind,
    /// `None` for online commitments (stationary, no drive); `Some` otherwise.
    pub location: Option<Location>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub title: String,
    pub description: String,
    pub fun: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub items: Vec<ScheduledActivity>,
    pub total_fun: f32,
    pub total_drive: Duration,
}
