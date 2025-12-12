use quick_xml::de::from_str;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tracing::info; // Add this crate for XML deserialization

use super::{Result, TravelAIError};
use crate::paragliding::sites::{
    Coordinates, DataSource, LaunchDirectionRange, ParaglidingSite, SiteCharacteristics, SiteType,
};

/// Paragliding Earth API client
pub struct ParaglidingEarthClient {
    client: Client,
    base_url: String,
}

/// Represents the main XML response container from the Paragliding Earth API[citation:1]
#[derive(Debug, Deserialize)]
#[serde(rename = "search")]
pub struct SearchResponse {
    #[serde(rename = "$value")]
    pub results: Vec<SiteResult>,
}

/// A single result in the API response, can be either a `takeoff` or `landing` element[citation:1]
#[derive(Debug, Deserialize)]
pub enum SiteResult {
    #[serde(rename = "takeoff")]
    Takeoff(Box<ParaglidingEarthSite>), // Boxed to avoid large enum variant issues
    #[serde(rename = "landing")]
    Landing(Box<LandingSite>), // You can handle landings separately if needed
}

/// Paragliding Earth API takeoff (launch site) structure[citation:1]
#[derive(Debug, Deserialize)]
pub struct ParaglidingEarthSite {
    #[serde(rename = "pge_site_id")]
    pub id: u64,
    pub name: String,
    pub lat: f64, // API uses 'lat', not 'latitude'
    pub lng: f64, // API uses 'lng', not 'longitude'
    #[serde(rename = "takeoff_altitude")]
    pub elevation: Option<f64>,
    pub countryCode: Option<String>, // 2-letter ISO code
    pub takeoff_description: Option<String>,
    pub paragliding: Option<u8>, // 1 or 0 in XML
    pub hanggliding: Option<u8>, // 1 or 0 in XML
    #[serde(rename = "pge_link")]
    pub url: Option<String>,

    // Wind orientations: 0=not suitable, 1=possible, 2=good[citation:1]
    pub orientations: Option<Orientations>,

    // Additional detailed fields (only available when style=detailled)[citation:1]
    pub flight_rules: Option<String>,
    pub going_there: Option<String>,
    pub comments: Option<String>,
    pub weather: Option<String>,
}

/// Wind orientation ratings for the 8 cardinal directions[citation:1]
#[derive(Debug, Deserialize)]
pub struct Orientations {
    pub N: Option<u8>,
    pub NE: Option<u8>,
    pub E: Option<u8>,
    pub SE: Option<u8>,
    pub S: Option<u8>,
    pub SW: Option<u8>,
    pub W: Option<u8>,
    pub NW: Option<u8>,
}

/// Landing site structure (simplified, based on API documentation)[citation:1]
#[derive(Debug, Deserialize)]
pub struct LandingSite {
    pub site: String,
    pub landing_name: String,
    pub landing_lat: f64,
    pub landing_lng: f64,
    pub landing_altitude: Option<f64>,
}

impl ParaglidingEarthClient {
    /// Create a new client
    #[must_use]
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("TravelAI/0.1.0")
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: "http://www.paraglidingearth.com/api".to_string(),
        }
    }

    /// Search for sites within radius of coordinates
    pub async fn search_sites(
        &self,
        center: &Coordinates,
        radius_km: f64,
    ) -> Result<Vec<ParaglidingSite>> {
        info!(
            "Searching Paragliding Earth sites within {}km of ({}, {})",
            radius_km, center.latitude, center.longitude
        );

        // CORRECTED URL: removed '/geojson/' from the path[citation:1]
        // Added 'xml' parameter to explicitly request XML format
        let url = format!(
            "{}/getAroundLatLngSites.php?lat={}&lng={}&distance={}&format=xml&style=detailled",
            self.base_url, center.latitude, center.longitude, radius_km
        );

        info!("Making request to: {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| TravelAIError::NetworkError(format!("API request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();

            return Err(TravelAIError::ApiError(format!(
                "Paragliding Earth API error {status}: {error_text}"
            )));
        }

        // Get response as text for XML parsing
        let xml_text = response.text().await.map_err(|e| {
            TravelAIError::ParseError(format!("Failed to read API response text: {e}"))
        })?;

        // Check if we got a valid response
        if xml_text.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Parse the XML response
        let sites = self.parse_xml_sites(&xml_text)?;

        info!("Found {} sites from Paragliding Earth API", sites.len());
        Ok(sites)
    }

    /// Parse XML response from Paragliding Earth API
    fn parse_xml_sites(&self, xml_text: &str) -> Result<Vec<ParaglidingSite>> {
        // Parse the full XML response
        let search_response: SearchResponse = from_str(xml_text)
            .map_err(|e| TravelAIError::ParseError(format!("Failed to parse XML: {e}")))?;

        let mut sites = Vec::new();

        // Extract only the takeoff (launch) sites from the response
        for result in search_response.results {
            match result {
                SiteResult::Takeoff(site_box) => {
                    let site = *site_box;
                    if let Some(converted) = self.convert_paragliding_earth_site(site) {
                        sites.push(converted);
                    }
                }
                SiteResult::Landing(_) => {
                    // Ignore landing sites for now, or handle them separately
                    continue;
                }
            }
        }

        Ok(sites)
    }

    /// Convert ParaglidingEarthSite to your application's ParaglidingSite
    fn convert_paragliding_earth_site(
        &self,
        pe_site: ParaglidingEarthSite,
    ) -> Option<ParaglidingSite> {
        // Only include sites suitable for paragliding
        if pe_site.paragliding != Some(1) {
            return None;
        }

        let launch_directions = self.convert_orientations(&pe_site.orientations);

        Some(ParaglidingSite {
            id: format!("pe_{}", pe_site.id),
            name: pe_site.name,
            coordinates: Coordinates {
                latitude: pe_site.lat,
                longitude: pe_site.lng,
            },
            elevation: pe_site.elevation,
            launch_directions,
            site_type: SiteType::Hang, // Default to Hang site since PE API doesn't specify type
            country: pe_site.countryCode,
            data_source: DataSource::ParaglidingEarth,
            characteristics: SiteCharacteristics {
                height_difference_max: None,
                site_url: pe_site.url,
                access_by_car: None, // Could parse from going_there or comments
                access_by_foot: None,
                access_by_public_transport: None,
                hanggliding: pe_site.hanggliding.map(|v| v == 1),
                paragliding: pe_site.paragliding.map(|v| v == 1),
            },
        })
    }

    /// Convert orientation ratings to LaunchDirection objects
    /// Ratings: 0=not suitable, 1=possible, 2=good[citation:1]
    fn convert_orientations(&self, orientations: &Option<Orientations>) -> Vec<LaunchDirectionRange> {
        let mut directions = Vec::new();

        if let Some(orient) = orientations {
            // Map each direction with rating >= 1 (possible or good)
            let direction_map = vec![
                ("N", 0.0, orient.N),
                ("NE", 45.0, orient.NE),
                ("E", 90.0, orient.E),
                ("SE", 135.0, orient.SE),
                ("S", 180.0, orient.S),
                ("SW", 225.0, orient.SW),
                ("W", 270.0, orient.W),
                ("NW", 315.0, orient.NW),
            ];

            for (name, degrees, rating) in direction_map {
                if let Some(r) = rating {
                    if r >= 1 {
                        // Include both "possible" (1) and "good" (2)
                        directions.push(LaunchDirectionRange {
                            direction_degrees_start: (degrees - 22.5_f64).rem_euclid(360.0),
                            direction_degrees_stop: (degrees + 22.5_f64).rem_euclid(360.0),
                        });
                    }
                }
            }
        }

        // Fallback: if no specific orientations, use common directions
        if directions.is_empty() {
            directions = vec![
                LaunchDirectionRange {
                    direction_degrees_start: 337.5, // N range (337.5-22.5)
                    direction_degrees_stop: 22.5,
                },
                LaunchDirectionRange {
                    direction_degrees_start: 67.5, // E range (67.5-112.5)
                    direction_degrees_stop: 112.5,
                },
                LaunchDirectionRange {
                    direction_degrees_start: 157.5, // S range (157.5-202.5)
                    direction_degrees_stop: 202.5,
                },
                LaunchDirectionRange {
                    direction_degrees_start: 247.5, // W range (247.5-292.5)
                    direction_degrees_stop: 292.5,
                },
            ];
        }

        directions
    }
}

