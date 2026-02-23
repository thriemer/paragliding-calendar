pub mod dhv;
pub mod site_evaluator;

use serde::{Deserialize, Serialize};

use super::Location;

pub trait ParaglidingSiteProvider {
    async fn fetch_all_sites(&self) -> Vec<ParaglidingSite>;
    async fn fetch_launches_within_radius(
        &self,
        center: &Location,
        radius_km: f64,
    ) -> Vec<(ParaglidingSite, f64)>;
}

/// Represents a paragliding site from any data source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParaglidingSite {
    pub name: String,
    pub launches: Vec<ParaglidingLaunch>,
    pub landings: Vec<ParaglidingLanding>,
    pub country: Option<String>,
    pub data_source: String,
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
pub enum SiteType {
    Hang,
    Winch,
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
