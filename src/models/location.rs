//! Location model for geographic coordinates and metadata

use haversine::{Location as HaversineLocation, Units, distance};
use serde::{Deserialize, Serialize};

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
            country,
        }
    }

    pub fn format_coordinates(&self) -> String {
        format!("{:.4}, {:.4}", self.latitude, self.longitude)
    }

    pub fn distance_to(&self, other: &Location) -> f64 {
        Self::calculate_distance(self, other)
    }

    pub fn calculate_distance(from: &Location, to: &Location) -> f64 {
        let from_haversine = HaversineLocation {
            latitude: from.latitude,
            longitude: from.longitude,
        };
        let to_haversine = HaversineLocation {
            latitude: to.latitude,
            longitude: to.longitude,
        };
        distance(from_haversine, to_haversine, Units::Kilometers)
    }
}
