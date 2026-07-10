use serde::{Deserialize, Serialize};

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
            // ponytail: bootstrap fallback only; real settings come from the DB
            // (SettingsRepository). Set your home via the /settings API.
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
