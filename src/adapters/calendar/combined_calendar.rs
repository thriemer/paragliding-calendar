use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::future;

use crate::{
    adapters::calendar::{
        google_calendar::GoogleCalendar, microsoft_calendar::MicrosoftCalendar,
    },
    domain::{calendar::CalendarEvent, ports::CalendarProvider},
};

pub struct CombinedCalendar {
    google: GoogleCalendar,
    microsoft: Option<MicrosoftCalendar>,
}

impl CombinedCalendar {
    pub fn new(google: GoogleCalendar, microsoft: Option<MicrosoftCalendar>) -> Self {
        Self { google, microsoft }
    }
}

#[async_trait]
impl CalendarProvider for CombinedCalendar {
    async fn get_events(
        &self,
        calendars: &[String],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<CalendarEvent>> {
        let google_fut = self.google.get_events(calendars, start, end);
        let microsoft_fut = async {
            if let Some(ms) = self.microsoft.as_ref() {
                match ms.get_events(calendars, start, end).await {
                    Ok(events) => events,
                    Err(e) => {
                        tracing::warn!(error = ?e, "Microsoft get_events failed; ignoring its events");
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            }
        };
        let (google_events, mut microsoft_events) = future::join(google_fut, microsoft_fut).await;
        let mut events = google_events?;
        events.append(&mut microsoft_events);
        Ok(events)
    }

    async fn get_calendar_names(&self) -> Result<Vec<String>> {
        self.google.get_calendar_names().await
    }

    async fn clear_calendar(&self, name: &str) -> Result<()> {
        self.google.clear_calendar(name).await
    }

    async fn create_event(&self, calendar: &str, event: CalendarEvent) -> Result<()> {
        self.google.create_event(calendar, event).await
    }

    async fn create_calendar(&self, name: &str) -> Result<()> {
        self.google.create_calendar(name).await
    }
}
