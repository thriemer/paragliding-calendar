use std::fmt::Display;

use anyhow::Result;
use chrono::{DateTime, Utc};
use google_calendar3::api::{Event, EventDateTime};

pub mod google;

pub trait CalendarProvider {
    async fn is_busy(
        &self,
        calendars: &Vec<String>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<bool>;
    async fn get_calendar_names(&self) -> Result<Vec<String>>;
    async fn clear_calendar(&mut self, name: &str) -> Result<()>;
    async fn create_event(&mut self, calendar: &str, event: CalendarEvent) -> Result<()>;
    async fn create_calendar(&mut self, name: &str) -> Result<()>;
}

#[derive(Debug)]
pub struct CalendarEvent {
    pub summary: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub is_all_day: bool,
    pub location: Option<String>,
}

impl CalendarEvent {
    pub fn has_overlap(&self, start: DateTime<Utc>, stop: DateTime<Utc>) -> bool {
        start < self.end_time && stop > self.start_time
    }
}
impl From<CalendarEvent> for Event {
    fn from(value: CalendarEvent) -> Self {
        let mut event = Event::default();
        event.summary = Some(value.summary);
        event.start = Some(to_event_time(value.start_time));
        event.end = Some(to_event_time(value.end_time));
        event.location = value.location;
        event
    }
}

impl Display for CalendarEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.summary)?;

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

fn to_event_time(time: DateTime<Utc>) -> EventDateTime {
    EventDateTime {
        date: None,
        date_time: Some(time),
        time_zone: None,
    }
}
