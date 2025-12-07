//! Paragliding site data types and functionality
//!
//! This module provides the core data structures for representing paragliding sites
//! and utilities for working with geographic coordinates and site search.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a paragliding site from any data source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParaglidingSite {
    pub id: String,
    pub name: String,
    pub coordinates: Coordinates,
    pub elevation: Option<f64>,
    pub launch_directions: Vec<LaunchDirection>,
    pub site_type: Option<String>,
    pub country: Option<String>,
    pub data_source: DataSource,
    pub characteristics: SiteCharacteristics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coordinates {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchDirection {
    pub direction_code: Option<String>, // DHV specific codes like "3B", "89A"
    pub direction_text: String,         // Human readable like "O, W" or "SSW-WSW"
    pub direction_degrees: Vec<f64>,    // Converted to compass degrees
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataSource {
    DHV,
    ParaglidingEarth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteCharacteristics {
    pub height_difference_max: Option<f64>,
    pub site_url: Option<String>,
    pub access_by_car: Option<bool>,
    pub access_by_foot: Option<bool>,
    pub access_by_public_transport: Option<bool>,
    pub hanggliding: Option<bool>,
    pub paragliding: Option<bool>,
}

/// Convert direction text like "O, W" or "SSW-WSW" to compass degrees
pub fn parse_direction_text_to_degrees(text: &str) -> Vec<f64> {
    let mut degrees = Vec::new();

    // Direction mappings
    let direction_map: HashMap<&str, f64> = [
        ("N", 0.0),
        ("NNE", 22.5),
        ("NE", 45.0),
        ("ENE", 67.5),
        ("E", 90.0),
        ("ESE", 112.5),
        ("SE", 135.0),
        ("SSE", 157.5),
        ("S", 180.0),
        ("SSW", 202.5),
        ("SW", 225.0),
        ("WSW", 247.5),
        ("W", 270.0),
        ("WNW", 292.5),
        ("NW", 315.0),
        ("NNW", 337.5),
        ("O", 90.0), // German "Ost" = East
    ]
    .iter()
    .copied()
    .collect();

    // Split on common separators and parse each direction
    for part in text.split(&[',', '-', ' '][..]) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        if let Some(&deg) = direction_map.get(part) {
            degrees.push(deg);
        }
    }

    degrees
}

/// Geographic search functionality
pub struct GeographicSearch;

impl GeographicSearch {
    /// Find sites within radius (km) of a location
    #[must_use] 
    pub fn sites_within_radius<'a>(
        sites: &'a [ParaglidingSite],
        center: &Coordinates,
        radius_km: f64,
    ) -> Vec<&'a ParaglidingSite> {
        sites
            .iter()
            .filter(|site| {
                let distance = haversine::distance(
                    haversine::Location {
                        latitude: center.latitude,
                        longitude: center.longitude,
                    },
                    haversine::Location {
                        latitude: site.coordinates.latitude,
                        longitude: site.coordinates.longitude,
                    },
                    haversine::Units::Kilometers,
                );
                distance <= radius_km
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_direction_text() {
        let degrees = parse_direction_text_to_degrees("O, W");
        assert_eq!(degrees, vec![90.0, 270.0]);

        let degrees = parse_direction_text_to_degrees("SSW-WSW");
        assert_eq!(degrees, vec![202.5, 247.5]);
    }

    #[test]
    fn test_geographic_search() {
        let sites = vec![
            ParaglidingSite {
                id: "test1".to_string(),
                name: "Near Site".to_string(),
                coordinates: Coordinates {
                    latitude: 45.0,
                    longitude: 6.0,
                },
                elevation: Some(1000.0),
                launch_directions: vec![],
                site_type: None,
                country: None,
                data_source: DataSource::DHV,
                characteristics: SiteCharacteristics {
                    height_difference_max: None,
                    site_url: None,
                    access_by_car: None,
                    access_by_foot: None,
                    access_by_public_transport: None,
                    hanggliding: None,
                    paragliding: None,
                },
            },
            ParaglidingSite {
                id: "test2".to_string(),
                name: "Far Site".to_string(),
                coordinates: Coordinates {
                    latitude: 46.0,
                    longitude: 7.0,
                },
                elevation: Some(1500.0),
                launch_directions: vec![],
                site_type: None,
                country: None,
                data_source: DataSource::DHV,
                characteristics: SiteCharacteristics {
                    height_difference_max: None,
                    site_url: None,
                    access_by_car: None,
                    access_by_foot: None,
                    access_by_public_transport: None,
                    hanggliding: None,
                    paragliding: None,
                },
            },
        ];

        let center = Coordinates {
            latitude: 45.0,
            longitude: 6.0,
        };
        let nearby = GeographicSearch::sites_within_radius(&sites, &center, 50.0);
        assert_eq!(nearby.len(), 1);
        assert_eq!(nearby[0].name, "Near Site");
    }
}