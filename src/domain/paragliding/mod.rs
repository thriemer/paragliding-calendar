pub mod flight;

use serde::{Deserialize, Serialize};

use crate::domain::location::Location;

pub trait ParaglidingSiteProvider {
    async fn fetch_all_sites(&self) -> Vec<ParaglidingSite>;
    async fn fetch_launches_within_radius(
        &self,
        center: &Location,
        radius_km: f64,
    ) -> Vec<(ParaglidingSite, f64)>;
}

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

#[must_use]
pub fn degrees_to_compass(degrees: f64) -> String {
    let normalized = degrees.rem_euclid(360.0);
    match normalized {
        d if d < 11.25 || d >= 348.75 => "N".to_string(),
        d if d < 33.75 => "NNE".to_string(),
        d if d < 56.25 => "NE".to_string(),
        d if d < 78.75 => "ENE".to_string(),
        d if d < 101.25 => "E".to_string(),
        d if d < 123.75 => "ESE".to_string(),
        d if d < 146.25 => "SE".to_string(),
        d if d < 168.75 => "SSE".to_string(),
        d if d < 191.25 => "S".to_string(),
        d if d < 213.75 => "SSW".to_string(),
        d if d < 236.25 => "SW".to_string(),
        d if d < 258.75 => "WSW".to_string(),
        d if d < 281.25 => "W".to_string(),
        d if d < 303.75 => "WNW".to_string(),
        d if d < 326.25 => "NW".to_string(),
        _ => "NNW".to_string(),
    }
}
