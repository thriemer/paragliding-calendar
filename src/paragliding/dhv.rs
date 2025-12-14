use anyhow::Result;
use quick_xml::de::from_str;
use serde::Deserialize;
use std::path::Path;
use std::{collections::HashMap, fs};

use crate::models::{Location, ParaglidingLanding, ParaglidingLaunch, ParaglidingSite, SiteType};

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

impl DHVLocation {
    fn is_launch(&self) -> bool {
        if let Some(site_type) = self.location_type
            && site_type == 1
        {
            true
        } else {
            false
        }
    }

    fn get_type(&self) -> SiteType {
        if let Some(length) = self.towing_length
            && length > 0.0
        {
            SiteType::Winch
        } else {
            SiteType::Hang
        }
    }
    pub fn get_location(&self, country: String) -> Result<Location, String> {
        let parts: Vec<&str> = self.coordinates.split(',').collect();

        if parts.len() != 2 {
            return Err(format!(
                "Expected format 'longitude,latitude', got '{}'",
                self.coordinates
            ));
        }

        let longitude = parts[0]
            .trim()
            .parse::<f64>()
            .map_err(|e| format!("Invalid longitude '{}': {}", parts[0], e))?;

        let latitude = parts[1]
            .trim()
            .parse::<f64>()
            .map_err(|e| format!("Invalid latitude '{}': {}", parts[1], e))?;

        Ok(Location {
            latitude,
            longitude,
            name: self.location_name.clone().unwrap(),
            country,
        })
    }

    fn get_launch_ranges(&self) -> Vec<(f64, f64)> {
        let text = self.directions_text.clone().unwrap();
        if text.is_empty() {
            return vec![];
        }
        if text.contains(',') && text.contains(',') {
            return text
                .split(',')
                .filter(|t| !t.trim().is_empty())
                .map(|s| Self::get_launch_range(s))
                .collect();
        }
        return vec![Self::get_launch_range(&text)];
    }

    fn get_launch_range(text: &str) -> (f64, f64) {
        let text = text.trim();
        // Handle range formats like "SO-S" or "SSW-WSW"
        if text.contains('-') {
            let parts: Vec<&str> = text.split('-').map(|s| s.trim()).collect();
            if parts.len() == 2 {
                let start_degrees = parse_direction_text_to_degrees(parts[0]);
                let stop_degrees = parse_direction_text_to_degrees(parts[1]);

                return (start_degrees, stop_degrees);
            }
        }

        // Handle multiple directions separated by comma or space
        // TODO: this is potentially very wrong
        if text.contains(',') || text.contains(' ') {
            let directions = text
                .split(&[',', ' '][..])
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|dir| parse_direction_text_to_degrees(dir))
                .collect::<Vec<_>>();
            let start = directions.iter().cloned().fold(f64::NAN, f64::min);
            let finish = directions.iter().cloned().fold(f64::NAN, f64::max);
            return (start, finish);
        }

        // Handle single direction
        let degrees = parse_direction_text_to_degrees(text);
        return (
            (degrees - 11.25).rem_euclid(360.0),
            (degrees + 11.25).rem_euclid(360.0),
        );
    }
}

fn parse_direction_text_to_degrees(text: &str) -> f64 {
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
        // german version
        ("NNO", 22.5),
        ("NO", 45.0),
        ("ONO", 67.5),
        ("O", 90.0),
        ("OSO", 112.5),
        ("SO", 135.0),
        ("SSO", 157.5),
        // german version end
        ("S", 180.0),
        ("SSW", 202.5),
        ("SW", 225.0),
        ("WSW", 247.5),
        ("W", 270.0),
        ("WNW", 292.5),
        ("NW", 315.0),
        ("NNW", 337.5),
    ]
    .iter()
    .copied()
    .collect();

    if let Some(deg) = direction_map.get(text.trim()) {
        return *deg;
    } else {
        println!(
            "Cannot find direction for text {}, contains - {}, contains: , {}",
            text,
            text.contains('-'),
            text.contains(',')
        );
        return 0.0;
    }
}

pub fn load_sites<T: AsRef<Path>>(xml_path: T) -> Vec<ParaglidingSite> {
    let xml_path = xml_path.as_ref();
    let xml_content = fs::read_to_string(xml_path).unwrap();
    let dhv_xml: DHVXml = from_str(&xml_content).unwrap();
    dhv_xml
        .flying_sites
        .sites
        .into_iter()
        .map(|dhv| dhv.into())
        .collect()
}

impl From<DHVFlyingSite> for ParaglidingSite {
    fn from(value: DHVFlyingSite) -> Self {
        let country = value.site_country.clone().unwrap();
        let launches = value
            .locations
            .iter()
            .filter(|site| site.is_launch())
            .flat_map(|launch| {
                let ranges = launch.get_launch_ranges();
                ranges.iter().map(|r| ParaglidingLaunch {
                    site_type: launch.get_type(),
                    location: launch.get_location(country.clone()).unwrap(),
                    direction_degrees_start: r.0,
                    direction_degrees_stop: r.1,
                    elevation: launch.altitude.unwrap(),
                }).collect::<Vec<ParaglidingLaunch>>()
            })
            .collect();

        let landings = value
            .locations
            .iter()
            .filter(|site| !site.is_launch())
            .map(|landing| ParaglidingLanding {
                location: landing.get_location(country.clone()).unwrap(),
                elevation: landing.altitude.unwrap(),
            })
            .collect();

        ParaglidingSite {
            name: value.site_name,
            launches,
            landings,
            country: value.site_country,
            data_source: "DHV".into(),
        }
    }
}
