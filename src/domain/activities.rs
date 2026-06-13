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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn time_window_duration_is_end_minus_start() {
        let start = Utc.with_ymd_and_hms(2026, 6, 13, 10, 0, 0).unwrap();
        let end = start + Duration::hours(3);
        let w = TimeWindow { start, end };
        assert_eq!(w.duration(), Duration::hours(3));
    }

    #[test]
    fn time_window_zero_duration_when_start_equals_end() {
        let t = Utc.with_ymd_and_hms(2026, 6, 13, 10, 0, 0).unwrap();
        let w = TimeWindow { start: t, end: t };
        assert_eq!(w.duration(), Duration::zero());
    }
}
