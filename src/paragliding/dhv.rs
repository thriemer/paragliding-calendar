use quick_xml::de::from_str;
use serde::Deserialize;
use std::fs;
use std::path::Path;
use tracing::{info, warn};

use super::ParaglidingSite;
use super::{Result, TravelAIError};

use crate::paragliding::Coordinates;
use crate::paragliding::DataSource;
use crate::paragliding::LaunchDirection;
use crate::paragliding::SiteCharacteristics;
use crate::paragliding::parse_direction_text_to_degrees;

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
            site_type: self.site_type.clone(),
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
    fn parse_launch_directions(&self) -> Vec<LaunchDirection> {
        match (&self.directions_text, &self.directions) {
            (Some(text), code) => {
                vec![LaunchDirection {
                    direction_code: code.clone(),
                    direction_text: text.clone(),
                    direction_degrees: parse_direction_text_to_degrees(text),
                }]
            }
            _ => vec![],
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_dhv_xml() {
        let xml_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<DhvXml Version="1.0" Timestamp="1764372633">
    <GeneratedAt>https://service.dhv.de/db2/geosearch.php</GeneratedAt>
    <GeneratedOn>2025-11-29T00:30:33+01:00</GeneratedOn>
    <Copyright>DHV - Deutscher Hängegleiterverband</Copyright>
    <CopyrightUrl>https://www.dhv.de/piloteninfos/gelaende-luftraum-natur/fluggelaende/nutzungsbedingungen-gelaendedatenbank/</CopyrightUrl>
    <FlyingSites>
        <FlyingSite>
            <SiteID>1</SiteID>
            <SiteName><![CDATA[Test Site]]></SiteName>
            <SiteCountry>DE</SiteCountry>
            <SiteType><![CDATA[Hanggelände für Gleitschirme]]></SiteType>
            <HeightDifferenceMax>100</HeightDifferenceMax>
            <SiteUrl><![CDATA[https://example.com]]></SiteUrl>
            <Location>
                <Coordinates>14.700136,50.998362</Coordinates>
                <Altitude>320</Altitude>
                <Directions>3B</Directions>
                <DirectionsText>O, W</DirectionsText>
                <AccessByCar>true</AccessByCar>
                <AccessByFoot>true</AccessByFoot>
                <AccessByPublicTransport>false</AccessByPublicTransport>
                <Hanggliding>true</Hanggliding>
                <Paragliding>true</Paragliding>
            </Location>
        </FlyingSite>
    </FlyingSites>
</DhvXml>"#;

        let sites = DHVParser::parse_xml(xml_content).unwrap();
        assert_eq!(sites.len(), 1);

        let site = &sites[0];
        assert_eq!(site.id, "dhv_1");
        assert_eq!(site.name, "Test Site");
        assert_eq!(site.coordinates.latitude, 50.998_362);
        assert_eq!(site.coordinates.longitude, 14.700_136);
        assert_eq!(site.elevation, Some(320.0));
        assert_eq!(site.launch_directions.len(), 1);
        assert_eq!(site.launch_directions[0].direction_text, "O, W");
        assert_eq!(
            site.launch_directions[0].direction_degrees,
            vec![90.0, 270.0]
        );
    }

    #[test]
    fn test_load_sites_from_file() {
        let xml_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<DhvXml Version="1.0" Timestamp="1764372633">
    <GeneratedAt>https://service.dhv.de/db2/geosearch.php</GeneratedAt>
    <GeneratedOn>2025-11-29T00:30:33+01:00</GeneratedOn>
    <Copyright>DHV - Deutscher Hängegleiterverband</Copyright>
    <CopyrightUrl>https://www.dhv.de/piloteninfos/gelaende-luftraum-natur/fluggelaende/nutzungsbedingungen-gelaendedatenbank/</CopyrightUrl>
    <FlyingSites>
        <FlyingSite>
            <SiteID>2</SiteID>
            <SiteName><![CDATA[File Test Site]]></SiteName>
            <SiteCountry>CH</SiteCountry>
            <SiteType><![CDATA[Berggelände]]></SiteType>
            <HeightDifferenceMax>500</HeightDifferenceMax>
            <Location>
                <Coordinates>7.5,46.5</Coordinates>
                <Altitude>1200</Altitude>
                <Directions>1234</Directions>
                <DirectionsText>N, E, S, W</DirectionsText>
                <AccessByCar>false</AccessByCar>
                <AccessByFoot>true</AccessByFoot>
                <Paragliding>true</Paragliding>
            </Location>
        </FlyingSite>
    </FlyingSites>
</DhvXml>"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(xml_content.as_bytes()).unwrap();

        let sites = DHVParser::load_sites(temp_file.path()).unwrap();
        assert_eq!(sites.len(), 1);

        let site = &sites[0];
        assert_eq!(site.id, "dhv_2");
        assert_eq!(site.name, "File Test Site");
        assert_eq!(site.country, Some("CH".to_string()));
    }

    #[test]
    fn test_file_not_found() {
        let result = DHVParser::load_sites("nonexistent_file.xml");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TravelAIError::FileNotFound(_)
        ));
    }
}
