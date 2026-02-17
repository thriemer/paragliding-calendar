use std::fmt::Display;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use google_calendar3::api::{Event, EventDateTime, Scope};
use tracing;

use crate::calender::google_backend::CalendarHubType;

pub mod google_backend;

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

struct Calendar {
    hub: CalendarHubType,
    lists: Vec<CalendarList>,
}

impl Calendar {
    pub async fn fetch_all_lists(
        &mut self,
        time_min: DateTime<Utc>,
        time_max: DateTime<Utc>,
    ) -> Result<()> {
        let (_, lists) = self
            .hub
            .calendar_list()
            .list()
            .add_scope(Scope::Full)
            .doit()
            .await?;

        let lists = lists.items.unwrap();
        for l in lists {
            let id = l.id.clone().unwrap();
            let name = l.summary.clone().unwrap();

            let rt = google_backend::fetch_calendar_events(&self.hub, &time_min, &time_max, &id)
                .await
                .context(format!("While fetching calender {:?}", l));
            match rt {
                Ok(rt) => {
                    let cl = CalendarList {
                        name: name.to_owned(),
                        events: rt,
                    };
                    self.lists.push(cl);
                }
                Err(err) => tracing::warn!("Failed to fetch calendar: {}", err),
            }
        }
        Ok(())
    }

    pub fn has_overlap(&self, start: DateTime<Utc>, stop: DateTime<Utc>) -> bool {
        self.lists.iter().any(|l| l.has_overlap(start, stop))
    }

    pub fn remove_calendar_list(&mut self, name: &str) {
        self.lists.retain(|f| f.name != name);
        for n in &self.lists {
            tracing::debug!("Calendar list: {}", n.name);
        }
    }

    pub async fn set_calendar_entries(&self, name: &str, events: Vec<CalendarEvent>) -> Result<()> {
        if let Some(id) = self.get_id_from_name(name).await {
            tracing::info!("Found calendar {} for name {}", id, name);
            if let Err(e) = self
                .hub
                .calendars()
                .delete(&id)
                .add_scope(Scope::Full)
                .doit()
                .await
            {
                tracing::warn!("Failed to delete calendar {}: {}", name, e);
            }
        }
        let mut cal = google_calendar3::api::Calendar::default();
        cal.summary = Some(name.into());
        let (_, cal) = self
            .hub
            .calendars()
            .insert(cal)
            .add_scope(Scope::Full)
            .doit()
            .await?;
        let id = cal.id.unwrap();
        for e in events {
            self.hub
                .events()
                .insert(e.into(), &id)
                .add_scope(Scope::Full)
                .doit()
                .await?;
        }
        Ok(())
    }

    async fn get_id_from_name(&self, name: &str) -> Option<String> {
        let (_, lists) = self
            .hub
            .calendar_list()
            .list()
            .add_scope(Scope::Full)
            .doit()
            .await
            .unwrap();

        let lists = lists.items.unwrap();
        lists
            .iter()
            .filter(|l| {
                if let Some(desc) = &l.summary {
                    desc == name
                } else {
                    false
                }
            })
            .map(|l| l.id.clone().unwrap())
            .collect::<Vec<String>>()
            .first()
            .cloned()
    }

    pub fn print_events(&self) {
        for list in &self.lists {
            tracing::debug!("Calendar: {}", list.name);
            for event in &list.events {
                tracing::debug!("{}", event);
            }
        }
    }
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

mod test {
    use chrono::{DateTime, TimeZone, Utc};
    use rstest::rstest;

    use crate::calender::CalendarEvent;

    fn test_event() -> CalendarEvent {
        CalendarEvent {
            summary: "Test Event".to_string(),
            start_time: Utc.with_ymd_and_hms(2023, 10, 1, 10, 0, 0).unwrap(), // Oct 1, 2023 10:00
            end_time: Utc.with_ymd_and_hms(2023, 10, 1, 12, 0, 0).unwrap(),   // Oct 1, 2023 12:00
            is_all_day: false,
            location: None,
        }
    }

    #[rstest]
    // Test cases where overlap should be true
    #[case(
    Utc.with_ymd_and_hms(2023, 10, 1, 9, 30, 0).unwrap(),  // start: 9:30
    Utc.with_ymd_and_hms(2023, 10, 1, 10, 30, 0).unwrap(),  // stop: 10:30
    true
)] // Range starts before event, ends during event
    #[case(
    Utc.with_ymd_and_hms(2023, 10, 1, 10, 30, 0).unwrap(),  // start: 10:30
    Utc.with_ymd_and_hms(2023, 10, 1, 13, 0, 0).unwrap(),   // stop: 13:00
    true
)] // Range starts during event, ends after event
    #[case(
    Utc.with_ymd_and_hms(2023, 10, 1, 9, 0, 0).unwrap(),    // start: 9:00
    Utc.with_ymd_and_hms(2023, 10, 1, 13, 0, 0).unwrap(),   // stop: 13:00
    true
)] // Range completely contains event
    #[case(
    Utc.with_ymd_and_hms(2023, 10, 1, 10, 30, 0).unwrap(),  // start: 10:30
    Utc.with_ymd_and_hms(2023, 10, 1, 11, 30, 0).unwrap(),  // stop: 11:30
    true
)] // Range completely inside event
    // Test cases where overlap should be false
    #[case(
    Utc.with_ymd_and_hms(2023, 10, 1, 8, 0, 0).unwrap(),    // start: 8:00
    Utc.with_ymd_and_hms(2023, 10, 1, 10, 0, 0).unwrap(),   // stop: 10:00
    false
)] // Range ends exactly at event start (no overlap with strict inequality)
    #[case(
    Utc.with_ymd_and_hms(2023, 10, 1, 12, 0, 0).unwrap(),   // start: 12:00
    Utc.with_ymd_and_hms(2023, 10, 1, 14, 0, 0).unwrap(),   // stop: 14:00
    false
)] // Range starts exactly at event end (no overlap with strict inequality)
    #[case(
    Utc.with_ymd_and_hms(2023, 10, 1, 13, 0, 0).unwrap(),   // start: 13:00
    Utc.with_ymd_and_hms(2023, 10, 1, 14, 0, 0).unwrap(),   // stop: 14:00
    false
)] // Range completely after event
    fn test_has_overlap_fixed(
        #[case] test_start: DateTime<Utc>,
        #[case] test_stop: DateTime<Utc>,
        #[case] expected_overlap: bool,
    ) {
        let event = test_event();
        assert_eq!(
            event.has_overlap(test_start, test_stop),
            expected_overlap,
            "Failed for range: {} to {}",
            test_start,
            test_stop
        );
    }

    // Test for inclusive boundaries (if using <= and >=)
    #[rstest]
    #[case(
    Utc.with_ymd_and_hms(2023, 10, 1, 8, 0, 0).unwrap(),    // start: 8:00
    Utc.with_ymd_and_hms(2023, 10, 1, 10, 0, 0).unwrap(),   // stop: 10:00
    true  // With inclusive boundaries, this would be true
)] // Range ends exactly at event start
    #[case(
    Utc.with_ymd_and_hms(2023, 10, 1, 12, 0, 0).unwrap(),   // start: 12:00
    Utc.with_ymd_and_hms(2023, 10, 1, 14, 0, 0).unwrap(),   // stop: 14:00
    true  // With inclusive boundaries, this would be true
)] // Range starts exactly at event end
    fn test_has_overlap_inclusive(
        #[case] test_start: DateTime<Utc>,
        #[case] test_stop: DateTime<Utc>,
        #[case] expected_overlap: bool,
    ) {
        let event = test_event();
        // Using inclusive version
        let result = test_start <= event.end_time && test_stop >= event.start_time;
        assert_eq!(
            result, expected_overlap,
            "Failed for inclusive range: {} to {}",
            test_start, test_stop
        );
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

#[derive(Debug)]
struct CalendarList {
    pub name: String,
    pub events: Vec<CalendarEvent>,
}

impl CalendarList {
    pub fn has_overlap(&self, start: DateTime<Utc>, stop: DateTime<Utc>) -> bool {
        self.events
            .iter()
            .any(|event| event.has_overlap(start, stop))
    }
}
