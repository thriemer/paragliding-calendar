//! Location Resolution Module
//!
//! This module handles resolving location inputs (coordinates, names, postal codes)
//! into structured Location objects for paragliding forecasting.

use crate::models::Location;
use crate::{LocationInput, WeatherApiClient};
use anyhow::Result;
use tracing::debug;

/// Service for resolving location inputs
pub struct LocationResolver;

impl LocationResolver {
    /// Resolve a location input into a structured Location
    pub async fn resolve_location(
        api_client: &WeatherApiClient,
        location_input: LocationInput,
    ) -> Result<Location> {
        debug!("Resolving location input: {:?}", location_input);

        let location = match location_input {
            LocationInput::Coordinates(lat, lon) => {
                Self::resolve_coordinates(api_client, lat, lon).await?
            }
            LocationInput::Name(name) => {
                Self::resolve_name(api_client, name).await?
            }
            LocationInput::PostalCode(postal) => {
                Self::resolve_postal_code(api_client, postal).await?
            }
        };

        debug!(
            "Resolved location: {} at ({}, {})",
            location.name, location.latitude, location.longitude
        );

        Ok(location)
    }

    /// Resolve coordinates to a location with proper name via reverse geocoding
    async fn resolve_coordinates(
        api_client: &WeatherApiClient,
        lat: f64,
        lon: f64,
    ) -> Result<Location> {
        debug!("Resolving coordinates: ({}, {})", lat, lon);

        // Try reverse geocoding to get a proper name
        match api_client.reverse_geocode(lat, lon) {
            Ok(results) if !results.is_empty() => {
                let result = results.into_iter().next().unwrap();
                Ok(Location::from(result))
            }
            Ok(_) => {
                debug!("No reverse geocoding results found, using coordinates as name");
                Ok(Location::new(lat, lon, format!("{lat:.4}, {lon:.4}")))
            }
            Err(e) => {
                debug!("Reverse geocoding failed: {}, using coordinates as name", e);
                Ok(Location::new(lat, lon, format!("{lat:.4}, {lon:.4}")))
            }
        }
    }

    /// Resolve a location name to coordinates via geocoding
    async fn resolve_name(
        api_client: &WeatherApiClient,
        name: String,
    ) -> Result<Location> {
        debug!("Geocoding location name: {}", name);

        let geocoding_results = api_client.geocode(&name).await?;
        if geocoding_results.is_empty() {
            return Err(anyhow::anyhow!("Location not found: {}", name));
        }

        // Use the first (best) result
        let geocoding = geocoding_results.into_iter().next().unwrap();
        debug!(
            "Found location: {} ({:.4}, {:.4})",
            geocoding.name, geocoding.lat, geocoding.lon
        );

        Ok(Location::from(geocoding))
    }

    /// Resolve a postal code to coordinates via geocoding
    async fn resolve_postal_code(
        api_client: &WeatherApiClient,
        postal: String,
    ) -> Result<Location> {
        debug!("Geocoding postal code: {}", postal);

        let geocoding_results = api_client.geocode(&postal).await?;
        if geocoding_results.is_empty() {
            return Err(anyhow::anyhow!("Postal code not found: {}", postal));
        }

        // Use the first (best) result
        let geocoding = geocoding_results.into_iter().next().unwrap();
        debug!(
            "Found location for postal code {}: {} ({:.4}, {:.4})",
            postal, geocoding.name, geocoding.lat, geocoding.lon
        );

        Ok(Location::from(geocoding))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests would require a mock API client in a real test suite
    // For now, they demonstrate the expected behavior

    #[test]
    fn test_resolve_coordinates_fallback() {
        let lat = 46.8182;
        let lon = 8.2275;
        
        // This would normally create a mock API client
        // For now, just test the fallback location creation
        let location = Location::new(lat, lon, format!("{lat:.4}, {lon:.4}"));
        
        assert_eq!(location.latitude, lat);
        assert_eq!(location.longitude, lon);
        assert_eq!(location.name, "46.8182, 8.2275");
    }
}