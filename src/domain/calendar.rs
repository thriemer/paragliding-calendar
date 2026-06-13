use std::fmt::Display;

use chrono::{DateTime, Utc};

#[derive(Debug)]
pub struct CalendarEvent {
    pub title: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub is_all_day: bool,
    pub location: Option<String>,
    pub body: Option<String>,
}

impl CalendarEvent {
    pub fn has_overlap(&self, start: DateTime<Utc>, stop: DateTime<Utc>) -> bool {
        start < self.end_time && stop > self.start_time
    }
}

impl Display for CalendarEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.title)?;

        if self.is_all_day {
            writeln!(f, "   📅 All-day event")?;
        } else {
            writeln!(f, "   ⏰ {} - {}", self.start_time, self.end_time)?;
        }

        if let Some(location) = &self.location {
            writeln!(f, "   🗺️ Location: {}", location)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};

    fn event(start_h: u32, end_h: u32) -> CalendarEvent {
        CalendarEvent {
            title: "evt".into(),
            start_time: Utc.with_ymd_and_hms(2026, 6, 13, start_h, 0, 0).unwrap(),
            end_time: Utc.with_ymd_and_hms(2026, 6, 13, end_h, 0, 0).unwrap(),
            is_all_day: false,
            location: None,
            body: None,
        }
    }

    #[test]
    fn overlap_returns_true_for_intersecting_intervals() {
        let e = event(10, 12);
        let s = Utc.with_ymd_and_hms(2026, 6, 13, 11, 0, 0).unwrap();
        assert!(e.has_overlap(s, s + Duration::hours(2)));
    }

    #[test]
    fn overlap_returns_false_for_disjoint_before() {
        let e = event(10, 12);
        let s = Utc.with_ymd_and_hms(2026, 6, 13, 8, 0, 0).unwrap();
        assert!(!e.has_overlap(s, s + Duration::hours(1)));
    }

    #[test]
    fn overlap_returns_false_for_disjoint_after() {
        let e = event(10, 12);
        let s = Utc.with_ymd_and_hms(2026, 6, 13, 13, 0, 0).unwrap();
        assert!(!e.has_overlap(s, s + Duration::hours(1)));
    }

    #[test]
    fn overlap_returns_false_for_touching_at_end() {
        let e = event(10, 12);
        let s = Utc.with_ymd_and_hms(2026, 6, 13, 12, 0, 0).unwrap();
        assert!(!e.has_overlap(s, s + Duration::hours(1)));
    }

    #[test]
    fn overlap_returns_false_for_touching_at_start() {
        let e = event(10, 12);
        let s = Utc.with_ymd_and_hms(2026, 6, 13, 9, 0, 0).unwrap();
        assert!(!e.has_overlap(s, s + Duration::hours(1)));
    }
}
