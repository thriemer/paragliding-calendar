use std::{collections::HashMap, fs, path::PathBuf};

use anyhow::Result;
use quick_xml::de::from_str;
use serde::Deserialize;
use tracing;

use crate::domain::{
    location::Location,
    paragliding::{
        ParaglidingLanding, ParaglidingLaunch, ParaglidingSite, ParaglidingSiteProvider, SiteType,
    },
};
use tracing::instrument;

pub struct DhvParaglidingSiteProvider {
    sites: Vec<ParaglidingSite>,
}

impl DhvParaglidingSiteProvider {
    #[instrument(skip_all)]
    pub fn new(dir: PathBuf) -> anyhow::Result<Self> {
        let paths = fs::read_dir(&dir)?;
        let sites: Vec<ParaglidingSite> = paths
            .filter_map(|p| {
                let path = match p {
                    Ok(path) => path,
                    Err(err) => {
                        tracing::warn!(
                            "Error while reading directory {:?}. load file. {:?}",
                            dir,
                            err
                        );
                        return None;
                    }
                };

                let dhv_sites: anyhow::Result<Vec<ParaglidingSite>> = load_sites(path.path());
                match dhv_sites {
                    Ok(sites) => Some(sites),
                    Err(err) => {
                        tracing::warn!("Error while loading flying sites. {:?}", err);
                        None
                    }
                }
            })
            .flatten()
            .collect();
        tracing::info!("Loaded {} flying sites.", sites.len());
        Ok(DhvParaglidingSiteProvider { sites })
    }
}

fn load_sites(xml_path: PathBuf) -> anyhow::Result<Vec<ParaglidingSite>> {
    let xml_content = fs::read_to_string(xml_path)?;
    parse_sites_from_xml(&xml_content)
}

pub fn parse_sites_from_xml(xml_content: &str) -> anyhow::Result<Vec<ParaglidingSite>> {
    let dhv_xml: DHVXml = from_str(xml_content)?;
    let sites: Vec<ParaglidingSite> = dhv_xml
        .flying_sites
        .sites
        .into_iter()
        .map(|dhv| dhv.into())
        .collect();
    Ok(sites)
}

impl ParaglidingSiteProvider for DhvParaglidingSiteProvider {
    #[instrument(skip_all, fields(center_lat = %center.latitude, center_lon = %center.longitude, radius_km = radius_km))]
    async fn fetch_launches_within_radius(
        &self,
        center: &Location,
        radius_km: f64,
    ) -> Vec<(ParaglidingSite, f64)> {
        let mut results = Vec::new();

        for site in &self.sites {
            // Find the closest launch to the center point
            let mut min_distance = f64::INFINITY;

            for launch in &site.launches {
                let distance = center.distance_to(&launch.location);
                if distance < min_distance {
                    min_distance = distance;
                }
            }

            // Include site if any launch is within radius
            if min_distance <= radius_km {
                results.push((site.clone(), min_distance));
            }
        }

        // Sort by distance (closest first)
        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        results
    }

    async fn fetch_all_sites(&self) -> Vec<ParaglidingSite> {
        self.sites.clone()
    }
}

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
            name: self.location_name.clone().unwrap_or_default(),
            country,
        })
    }

    fn get_launch_ranges(&self) -> Vec<(f64, f64)> {
        let Some(text) = self.directions_text.as_deref() else {
            return vec![];
        };
        if text.trim().is_empty() {
            return vec![];
        }
        if text.contains(',') {
            return text
                .split(',')
                .filter(|t| !t.trim().is_empty())
                .filter_map(Self::get_launch_range)
                .collect();
        }
        Self::get_launch_range(text).into_iter().collect()
    }

    fn get_launch_range(text: &str) -> Option<(f64, f64)> {
        let text = text.trim();

        // "SO-S" or "SSW-WSW"
        if let Some((a, b)) = text.split_once('-') {
            let start = parse_direction_text_to_degrees(a.trim())?;
            let stop = parse_direction_text_to_degrees(b.trim())?;
            return Some((start, stop));
        }

        // Single direction — bracket it with ±11.25° (half a 16-point sector).
        let degrees = parse_direction_text_to_degrees(text)?;
        Some((
            (degrees - 11.25).rem_euclid(360.0),
            (degrees + 11.25).rem_euclid(360.0),
        ))
    }
}

fn parse_direction_text_to_degrees(text: &str) -> Option<f64> {
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

    let trimmed = text.trim();
    match direction_map.get(trimmed) {
        Some(deg) => Some(*deg),
        None => {
            tracing::warn!(direction = trimmed, "skipping unknown compass direction");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("N", 0.0)]
    #[case("E", 90.0)]
    #[case("S", 180.0)]
    #[case("W", 270.0)]
    #[case("NE", 45.0)]
    #[case("NO", 45.0)]
    #[case("SO", 135.0)]
    #[case("SSO", 157.5)]
    fn direction_text_handles_english_and_german(#[case] text: &str, #[case] expected: f64) {
        assert_eq!(parse_direction_text_to_degrees(text), Some(expected));
    }

    #[test]
    fn unknown_direction_text_returns_none_so_caller_can_skip() {
        assert!(parse_direction_text_to_degrees("XYZ").is_none());
    }

    #[test]
    fn launch_range_with_unknown_direction_is_dropped() {
        let loc = location_with_text("XYZ-S");
        let ranges = loc.get_launch_ranges();
        assert!(ranges.is_empty(), "unknown directions should be skipped, not become north");
    }

    fn location_with_text(text: &str) -> DHVLocation {
        DHVLocation {
            location_name: Some("L".into()),
            coordinates: "13.0,50.0".into(),
            location_type: Some(1),
            altitude: Some(500.0),
            directions: None,
            directions_text: Some(text.into()),
            towing_height1: None,
            towing_height2: None,
            towing_length: None,
            access_by_car: None,
            access_by_foot: None,
            access_by_public_transport: None,
            hanggliding: None,
            paragliding: Some(true),
        }
    }

    #[test]
    fn launch_range_from_dash_separated_pair() {
        let loc = location_with_text("SO-S");
        let ranges = loc.get_launch_ranges();
        assert_eq!(ranges, vec![(135.0, 180.0)]);
    }

    #[test]
    fn launch_range_single_direction_brackets_eleven_degrees_each_side() {
        let loc = location_with_text("N");
        let ranges = loc.get_launch_ranges();
        assert_eq!(ranges.len(), 1);
        let (start, stop) = ranges[0];
        assert_eq!(start, 348.75);
        assert_eq!(stop, 11.25);
    }

    #[test]
    fn launch_range_comma_separated_emits_multiple_ranges() {
        let loc = location_with_text("SO-S, W-NW");
        let ranges = loc.get_launch_ranges();
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0], (135.0, 180.0));
        assert_eq!(ranges[1], (270.0, 315.0));
    }

    #[test]
    fn get_location_parses_lon_lat_in_dhv_order() {
        let loc = location_with_text("N");
        let parsed = loc.get_location("DE".into()).unwrap();
        assert_eq!(parsed.longitude, 13.0);
        assert_eq!(parsed.latitude, 50.0);
        assert_eq!(parsed.country, "DE");
    }

    #[test]
    fn parse_sites_from_xml_maps_minimal_site() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<DHVXml>
    <FlyingSites>
        <FlyingSite>
            <SiteID>1</SiteID>
            <SiteName>Test Hill</SiteName>
            <SiteCountry>DE</SiteCountry>
            <Location>
                <LocationName>Launch</LocationName>
                <Coordinates>13.0,50.0</Coordinates>
                <LocationType>1</LocationType>
                <Altitude>500.0</Altitude>
                <DirectionsText>SO-S</DirectionsText>
            </Location>
        </FlyingSite>
    </FlyingSites>
</DHVXml>"#;
        let sites = parse_sites_from_xml(xml).unwrap();
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].name, "Test Hill");
        assert_eq!(sites[0].country.as_deref(), Some("DE"));
        assert_eq!(sites[0].launches.len(), 1);
        let launch = &sites[0].launches[0];
        assert_eq!(launch.direction_degrees_start, 135.0);
        assert_eq!(launch.direction_degrees_stop, 180.0);
        assert_eq!(launch.elevation, 500.0);
    }
}

impl From<DHVFlyingSite> for ParaglidingSite {
    fn from(value: DHVFlyingSite) -> Self {
        let country = value.site_country.clone().unwrap_or_default();
        let launches = value
            .locations
            .iter()
            .filter(|site| site.is_launch())
            .flat_map(|launch| {
                let ranges = launch.get_launch_ranges();
                let location = match launch.get_location(country.clone()) {
                    Ok(loc) => loc,
                    Err(e) => {
                        tracing::warn!(site = %value.site_name, error = %e, "skipping launch with bad coordinates");
                        return Vec::new();
                    }
                };
                let elevation = launch.altitude.unwrap_or(0.0);
                ranges
                    .into_iter()
                    .map(|(start, stop)| ParaglidingLaunch {
                        site_type: launch.get_type(),
                        location: location.clone(),
                        direction_degrees_start: start,
                        direction_degrees_stop: stop,
                        elevation,
                    })
                    .collect()
            })
            .collect();

        let landings = value
            .locations
            .iter()
            .filter(|site| !site.is_launch())
            .filter_map(|landing| {
                let location = landing
                    .get_location(country.clone())
                    .map_err(|e| {
                        tracing::warn!(site = %value.site_name, error = %e, "skipping landing with bad coordinates");
                    })
                    .ok()?;
                Some(ParaglidingLanding {
                    location,
                    elevation: landing.altitude.unwrap_or(0.0),
                })
            })
            .collect();

        ParaglidingSite {
            name: value.site_name,
            launches,
            landings,
            country: value.site_country,
            data_source: "DHV".into(),
            parking_location: None,
            mute_alerts: None,
            rating: None,
            preferred_weather_model: None,
        }
    }
}
