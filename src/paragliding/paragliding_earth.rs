use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{info, warn, error};

use super::{Result, TravelAIError};
use crate::config::TravelAiConfig;
use super::{ParaglidingSite, Coordinates, LaunchDirection, DataSource, SiteCharacteristics};

/// Paragliding Earth API client
pub struct ParaglidingEarthClient {
    client: Client,
    api_key: Option<String>,
    base_url: String,
}

/// Paragliding Earth API site response
#[derive(Debug, Deserialize)]
pub struct ParaglidingEarthSite {
    pub id: u64,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation: Option<f64>,
    pub launch_directions: Option<String>,
    pub site_type: Option<String>,
    pub country: Option<String>,
    pub description: Option<String>,
    pub url: Option<String>,
}

/// Search response from Paragliding Earth API
#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub sites: Vec<ParaglidingEarthSite>,
    pub total_count: Option<u64>,
}

impl ParaglidingEarthClient {
    /// Create a new client
    pub fn new(config: &TravelAiConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("TravelAI/0.1.0")
            .build()
            .expect("Failed to create HTTP client");
            
        Self {
            client,
            api_key: config.paragliding.paragliding_earth_api_key.clone(),
            base_url: "https://paraglidingearth.com/api/v1".to_string(),
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
        
        let url = format!(
            "{}/sites/search?lat={}&lng={}&radius={}",
            self.base_url, center.latitude, center.longitude, radius_km
        );
        
        let mut request = self.client.get(&url);
        
        if let Some(api_key) = &self.api_key {
            request = request.header("Authorization", format!("Bearer {}", api_key));
        }
        
        let response = request
            .send()
            .await
            .map_err(|e| TravelAIError::NetworkError(format!("API request failed: {}", e)))?;
            
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            
            return match status.as_u16() {
                401 => Err(TravelAIError::AuthenticationError(
                    "Invalid or missing Paragliding Earth API key".to_string()
                )),
                429 => Err(TravelAIError::RateLimitError(
                    "Paragliding Earth API rate limit exceeded".to_string()
                )),
                _ => Err(TravelAIError::ApiError(
                    format!("Paragliding Earth API error {}: {}", status, error_text)
                )),
            };
        }
        
        let search_response: SearchResponse = response
            .json()
            .await
            .map_err(|e| TravelAIError::ParseError(
                format!("Failed to parse Paragliding Earth response: {}", e)
            ))?;
            
        let sites: Vec<ParaglidingSite> = search_response.sites
            .into_iter()
            .map(|site| site.to_paragliding_site())
            .collect();
            
        info!("Found {} sites from Paragliding Earth API", sites.len());
        Ok(sites)
    }
    
    /// Check if API is available and properly configured
    pub async fn health_check(&self) -> Result<()> {
        let url = format!("{}/health", self.base_url);
        
        let response = self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| TravelAIError::NetworkError(
                format!("Health check failed: {}", e)
            ))?;
            
        if response.status().is_success() {
            info!("Paragliding Earth API is available");
            Ok(())
        } else {
            Err(TravelAIError::ApiError(
                format!("Paragliding Earth API health check failed: {}", response.status())
            ))
        }
    }
}

impl ParaglidingEarthSite {
    /// Convert to unified ParaglidingSite
    pub fn to_paragliding_site(self) -> ParaglidingSite {
        let coordinates = Coordinates {
            latitude: self.latitude,
            longitude: self.longitude,
        };
        
        let launch_directions = self.launch_directions
            .map(|dirs| vec![LaunchDirection {
                direction_code: None,
                direction_text: dirs.clone(),
                direction_degrees: parse_paragliding_earth_directions(&dirs),
            }])
            .unwrap_or_default();
        
        ParaglidingSite {
            id: format!("pe_{}", self.id),
            name: self.name,
            coordinates,
            elevation: self.elevation,
            launch_directions,
            site_type: self.site_type,
            country: self.country,
            data_source: DataSource::ParaglidingEarth,
            characteristics: SiteCharacteristics {
                height_difference_max: None,
                site_url: self.url,
                access_by_car: None,
                access_by_foot: None,
                access_by_public_transport: None,
                hanggliding: None,
                paragliding: Some(true), // Assume all sites support paragliding
            },
        }
    }
}

/// Parse Paragliding Earth direction format to degrees
fn parse_paragliding_earth_directions(directions: &str) -> Vec<f64> {
    // This is a simplified parser - Paragliding Earth format may vary
    // Common formats might be "N, NE, E" or "270-360" or "N-E"
    let mut degrees = Vec::new();
    
    // Direction mappings
    let direction_map = [
        ("N", 0.0), ("NNE", 22.5), ("NE", 45.0), ("ENE", 67.5),
        ("E", 90.0), ("ESE", 112.5), ("SE", 135.0), ("SSE", 157.5),
        ("S", 180.0), ("SSW", 202.5), ("SW", 225.0), ("WSW", 247.5),
        ("W", 270.0), ("WNW", 292.5), ("NW", 315.0), ("NNW", 337.5),
    ];
    
    for part in directions.split(&[',', '-', ' ', ';'][..]) {
        let part = part.trim().to_uppercase();
        if part.is_empty() {
            continue;
        }
        
        // Try to find direct direction match
        for (dir, deg) in &direction_map {
            if part == *dir {
                degrees.push(*deg);
                break;
            }
        }
        
        // Try to parse as numeric degrees
        if let Ok(deg) = part.parse::<f64>() {
            if (0.0..=360.0).contains(&deg) {
                degrees.push(deg);
            }
        }
    }
    
    degrees
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TravelAiConfig;
    
    #[test]
    fn test_parse_paragliding_earth_directions() {
        let degrees = parse_paragliding_earth_directions("N, NE, E");
        assert_eq!(degrees, vec![0.0, 45.0, 90.0]);
        
        let degrees = parse_paragliding_earth_directions("270, 315, 0");
        assert_eq!(degrees, vec![270.0, 315.0, 0.0]);
        
        let degrees = parse_paragliding_earth_directions("SW-W");
        assert_eq!(degrees, vec![225.0, 270.0]);
    }
    
    #[test]
    fn test_paragliding_earth_site_conversion() {
        let pe_site = ParaglidingEarthSite {
            id: 123,
            name: "Test Site PE".to_string(),
            latitude: 45.8566,
            longitude: 6.8644,
            elevation: Some(1000.0),
            launch_directions: Some("N, E".to_string()),
            site_type: Some("Mountain".to_string()),
            country: Some("FR".to_string()),
            description: Some("Test description".to_string()),
            url: Some("https://example.com".to_string()),
        };
        
        let site = pe_site.to_paragliding_site();
        assert_eq!(site.id, "pe_123");
        assert_eq!(site.name, "Test Site PE");
        assert_eq!(site.coordinates.latitude, 45.8566);
        assert_eq!(site.coordinates.longitude, 6.8644);
        assert_eq!(site.elevation, Some(1000.0));
        assert_eq!(site.launch_directions.len(), 1);
        assert_eq!(site.launch_directions[0].direction_text, "N, E");
        assert_eq!(site.launch_directions[0].direction_degrees, vec![0.0, 90.0]);
        assert!(matches!(site.data_source, DataSource::ParaglidingEarth));
    }
    
    #[test]
    fn test_client_creation() {
        let config = TravelAiConfig::default();
        let client = ParaglidingEarthClient::new(&config);
        assert_eq!(client.base_url, "https://paraglidingearth.com/api/v1");
    }
}