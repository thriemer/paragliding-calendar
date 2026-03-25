use anyhow::{Context, Result};
use cached::proc_macro::cached;
use serde::Deserialize;

use super::{Location, LocationProvider};

#[derive(Clone, Default)]
pub struct OpenMeteoLocationProvider;

impl OpenMeteoLocationProvider {
    pub fn new() -> Self {
        Self
    }
}

#[cached(
    time = 604800,
    result,
    key = "String",
    convert = r#"{ location_name.clone() }"#
)]
pub async fn geocode(location_name: String) -> Result<Vec<Location>> {
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=5&language=en&format=json",
        urlencoding::encode(&location_name)
    );

    let response = reqwest::get(url).await?;

    let openmeteo_response: GeocodingResponse = response
        .json()
        .await
        .with_context(|| "Failed to parse OpenMeteo geocoding response")?;

    let geocoding_results: Vec<Location> = openmeteo_response
        .results
        .unwrap_or_default()
        .into_iter()
        .map(|geocoding_result| geocoding_result.into())
        .collect();

    tracing::info!(
        "Geocoding found {} results for {}.",
        geocoding_results.len(),
        location_name
    );
    Ok(geocoding_results)
}

#[cached(
    time = 31536000,
    result,
    key = "String",
    convert = r#"{ format!("{}_{}", (latitude * 1000.0).round() / 1000.0, (longitude * 1000.0).round() / 1000.0) }"#
)]
pub async fn fetch_elevation(latitude: f64, longitude: f64) -> Result<f64> {
    let url = format!(
        "https://api.open-meteo.com/v1/elevation?latitude={}&longitude={}",
        latitude, longitude
    );

    let response = reqwest::get(&url).await?;

    let data: serde_json::Value = response.json().await?;

    let elevation = data["elevation"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_f64())
        .ok_or_else(|| anyhow::anyhow!("No elevation provided in response"))?;

    Ok(elevation)
}

impl LocationProvider for OpenMeteoLocationProvider {
    async fn geocode(&self, location_name: String) -> Result<Vec<Location>> {
        geocode(location_name).await
    }

    async fn fetch_elevation(&self, latitude: f64, longitude: f64) -> Result<f64> {
        fetch_elevation(latitude, longitude).await
    }
}

#[derive(Debug, Deserialize)]
struct GeocodingResponse {
    results: Option<Vec<GeocodingResult>>,
}

#[derive(Debug, Deserialize)]
struct GeocodingResult {
    name: String,
    latitude: f64,
    longitude: f64,
    country: Option<String>,
    admin1: Option<String>,
    admin2: Option<String>,
    timezone: Option<String>,
}

impl From<GeocodingResult> for Location {
    fn from(value: GeocodingResult) -> Self {
        Location {
            latitude: value.latitude,
            longitude: value.longitude,
            name: value.name,
            country: value.country.unwrap_or_else(|| "Unknown".to_string()),
        }
    }
}
