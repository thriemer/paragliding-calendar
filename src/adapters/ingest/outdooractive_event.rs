use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use crate::domain::{location::Location, outdooractive::{EventDate, OutdoorEvent}};

/// Parse one outdoor-active event JSON document into an `OutdoorEvent`. Mirrors `hiking::parse_tour`:
/// the adapter owns the wire format, the domain type is what leaves here.
pub fn parse_event(json: &str) -> Result<OutdoorEvent> {
    let root: serde_json::Value = serde_json::from_str(json).context("invalid JSON")?;
    let event = root
        .pointer("/answer/contents/0")
        .context("missing answer.contents[0]")?;

    let id = event.get("id").and_then(|v| v.as_str()).context("missing id")?.to_string();
    let title = event.get("title").and_then(|v| v.as_str()).context("missing title")?.to_string();

    let location = event
        .get("point")
        .and_then(|v| v.as_array())
        .filter(|arr| arr.len() >= 2)
        .and_then(|arr| Some((arr[0].as_f64()?, arr[1].as_f64()?)))
        .map(|(lon, lat)| Location::new(lat, lon, String::new(), String::new()));

    let category_id = event.pointer("/category/id").and_then(|v| v.as_str().map(String::from));
    let category_title = event.pointer("/category/title").and_then(|v| v.as_str().map(String::from));
    let category_keys: Vec<String> = event
        .pointer("/category/keys")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let description_short = event.pointer("/texts/short").and_then(|v| v.as_str().map(String::from));
    let description_long = event.pointer("/texts/long").and_then(|v| v.as_str().map(String::from));
    let homepage = event.get("homepage").and_then(|v| v.as_str().map(String::from));
    let address = event
        .pointer("/locationInfo/address")
        .filter(|v| !v.is_null())
        .cloned();
    let organizer = event.pointer("/organizer/name").and_then(|v| v.as_str().map(String::from));
    let schedule_rules = event.get("scheduleRules").filter(|v| !v.is_null()).cloned();

    let dates = event
        .get("nextDates")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|d| {
                    let time_from = d.get("timeFrom")?.as_str()?.parse::<DateTime<Utc>>().ok()?;
                    let time_to = d.get("timeTo")?.as_str()?.parse::<DateTime<Utc>>().ok()?;
                    let date_text = d.get("text").and_then(|v| v.as_str().map(String::from));
                    Some(EventDate { time_from, time_to, date_text })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(OutdoorEvent {
        id,
        title,
        location,
        category_id,
        category_title,
        category_keys,
        description_short,
        description_long,
        homepage,
        address,
        organizer,
        schedule_rules,
        dates,
        data: event.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_event_with_dates_and_location() {
        let json = r#"{"answer":{"contents":[{
            "id":"e1","title":"Festival","point":[13.0,50.0,300.0],
            "category":{"id":"c1","title":"Musik","keys":["music","open-air"]},
            "texts":{"short":"short","long":"long"},
            "homepage":"https://x",
            "organizer":{"name":"Org"},
            "nextDates":[
                {"timeFrom":"2026-06-14T19:00:00+00:00","timeTo":"2026-06-14T23:00:00+00:00","text":"Sat"},
                {"timeFrom":"bad","timeTo":"2026-06-15T23:00:00+00:00"}
            ]
        }]}}"#;
        let e = parse_event(json).unwrap();
        assert_eq!(e.id, "e1");
        assert_eq!(e.title, "Festival");
        let loc = e.location.unwrap();
        assert_eq!((loc.latitude, loc.longitude), (50.0, 13.0));
        assert_eq!(e.category_keys, vec!["music", "open-air"]);
        assert_eq!(e.organizer.as_deref(), Some("Org"));
        assert_eq!(e.dates.len(), 1); // the unparseable date is dropped
        assert_eq!(e.dates[0].date_text.as_deref(), Some("Sat"));
    }

    #[test]
    fn locationless_and_dateless_event_still_parses() {
        let json = r#"{"answer":{"contents":[{"id":"e2","title":"Online Talk"}]}}"#;
        let e = parse_event(json).unwrap();
        assert!(e.location.is_none());
        assert!(e.dates.is_empty());
        assert!(e.category_keys.is_empty());
    }

    #[test]
    fn short_point_array_yields_no_location() {
        let json = r#"{"answer":{"contents":[{"id":"e3","title":"T","point":[13.0]}]}}"#;
        assert!(parse_event(json).unwrap().location.is_none());
    }
}
