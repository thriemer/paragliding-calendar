//! Location model for geographic coordinates and metadata

use serde::{Deserialize, Serialize};

/// Location coordinates
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Location {
    /// Latitude in decimal degrees
    pub latitude: f64,
    /// Longitude in decimal degrees  
    pub longitude: f64,
    /// Location name (city, region, etc.)
    pub name: String,
    /// Country code (ISO 3166-1 alpha-2)
    pub country: Option<String>,
}

impl Location {
    /// Create a new location
    #[must_use] 
    pub fn new(latitude: f64, longitude: f64, name: String) -> Self {
        Self {
            latitude,
            longitude,
            name,
            country: None,
        }
    }

    /// Create location with country
    #[must_use] 
    pub fn with_country(latitude: f64, longitude: f64, name: String, country: String) -> Self {
        Self {
            latitude,
            longitude,
            name,
            country: Some(country),
        }
    }

    /// Format location as coordinates string
    #[must_use] 
    pub fn format_coordinates(&self) -> String {
        format!("{:.4}, {:.4}", self.latitude, self.longitude)
    }

    /// Round coordinates for cache key generation
    #[must_use] 
    pub fn rounded_coordinates(&self, precision: u32) -> (f64, f64) {
        let multiplier = 10_f64.powi(i32::try_from(precision).unwrap_or(4));
        let lat = (self.latitude * multiplier).round() / multiplier;
        let lon = (self.longitude * multiplier).round() / multiplier;
        (lat, lon)
    }

    /// Generate cache key for this location
    #[must_use] 
    pub fn cache_key(&self, date: &str) -> String {
        let (lat, lon) = self.rounded_coordinates(2); // Round to 2 decimal places
        format!("weather:{lat:.2}:{lon:.2}:{date}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_location_cache_key() {
        let location = Location::new(46.8182, 8.2275, "Interlaken".to_string());
        let key = location.cache_key("2023-12-01");
        assert_eq!(key, "weather:46.82:8.23:2023-12-01");
    }

    #[test]
    fn test_location_rounded_coordinates() {
        let location = Location::new(46.818_234, 8.227_456, "Test".to_string());
        let (lat, lon) = location.rounded_coordinates(2);
        assert_eq!(lat, 46.82);
        assert_eq!(lon, 8.23);
    }
}