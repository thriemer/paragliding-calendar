use chrono::{DateTime, Duration, Utc};

use crate::domain::location::Location;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    Paragliding,
}

#[derive(Debug, Clone, Copy)]
pub struct TimeWindow {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl TimeWindow {
    pub fn duration(&self) -> Duration {
        self.end - self.start
    }
}

#[derive(Debug, Clone)]
pub enum Timing {
    Fixed {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    },
    Flexible {
        window: TimeWindow,
        min_duration: Duration,
    },
}

#[derive(Debug, Clone)]
pub struct Score {
    pub value: f32,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ActivitySuggestion {
    pub kind: ActivityKind,
    pub location: Location,
    pub timing: Timing,
    pub title: String,
    pub description: String,
    pub score: Option<Score>,
}

#[derive(Debug, Clone)]
pub struct PlanningContext {
    pub home: Location,
    pub horizon: TimeWindow,
    pub conflict_calendars: Vec<String>,
}
