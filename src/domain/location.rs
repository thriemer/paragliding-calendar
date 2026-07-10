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

    pub fn to_key(&self) -> String {
        let lat = (self.latitude * 1_000_000.0).round() as i64;
        let lon = (self.longitude * 1_000_000.0).round() as i64;
        format!("{}_{}_{}_{}", lat, lon, self.name, self.country)
    }

    /// Snap to a coarse lat/lon grid so nearby points collapse to one cache key. Name/country are
    /// cleared so `to_key()` keys purely on the cell — used for tour weather, where ~10 km accuracy
    /// is plenty and per-tour forecasts would otherwise be one API call each.
    pub fn snapped_to_grid(&self, grid_deg: f64) -> Location {
        let snap = |v: f64| (v / grid_deg).round() * grid_deg;
        Location::new(
            snap(self.latitude),
            snap(self.longitude),
            String::new(),
            String::new(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_to_same_location_is_zero() {
        let a = Location::new(50.7, 13.0, "A".into(), "DE".into());
        assert_eq!(a.distance_to(&a), 0.0);
    }

    #[test]
    fn distance_to_berlin_munich_within_one_percent() {
        let berlin = Location::new(52.520, 13.405, "Berlin".into(), "DE".into());
        let munich = Location::new(48.137, 11.575, "Munich".into(), "DE".into());
        let km = berlin.distance_to(&munich);
        let expected_km = 504.0;
        assert!(
            (km - expected_km).abs() / expected_km < 0.01,
            "expected ~{expected_km} km, got {km} km",
        );
    }

    #[test]
    fn to_key_uses_micro_degree_precision_in_expected_format() {
        let a = Location::new(50.7, 13.0, "Test".into(), "DE".into());
        assert_eq!(a.to_key(), "50700000_13000000_Test_DE");
    }

    #[test]
    fn to_key_distinguishes_distant_locations() {
        let a = Location::new(50.7, 13.0, "A".into(), "DE".into());
        let b = Location::new(50.71, 13.0, "A".into(), "DE".into());
        assert_ne!(a.to_key(), b.to_key());
    }

    #[test]
    fn snapped_to_grid_collapses_nearby_points_and_clears_name() {
        // Two tours ~2 km apart in the same cell, with different names, snap to one cache key.
        let a = Location::new(50.72, 13.03, "Tour A".into(), "DE".into());
        let b = Location::new(50.74, 13.01, "Tour B".into(), "DE".into());
        assert_eq!(
            a.snapped_to_grid(0.1).to_key(),
            b.snapped_to_grid(0.1).to_key()
        );
        // A distant point still lands in a different cell.
        let c = Location::new(51.24, 13.0, "Tour C".into(), "DE".into());
        assert_ne!(
            a.snapped_to_grid(0.1).to_key(),
            c.snapped_to_grid(0.1).to_key()
        );
    }
}
