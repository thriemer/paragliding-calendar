use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::location::Location;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Happening {
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
    pub dates: Vec<HappeningDate>,
    pub source_url: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HappeningDate {
    pub time_from: DateTime<Utc>,
    pub time_to: DateTime<Utc>,
    pub date_text: Option<String>,
}
