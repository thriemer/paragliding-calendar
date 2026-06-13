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
