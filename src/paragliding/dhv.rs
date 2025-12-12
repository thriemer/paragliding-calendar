use quick_xml::de::from_str;
use serde::Deserialize;
use std::fs;
use std::path::Path;
use tracing::{info, warn};

use super::ParaglidingSite;
use super::{Result, TravelAIError};

use crate::paragliding::sites::{Coordinates, DataSource, LaunchDirectionRange, SiteCharacteristics, SiteType};
use crate::paragliding::sites::parse_direction_text_to_degrees;

/// DHV XML parser and site loader
pub struct DHVParser;
/// DHV XML structure for deserialization
#[derive(Debug, Deserialize)]
pub struct DHVXml {
    #[serde(rename = "FlyingSites")]
    pub flying_sites: DHVFlyingSites,
}

#[derive(Debug, Deserialize)]
pub struct DHVFlyingSites {
    #[serde(rename = "FlyingSite")]
    pub sites: Vec<DHVFlyingSite>,
}

#[derive(Debug, Deserialize)]
pub struct DHVFlyingSite {
    #[serde(rename = "SiteID")]
    pub site_id: String,
    #[serde(rename = "SiteName")]
    pub site_name: String,
    #[serde(rename = "SiteCountry")]
    pub site_country: Option<String>,
    #[serde(rename = "SiteType")]
    pub site_type: Option<String>,
    #[serde(rename = "HeightDifferenceMax")]
    pub height_difference_max: Option<f64>,
    #[serde(rename = "SiteUrl")]
    pub site_url: Option<String>,
    #[serde(rename = "Location")]
    pub locations: Vec<DHVLocation>,
}

#[derive(Debug, Deserialize)]
pub struct DHVLocation {
    #[serde(rename = "LocationName")]
    pub location_name: Option<String>,
    #[serde(rename = "Coordinates")]
    pub coordinates: String, // Format: "longitude,latitude"
    #[serde(rename = "LocationType")]
    pub location_type: Option<u8>, // 1 = launch, 2 = landing
    #[serde(rename = "Altitude")]
    pub altitude: Option<f64>,
    #[serde(rename = "Directions")]
    pub directions: Option<String>,
    #[serde(rename = "DirectionsText")]
    pub directions_text: Option<String>,
    #[serde(rename = "TowingHeight1")]
    pub towing_height1: Option<f64>,
    #[serde(rename = "TowingHeight2")]
    pub towing_height2: Option<f64>,
    #[serde(rename = "TowingLength")]
    pub towing_length: Option<f64>,
    #[serde(rename = "AccessByCar")]
    pub access_by_car: Option<bool>,
    #[serde(rename = "AccessByFoot")]
    pub access_by_foot: Option<bool>,
    #[serde(rename = "AccessByPublicTransport")]
    pub access_by_public_transport: Option<bool>,
    #[serde(rename = "Hanggliding")]
    pub hanggliding: Option<bool>,
    #[serde(rename = "Paragliding")]
    pub paragliding: Option<bool>,
}

impl DHVFlyingSite {
    /// Convert DHV site to unified `ParaglidingSite`
    pub fn to_paragliding_site(&self) -> Result<ParaglidingSite> {
        // Find the launch location (LocationType = 1)
        let launch_location = self.locations.iter()
            .find(|loc| loc.location_type == Some(1))
            .or_else(|| self.locations.first()) // Fallback to first location if no launch type found
            .ok_or_else(|| TravelAIError::ParseError(format!(
                "No launch location found for site {}", self.site_id
            )))?;

        let coordinates = launch_location.parse_coordinates()?;
        let launch_directions = launch_location.parse_launch_directions();

        Ok(ParaglidingSite {
            id: format!("dhv_{}", self.site_id),
            name: self.site_name.clone(),
            coordinates,
            elevation: launch_location.altitude,
            launch_directions,
            site_type: {
                let has_towing = launch_location.towing_height1.unwrap_or(0.0) > 0.0
                    || launch_location.towing_height2.unwrap_or(0.0) > 0.0
                    || launch_location.towing_length.unwrap_or(0.0) > 0.0;
                
                if has_towing {
                    SiteType::Winch
                } else {
                    SiteType::Hang
                }
            },
            country: self.site_country.clone(),
            data_source: DataSource::DHV,
            characteristics: SiteCharacteristics {
                height_difference_max: self.height_difference_max,
                site_url: self.site_url.clone(),
                access_by_car: launch_location.access_by_car,
                access_by_foot: launch_location.access_by_foot,
                access_by_public_transport: launch_location.access_by_public_transport,
                hanggliding: launch_location.hanggliding,
                paragliding: launch_location.paragliding,
            },
        })
    }
}

impl DHVLocation {
    /// Parse DHV coordinate format "longitude,latitude" to Coordinates
    pub fn parse_coordinates(&self) -> Result<Coordinates> {
        let parts: Vec<&str> = self.coordinates.split(',').collect();
        if parts.len() != 2 {
            return Err(TravelAIError::ParseError(format!(
                "Invalid coordinate format: {}",
                self.coordinates
            )));
        }

        let longitude = parts[0]
            .trim()
            .parse::<f64>()
            .map_err(|_| TravelAIError::ParseError(format!("Invalid longitude: {}", parts[0])))?;

        let latitude = parts[1]
            .trim()
            .parse::<f64>()
            .map_err(|_| TravelAIError::ParseError(format!("Invalid latitude: {}", parts[1])))?;

        Ok(Coordinates {
            latitude,
            longitude,
        })
    }

    /// Parse launch directions from DHV format to unified format
    fn parse_launch_directions(&self) -> Vec<LaunchDirectionRange> {
        match &self.directions_text {
            Some(text) => {
                let text = text.trim();
                if text.is_empty() {
                    return vec![];
                }
                
                // Handle range formats like "SO-S" or "SSW-WSW"
                if text.contains('-') {
                    let parts: Vec<&str> = text.split('-').map(|s| s.trim()).collect();
                    if parts.len() == 2 {
                        let start_degrees = parse_direction_text_to_degrees(parts[0]);
                        let stop_degrees = parse_direction_text_to_degrees(parts[1]);
                        
                        if !start_degrees.is_empty() && !stop_degrees.is_empty() {
                            return vec![LaunchDirectionRange {
                                direction_degrees_start: start_degrees[0],
                                direction_degrees_stop: stop_degrees[0],
                            }];
                        }
                    }
                }
                
                // Handle multiple directions separated by comma or space
                if text.contains(',') || text.contains(' ') {
                    let directions = text.split(&[',', ' '][..])
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>();
                    
                    let mut ranges = Vec::new();
                    for dir in directions {
                        let degrees = parse_direction_text_to_degrees(dir);
                        if !degrees.is_empty() {
                            // Create 45-degree range around each direction
                            ranges.push(LaunchDirectionRange {
                                direction_degrees_start: (degrees[0] - 22.5).rem_euclid(360.0),
                                direction_degrees_stop: (degrees[0] + 22.5).rem_euclid(360.0),
                            });
                        }
                    }
                    return ranges;
                }
                
                // Handle single direction
                let degrees = parse_direction_text_to_degrees(text);
                if !degrees.is_empty() {
                    vec![LaunchDirectionRange {
                        direction_degrees_start: (degrees[0] - 22.5).rem_euclid(360.0),
                        direction_degrees_stop: (degrees[0] + 22.5).rem_euclid(360.0),
                    }]
                } else {
                    vec![]
                }
            }
            None => vec![],
        }
    }
}

impl DHVParser {
    /// Load and parse DHV XML file
    pub fn load_sites<P: AsRef<Path>>(xml_path: P) -> Result<Vec<ParaglidingSite>> {
        let xml_path = xml_path.as_ref();
        info!("Loading DHV sites from: {:?}", xml_path);

        if !xml_path.exists() {
            return Err(TravelAIError::FileNotFound(
                xml_path.to_string_lossy().to_string(),
            ));
        }

        let xml_content = fs::read_to_string(xml_path)
            .map_err(|e| TravelAIError::IoError(format!("Failed to read DHV XML file: {e}")))?;

        Self::parse_xml(&xml_content)
    }

    /// Parse DHV XML content
    pub fn parse_xml(xml_content: &str) -> Result<Vec<ParaglidingSite>> {
        info!("Parsing DHV XML content");

        let dhv_xml: DHVXml = from_str(xml_content)
            .map_err(|e| TravelAIError::ParseError(format!("Failed to parse DHV XML: {e}")))?;

        let mut sites = Vec::new();
        let mut parse_errors = 0;

        for dhv_site in dhv_xml.flying_sites.sites {
            match dhv_site.to_paragliding_site() {
                Ok(site) => sites.push(site),
                Err(e) => {
                    warn!("Failed to parse DHV site {}: {}", dhv_site.site_id, e);
                    parse_errors += 1;
                }
            }
        }

        info!(
            "Loaded {} sites from DHV XML ({} parse errors)",
            sites.len(),
            parse_errors
        );

        if sites.is_empty() && parse_errors > 0 {
            return Err(TravelAIError::ParseError(
                "No valid sites could be parsed from DHV XML".to_string(),
            ));
        }

        Ok(sites)
    }

    /// Get file modification time for cache validation
    pub fn get_file_mtime<P: AsRef<Path>>(xml_path: P) -> Result<std::time::SystemTime> {
        let metadata = fs::metadata(xml_path.as_ref())
            .map_err(|e| TravelAIError::IoError(format!("Failed to get file metadata: {e}")))?;

        metadata.modified().map_err(|e| {
            TravelAIError::IoError(format!("Failed to get file modification time: {e}"))
        })
    }
}

