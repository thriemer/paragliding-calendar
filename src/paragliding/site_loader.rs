//! Site Loading Module
//!
//! This module handles the loading and discovery of paragliding sites
//! from various data sources (DHV XML files, Paragliding Earth API, etc.).

use crate::config::TravelAiConfig;
use crate::models::Location;
use crate::paragliding::paragliding_earth::ParaglidingEarthClient;
use crate::paragliding::sites::{Coordinates, GeographicSearch, ParaglidingSite, SiteType};
use anyhow::Result;
use tracing::{debug, info, warn};

/// Service for loading and filtering paragliding sites
pub struct SiteLoader;

impl SiteLoader {
    /// Load paragliding sites within radius of a location
    pub async fn load_sites_in_area(
        location: &Location,
        radius_km: f64,
        config: Option<&TravelAiConfig>,
    ) -> Result<Vec<ParaglidingSite>> {
        info!(
            "Loading paragliding sites within {}km of {}",
            radius_km, location.name
        );

        let sites = Self::load_all_sites(location, radius_km, config).await?;
        let filtered_sites = Self::filter_sites_by_distance(&sites, location, radius_km);

        info!(
            "Found {} sites within {}km radius. \n Sites: {:#?}",
            filtered_sites.len(),
            radius_km,
            filtered_sites
        );

        Ok(filtered_sites)
    }

    /// Load all available sites from data sources
    async fn load_all_sites(
        location: &Location,
        radius_km: f64,
        config: Option<&TravelAiConfig>,
    ) -> Result<Vec<ParaglidingSite>> {
        let mut all_sites = Vec::new();

        // Load DHV XML sites
        let dhv_sites = Self::load_dhv_sites()?;
        all_sites.extend(dhv_sites);

        // Load Paragliding Earth sites (no API key required)
        let center = Coordinates {
            latitude: location.latitude,
            longitude: location.longitude,
        };

        if let Some(pe_sites) = Self::load_paragliding_earth_sites(&center, radius_km).await? {
            all_sites.extend(pe_sites);
        }

        debug!("Loaded {} sites from all data sources", all_sites.len());
        Ok(all_sites)
    }

    /// Load sites from DHV XML file
    fn load_dhv_sites() -> Result<Vec<ParaglidingSite>> {
        let dhv_file_path = "dhvgelaende_dhvxml_de.xml";

        let sites = if std::path::Path::new(dhv_file_path).exists() {
            debug!("Loading sites from DHV XML file: {}", dhv_file_path);
            crate::paragliding::dhv::DHVParser::load_sites(dhv_file_path)?
        } else {
            warn!(
                "DHV XML file not found at {}, skipping DHV sites",
                dhv_file_path
            );
            Vec::new()
        };

        debug!("Loaded {} sites from DHV XML", sites.len());
        Ok(sites)
    }

    /// Load sites from Paragliding Earth API
    async fn load_paragliding_earth_sites(
        center: &Coordinates,
        radius_km: f64,
    ) -> Result<Option<Vec<ParaglidingSite>>> {
        debug!("Loading sites from Paragliding Earth API (no API key required)");

        let client = ParaglidingEarthClient::new();
        match client.search_sites(center, radius_km).await {
            Ok(sites) => {
                debug!("Loaded {} sites from Paragliding Earth API", sites.len());
                Ok(Some(sites))
            }
            Err(e) => {
                warn!("Failed to load sites from Paragliding Earth API: {}", e);
                // Don't fail the entire operation, just skip PE sites
                Ok(None)
            }
        }
    }

    /// Filter sites by distance from a center location
    fn filter_sites_by_distance(
        sites: &[ParaglidingSite],
        center_location: &Location,
        radius_km: f64,
    ) -> Vec<ParaglidingSite> {
        let search_center = Coordinates {
            latitude: center_location.latitude,
            longitude: center_location.longitude,
        };

        let nearby_sites = GeographicSearch::sites_within_radius(sites, &search_center, radius_km);

        // Filter to only return Hang sites by default (exclude Winch sites)
        nearby_sites
            .into_iter()
            .filter(|site| matches!(site.site_type, SiteType::Hang))
            .cloned()
            .collect()
    }

    /// Get the distance from a location to a site in kilometers
    pub fn distance_to_site(location: &Location, site: &ParaglidingSite) -> f64 {
        haversine::distance(
            haversine::Location {
                latitude: location.latitude,
                longitude: location.longitude,
            },
            haversine::Location {
                latitude: site.coordinates.latitude,
                longitude: site.coordinates.longitude,
            },
            haversine::Units::Kilometers,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paragliding::sites::{DataSource, SiteCharacteristics};

    fn create_test_site(lat: f64, lon: f64, name: &str) -> ParaglidingSite {
        ParaglidingSite {
            id: format!("test_{}", name.to_lowercase().replace(' ', "_")),
            name: name.to_string(),
            coordinates: Coordinates {
                latitude: lat,
                longitude: lon,
            },
            elevation: Some(1000.0),
            launch_directions: vec![],
            site_type: SiteType::Hang,
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
        }
    }

    #[test]
    fn test_filter_sites_by_distance() {
        let center_location = Location::new(46.0, 8.0, "Test Center".to_string());

        let sites = vec![
            create_test_site(46.01, 8.01, "Near Site"), // ~1.5 km away
            create_test_site(46.5, 8.5, "Far Site"),    // ~78 km away
            create_test_site(45.99, 7.99, "Close Site"), // ~1.5 km away
        ];

        let filtered = SiteLoader::filter_sites_by_distance(&sites, &center_location, 50.0);

        // Should include near and close sites, but not far site
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().any(|s| s.name == "Near Site"));
        assert!(filtered.iter().any(|s| s.name == "Close Site"));
        assert!(!filtered.iter().any(|s| s.name == "Far Site"));
    }

    #[test]
    fn test_distance_to_site() {
        let location = Location::new(46.0, 8.0, "Test Location".to_string());
        let site = create_test_site(46.01, 8.01, "Test Site");

        let distance = SiteLoader::distance_to_site(&location, &site);

        // Should be approximately 1.5 km
        assert!(distance > 1.0 && distance < 2.0);
    }
}
