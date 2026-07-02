pub mod flight;

use serde::{Deserialize, Serialize};

use crate::domain::location::Location;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParaglidingSite {
    pub name: String,
    pub launches: Vec<ParaglidingLaunch>,
    pub landings: Vec<ParaglidingLanding>,
    pub country: Option<String>,
    pub data_source: String,
    pub parking_location: Option<Location>,
    pub mute_alerts: Option<bool>,
    pub rating: Option<u8>,
    pub preferred_weather_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParaglidingLaunch {
    pub site_type: SiteType,
    pub location: Location,
    pub direction_degrees_start: f64,
    pub direction_degrees_stop: f64,
    pub elevation: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParaglidingLanding {
    pub location: Location,
    pub elevation: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum SiteType {
    Hang,
    Winch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettings {
    pub location_name: String,
    pub location_latitude: f64,
    pub location_longitude: f64,
    pub search_radius_km: f64,
    pub calendar_name: String,
    pub minimum_flyable_hours: u32,
    pub excluded_calendar_names: Vec<String>,
}

impl Default for UserSettings {
    fn default() -> Self {
        let calendar_name = "Paragliding".to_string();
        Self {
            //TODO: replace with real location
            location_name: "Gornau/Erz".to_string(),
            location_latitude: 50.7,
            location_longitude: 13.0,
            search_radius_km: 150.0,
            calendar_name: calendar_name.clone(),
            minimum_flyable_hours: 2,
            excluded_calendar_names: vec![calendar_name],
        }
    }
}
