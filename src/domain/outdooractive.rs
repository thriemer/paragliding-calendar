use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::location::Location;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutdoorEvent {
    pub id: String,
    pub title: String,
    pub location: Option<Location>,
    pub category_id: Option<String>,
    pub category_title: Option<String>,
    pub category_keys: Vec<String>,
    pub description_short: Option<String>,
    pub description_long: Option<String>,
    pub homepage: Option<String>,
    pub address: Option<serde_json::Value>,
    pub organizer: Option<String>,
    pub schedule_rules: Option<serde_json::Value>,
    pub dates: Vec<EventDate>,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDate {
    pub time_from: DateTime<Utc>,
    pub time_to: DateTime<Utc>,
    pub date_text: Option<String>,
}

/// Append the outdoor-active detail page for `id` to a description `body`. `/de/r/<id>` 301-redirects
/// to the full localized page and works for both tours and events. A bare URL so Google Calendar
/// auto-links it; a blank line separates it from the body (or it stands alone if the body is empty).
pub fn outdooractive_link(body: &str, id: &str) -> String {
    let url = format!("https://www.outdooractive.com/de/r/{id}");
    if body.is_empty() {
        url
    } else {
        format!("{body}\n\n{url}")
    }
}

#[cfg(test)]
mod tests {
    use super::outdooractive_link;

    #[test]
    fn appends_link_after_blank_line() {
        assert_eq!(
            outdooractive_link("A hike", "801692621"),
            "A hike\n\nhttps://www.outdooractive.com/de/r/801692621"
        );
    }

    #[test]
    fn empty_body_yields_bare_link() {
        assert_eq!(
            outdooractive_link("", "38026594"),
            "https://www.outdooractive.com/de/r/38026594"
        );
    }
}
