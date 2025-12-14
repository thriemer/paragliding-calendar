//! Location model for geographic coordinates and metadata

use serde::{Deserialize, Serialize};

/// Location coordinates
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    pub name: String,
    pub country: String,
}

impl Location {
    pub fn new(latitude: f64, longitude: f64, name: String, country: String) -> Self {
        Self {
            latitude,
            longitude,
            name,
            country
        }
    }

    pub fn format_coordinates(&self) -> String {
        format!("{:.4}, {:.4}", self.latitude, self.longitude)
    }

}
