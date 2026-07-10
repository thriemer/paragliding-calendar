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
