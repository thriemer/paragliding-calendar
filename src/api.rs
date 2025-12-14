//! Weather API client for `OpenMeteo` integration  
//!
//! This module provides HTTP client functionality for retrieving weather data
//! from the `OpenMeteo` API with rate limiting, retry logic, and error handling.
//! Previously integrated with `OpenWeatherMap`, now uses `OpenMeteo` for API-key-free access.

use crate::config::TravelAiConfig;
use crate::models::{Location, WeatherData, WeatherForecast};
use crate::{ErrorCode, TravelAiError};
use anyhow::{Context, Result};
use chrono::Utc;
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::{Level, debug, error, info, instrument, span, warn};


/// Weather API client for `OpenMeteo`
pub struct WeatherApiClient {
    /// HTTP client
    client: Client,
    /// API configuration
    config: TravelAiConfig,
}

impl WeatherApiClient {
    /// Create a new weather API client
    pub fn new(config: TravelAiConfig) -> Result<Self> {
        let timeout = Duration::from_secs(config.weather.timeout_seconds.into());

        let client = Client::builder()
            .timeout(timeout)
            .user_agent("TravelAI/0.1.0")
            .build()
            .with_context(|| "Failed to create HTTP client")?;

        Ok(Self {
            client,
            config,
        })
    }

    /// Get current weather for a location using `OpenMeteo` API
    #[instrument(skip(self), fields(lat, lon))]
    pub async fn get_current_weather(&self, lat: f64, lon: f64) -> Result<WeatherData> {
        let span = span!(Level::INFO, "get_current_weather", lat, lon);
        let _enter = span.enter();

        info!(
            "Getting current weather for coordinates: {:.4}, {:.4}",
            lat, lon
        );
        let start_time = Instant::now();

        // OpenMeteo API doesn't require API key, use forecast endpoint with current=true
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,windspeed_10m,winddirection_10m,windgusts_10m,precipitation,cloudcover,surface_pressure,visibility,weathercode&wind_speed_unit=ms"
        );

        debug!("OpenMeteo API request URL: {}", url);

        let response = self.make_request(&url).await?;

        let parse_start = Instant::now();
        let forecast_response: openmeteo::ForecastResponse = response
            .json().await
            .with_context(|| "Failed to parse OpenMeteo weather response")
            .map_err(|e| {
                error!("Failed to parse weather response: {}", e);
                TravelAiError::api_with_context(
                    "Invalid weather data received from OpenMeteo API",
                    ErrorCode::ApiInvalidResponse,
                    HashMap::from([("coordinates".to_string(), format!("{lat:.4},{lon:.4}"))]),
                )
            })?;

        let parse_duration = parse_start.elapsed();
        let total_duration = start_time.elapsed();

        info!(
            "Successfully retrieved current weather in {:.3}s (parse: {:.3}s)",
            total_duration.as_secs_f64(),
            parse_duration.as_secs_f64()
        );

        if total_duration.as_secs() > 5 {
            warn!(
                "Slow API response detected: {:.3}s",
                total_duration.as_secs_f64()
            );
        }

        // Extract current weather from OpenMeteo response
        if let Some(current) = &forecast_response.current {
            Ok(WeatherData {
                timestamp: Utc::now(),
                temperature: current.temperature,
                wind_speed: current.wind_speed,
                wind_direction: current.wind_direction,
                wind_gust: current.wind_gusts,
                precipitation: current.precipitation,
                cloud_cover: current.cloud_cover,
                pressure: current.pressure,
                visibility: current.visibility,
                description: openmeteo::weather_code_to_description(current.weather_code)
                    .to_string(),
                icon: None,
            })
        } else {
            Err(TravelAiError::api_with_context(
                "No current weather data available from OpenMeteo",
                ErrorCode::ApiInvalidResponse,
                HashMap::from([("coordinates".to_string(), format!("{lat:.4},{lon:.4}"))]),
            )
            .into())
        }
    }

    /// Get 7-day weather forecast for a location using `OpenMeteo` API
    #[instrument(skip(self), fields(lat, lon))]
    pub async fn get_forecast(&self, lat: f64, lon: f64) -> Result<WeatherForecast> {
        let span = span!(Level::INFO, "get_forecast", lat, lon);
        let _enter = span.enter();

        info!(
            "Getting 7-day forecast for coordinates: {:.4}, {:.4}",
            lat, lon
        );
        let start_time = Instant::now();

        // OpenMeteo API for hourly forecast data (7 days)
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&hourly=temperature_2m,windspeed_10m,winddirection_10m,windgusts_10m,precipitation,cloudcover,surface_pressure,visibility,weathercode&timezone=auto&forecast_days=7&wind_speed_unit=ms"
        );

        let response = self.make_request(&url).await?;
        debug!("API Response: {:?}", response);

        let parse_start = Instant::now();
        let forecast_response: openmeteo::ForecastResponse = response
            .json().await
            .with_context(|| "Failed to parse OpenMeteo forecast response")
            .map_err(|e| {
                error!("Failed to parse forecast response: {}", e);
                TravelAiError::api_with_context(
                    "Invalid forecast data received from OpenMeteo API",
                    ErrorCode::ApiInvalidResponse,
                    HashMap::from([("coordinates".to_string(), format!("{lat:.4},{lon:.4}"))]),
                )
            })?;

        let parse_duration = parse_start.elapsed();
        let total_duration = start_time.elapsed();

        // Create forecast using our OpenMeteo conversion method
        let location_name = format!("{lat:.4}, {lon:.4}"); // Default name, will be updated by geocoding
        let forecast = WeatherForecast::from_openmeteo(&forecast_response, location_name);

        info!(
            "Successfully retrieved forecast with {} data points in {:.3}s (parse: {:.3}s)",
            forecast.forecasts.len(),
            total_duration.as_secs_f64(),
            parse_duration.as_secs_f64()
        );
        debug!("Weather Forecst {:?}", forecast);

        if total_duration.as_secs() > 5 {
            warn!(
                "Slow forecast API response: {:.3}s",
                total_duration.as_secs_f64()
            );
        }

        Ok(forecast)
    }

    /// Get geocoding information for a location name using `OpenMeteo` API
    #[instrument(skip(self), fields(location = location_name))]
    pub async fn geocode(&self, location_name: &str) -> Result<Vec<GeocodingResult>> {
        let span = span!(Level::INFO, "geocode", location = location_name);
        let _enter = span.enter();

        info!("Geocoding location: '{}'", location_name);
        let start_time = Instant::now();

        // OpenMeteo geocoding API (no API key required)
        let url = format!(
            "https://geocoding-api.open-meteo.com/v1/search?name={}&count=5&language=en&format=json",
            urlencoding::encode(location_name)
        );

        let response = self.make_request(&url).await?;

        let parse_start = Instant::now();
        let openmeteo_response: openmeteo::GeocodingResponse = response
            .json().await
            .with_context(|| "Failed to parse OpenMeteo geocoding response")
            .map_err(|e| {
                error!(
                    "Failed to parse geocoding response for '{}': {}",
                    location_name, e
                );
                TravelAiError::api_with_context(
                    "Invalid geocoding data received from OpenMeteo API",
                    ErrorCode::ApiInvalidResponse,
                    HashMap::from([("location".to_string(), location_name.to_string())]),
                )
            })?;

        // Convert OpenMeteo results to our existing GeocodingResult format
        let geocoding_results: Vec<GeocodingResult> = openmeteo_response
            .results
            .unwrap_or_default()
            .iter()
            .map(|result| GeocodingResult {
                name: result.name.clone(),
                local_names: None, // OpenMeteo doesn't provide local names
                lat: result.latitude,
                lon: result.longitude,
                country: result.country.clone().unwrap_or_default(),
                state: result.admin1.clone(),
            })
            .collect();

        let parse_duration = parse_start.elapsed();
        let total_duration = start_time.elapsed();

        if geocoding_results.is_empty() {
            warn!("No results found for location '{}'", location_name);
        } else {
            info!(
                "Found {} geocoding results for '{}' in {:.3}s (parse: {:.3}s)",
                geocoding_results.len(),
                location_name,
                total_duration.as_secs_f64(),
                parse_duration.as_secs_f64()
            );

            debug!(
                "Geocoding results: {:?}",
                geocoding_results
                    .iter()
                    .map(|r| format!("{} ({:.4}, {:.4})", r.name, r.lat, r.lon))
                    .collect::<Vec<_>>()
            );
        }

        Ok(geocoding_results)
    }

    /// Get reverse geocoding information for coordinates using `OpenMeteo` API
    pub fn reverse_geocode(&self, lat: f64, lon: f64) -> Result<Vec<GeocodingResult>> {
        // OpenMeteo doesn't have a reverse geocoding API, so we return a basic result
        let geocoding_result = GeocodingResult {
            name: format!("{lat:.4}, {lon:.4}"),
            local_names: None,
            lat,
            lon,
            country: "Unknown".to_string(),
            state: None,
        };

        Ok(vec![geocoding_result])
    }

    /// Make a request with retry logic
    #[instrument(skip(self, url), fields(url = %url.split("appid=").next().unwrap_or(url)))]
    #[allow(clippy::too_many_lines)]
    async fn make_request(&self, url: &str) -> Result<Response> {
        let span = span!(Level::DEBUG, "make_request");
        let _enter = span.enter();

        let mut attempt = 0;
        let max_attempts = self.config.weather.max_retries + 1;
        let request_start = Instant::now();

        debug!("Starting HTTP request (max attempts: {})", max_attempts);

        while attempt < max_attempts {
            let attempt_start = Instant::now();


            debug!(
                "Making HTTP request (attempt {}/{})",
                attempt + 1,
                max_attempts
            );

            // Make the request
            match self.client.get(url).send().await {
                Ok(response) => {
                    let attempt_duration = attempt_start.elapsed();
                    let status = response.status();

                    debug!(
                        "HTTP response received: {} in {:.3}s",
                        status,
                        attempt_duration.as_secs_f64()
                    );

                    if status.is_success() {
                        let total_duration = request_start.elapsed();
                        info!(
                            "Successful API request in {:.3}s (attempt {})",
                            total_duration.as_secs_f64(),
                            attempt + 1
                        );
                        return Ok(response);
                    } else if status.as_u16() == 401 {
                        error!("API authentication failed (HTTP 401)");
                        return Err(TravelAiError::api_with_context(
                            "API authentication failed. Please check your weather API configuration.",
                            ErrorCode::ApiUnauthorized,
                            HashMap::new(),
                        )
                        .into());
                    } else if status.as_u16() == 404 {
                        warn!("Location not found (HTTP 404)");
                        return Err(TravelAiError::api_with_context(
                            "Location not found. Please check the coordinates or location name.",
                            ErrorCode::ApiLocationNotFound,
                            HashMap::new(),
                        )
                        .into());
                    } else if status.as_u16() == 429 {
                        // Rate limited by server
                        let retry_after = response
                            .headers()
                            .get("retry-after")
                            .and_then(|h| h.to_str().ok())
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(60);

                        warn!(
                            "Server rate limit exceeded (HTTP 429), retry after {}s",
                            retry_after
                        );

                        if attempt < max_attempts - 1 {
                            debug!("Sleeping {}s before retry", retry_after);
                            tokio::time::sleep(Duration::from_secs(retry_after)).await;
                            attempt += 1;
                        } else {
                            error!("Rate limit exceeded and retry attempts exhausted");
                            return Err(TravelAiError::api_with_context(
                                "Rate limit exceeded and retry attempts exhausted.",
                                ErrorCode::ApiRateLimit,
                                HashMap::from([(
                                    "retry_after".to_string(),
                                    retry_after.to_string(),
                                )]),
                            )
                            .into());
                        }
                    }
                    let error_msg = format!(
                        "API request failed with status: {status} - {}",
                        status.canonical_reason().unwrap_or("Unknown error")
                    );

                    warn!("HTTP error on attempt {}: {error_msg}", attempt + 1);

                    if attempt < max_attempts - 1 {
                        // Exponential backoff for server errors
                        let backoff = Duration::from_millis(1000 * (2_u64.pow(attempt)));
                        debug!("Exponential backoff: waiting {:.1}s", backoff.as_secs_f64());
                        tokio::time::sleep(backoff).await;
                        attempt += 1;
                    } else {
                        error!("API request failed after all attempts: {error_msg}");
                        return Err(TravelAiError::api_with_context(
                            error_msg,
                            ErrorCode::ApiNetworkError,
                            HashMap::from([
                                ("status_code".to_string(), status.as_u16().to_string()),
                                ("attempts".to_string(), max_attempts.to_string()),
                            ]),
                        )
                        .into());
                    }
                }
                Err(e) => {
                    let attempt_duration = attempt_start.elapsed();
                    warn!(
                        "Network error on attempt {} ({:.3}s): {}",
                        attempt + 1,
                        attempt_duration.as_secs_f64(),
                        e
                    );

                    if attempt < max_attempts - 1 {
                        // Exponential backoff for network errors
                        let backoff = Duration::from_millis(1000 * (2_u64.pow(attempt)));
                        debug!(
                            "Network error backoff: waiting {:.1}s",
                            backoff.as_secs_f64()
                        );
                        tokio::time::sleep(backoff).await;
                        attempt += 1;
                        continue;
                    }
                    error!("Network error after {max_attempts} attempts: {e}");
                    return Err(TravelAiError::api_with_context(
                        format!("Network error after {max_attempts} attempts: {e}"),
                        ErrorCode::ApiNetworkError,
                        HashMap::from([
                            ("attempts".to_string(), max_attempts.to_string()),
                            ("error".to_string(), e.to_string()),
                        ]),
                    )
                    .into());
                }
            }
        }

        error!("Request failed after all retry attempts");
        Err(TravelAiError::api_with_context(
            "Request failed after all retry attempts",
            ErrorCode::ApiNetworkError,
            HashMap::from([("max_attempts".to_string(), max_attempts.to_string())]),
        )
        .into())
    }
}

/// Geocoding result from weather API
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GeocodingResult {
    /// Location name
    pub name: String,
    /// Local names in different languages
    pub local_names: Option<std::collections::HashMap<String, String>>,
    /// Latitude
    pub lat: f64,
    /// Longitude
    pub lon: f64,
    /// Country code
    pub country: String,
    /// State code (for US locations)
    pub state: Option<String>,
}

impl From<GeocodingResult> for Location {
    fn from(geocoding: GeocodingResult) -> Self {
        let name = if let Some(state) = geocoding.state {
            format!("{}, {}", geocoding.name, state)
        } else {
            geocoding.name
        };

        Location::with_country(geocoding.lat, geocoding.lon, name, geocoding.country)
    }
}

/// Location parsing utilities
pub struct LocationParser;

impl LocationParser {
    /// Parse location input (coordinates, city names, postal codes)
    pub fn parse(input: &str) -> Result<LocationInput> {
        let input = input.trim();

        // Try to parse as coordinates (lat,lon)
        if let Ok(coords) = Self::parse_coordinates(input) {
            return Ok(LocationInput::Coordinates(coords.0, coords.1));
        }

        // Try to parse as postal code (numbers only or with country code)
        if Self::is_postal_code(input) {
            return Ok(LocationInput::PostalCode(input.to_string()));
        }

        // Otherwise treat as location name
        Ok(LocationInput::Name(input.to_string()))
    }

    /// Parse coordinates from string like "46.8182,8.2275" or "46.8182 8.2275"
    fn parse_coordinates(input: &str) -> Result<(f64, f64)> {
        let parts: Vec<&str> = input
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .collect();

        if parts.len() != 2 {
            return Err(
                TravelAiError::validation("Coordinates must be in format 'lat,lon'").into(),
            );
        }

        let lat = parts[0]
            .parse::<f64>()
            .with_context(|| format!("Invalid latitude: {}", parts[0]))?;
        let lon = parts[1]
            .parse::<f64>()
            .with_context(|| format!("Invalid longitude: {}", parts[1]))?;

        // Validate coordinate ranges
        if !(-90.0..=90.0).contains(&lat) {
            return Err(TravelAiError::validation(format!(
                "Latitude must be between -90 and 90, got: {lat}"
            ))
            .into());
        }

        if !(-180.0..=180.0).contains(&lon) {
            return Err(TravelAiError::validation(format!(
                "Longitude must be between -180 and 180, got: {lon}"
            ))
            .into());
        }

        Ok((lat, lon))
    }

    /// Check if input looks like a postal code
    fn is_postal_code(input: &str) -> bool {
        // Simple heuristic: contains mostly digits, optionally with country prefix
        let normalized = input.replace([' ', '-'], "");

        // US ZIP codes: 5 or 9 digits
        if normalized.len() == 5 || normalized.len() == 9 {
            return normalized.chars().all(|c| c.is_ascii_digit());
        }

        // International postal codes: country code + digits/letters
        // Must contain at least some digits to be a postal code
        if normalized.len() >= 3 && normalized.len() <= 10 {
            let (prefix, suffix) = normalized.split_at(2);
            if prefix.chars().all(|c| c.is_ascii_alphabetic())
                && suffix.len() >= 3
                && suffix.chars().all(|c| c.is_ascii_alphanumeric())
                && suffix.chars().any(|c| c.is_ascii_digit())
            {
                // Must contain digits
                return true;
            }
        }

        false
    }
}

/// Types of location input
#[derive(Debug, Clone)]
pub enum LocationInput {
    /// Coordinates (latitude, longitude)
    Coordinates(f64, f64),
    /// Location name (city, region, etc.)
    Name(String),
    /// Postal code
    PostalCode(String),
}

/// `OpenMeteo` API response structures and conversion utilities
pub mod openmeteo {
    use super::{Location, WeatherData, WeatherForecast};
    use chrono::Utc;
    use serde::Deserialize;

    /// Current weather and forecast response from `OpenMeteo` API
    #[derive(Debug, Deserialize)]
    pub struct ForecastResponse {
        pub latitude: f64,
        pub longitude: f64,
        pub timezone: String,
        pub timezone_abbreviation: String,
        pub hourly: Option<HourlyData>,
        pub daily: Option<DailyData>,
        pub current: Option<CurrentData>,
    }

    /// Hourly weather data from `OpenMeteo`
    #[derive(Debug, Deserialize)]
    pub struct HourlyData {
        pub time: Vec<String>,
        #[serde(rename = "temperature_2m")]
        pub temperature: Option<Vec<f32>>,
        #[serde(rename = "windspeed_10m")]
        pub wind_speed: Option<Vec<f32>>,
        #[serde(rename = "winddirection_10m")]
        pub wind_direction: Option<Vec<u16>>,
        #[serde(rename = "windgusts_10m")]
        pub wind_gusts: Option<Vec<f32>>,
        pub precipitation: Option<Vec<f32>>,
        #[serde(rename = "cloudcover")]
        pub cloud_cover: Option<Vec<u8>>,
        #[serde(rename = "surface_pressure")]
        pub pressure: Option<Vec<f32>>,
        pub visibility: Option<Vec<f32>>,
        #[serde(rename = "weathercode")]
        pub weather_code: Option<Vec<u8>>,
    }

    /// Daily weather data from `OpenMeteo`
    #[derive(Debug, Deserialize)]
    pub struct DailyData {
        pub time: Vec<String>,
        #[serde(rename = "temperature_2m_max")]
        pub temperature_max: Option<Vec<Option<f32>>>,
        #[serde(rename = "temperature_2m_min")]
        pub temperature_min: Option<Vec<Option<f32>>>,
        #[serde(rename = "windspeed_10m_max")]
        pub wind_speed_max: Option<Vec<Option<f32>>>,
        #[serde(rename = "winddirection_10m_dominant")]
        pub wind_direction: Option<Vec<Option<u16>>>,
        #[serde(rename = "precipitation_sum")]
        pub precipitation: Option<Vec<Option<f32>>>,
        #[serde(rename = "weathercode")]
        pub weather_code: Option<Vec<Option<u8>>>,
    }

    /// Current weather data from `OpenMeteo` (when available)
    #[derive(Debug, Deserialize)]
    pub struct CurrentData {
        #[serde(rename = "temperature_2m")]
        pub temperature: f32,
        #[serde(rename = "windspeed_10m")]
        pub wind_speed: f32,
        #[serde(rename = "winddirection_10m")]
        pub wind_direction: u16,
        #[serde(rename = "windgusts_10m")]
        pub wind_gusts: f32,
        pub precipitation: f32,
        #[serde(rename = "cloudcover")]
        pub cloud_cover: u8,
        #[serde(rename = "surface_pressure")]
        pub pressure: f32,
        pub visibility: f32,
        #[serde(rename = "weathercode")]
        pub weather_code: u8,
    }

    /// Geocoding response from `OpenMeteo`
    #[derive(Debug, Deserialize)]
    pub struct GeocodingResponse {
        pub results: Option<Vec<GeocodingResult>>,
    }

    #[derive(Debug, Deserialize)]
    pub struct GeocodingResult {
        pub name: String,
        pub latitude: f64,
        pub longitude: f64,
        pub country: Option<String>,
        pub admin1: Option<String>,
        pub admin2: Option<String>,
        pub timezone: Option<String>,
    }

    /// Convert `OpenMeteo` weather code to human-readable description
    #[must_use] 
    pub fn weather_code_to_description(code: u8) -> &'static str {
        match code {
            0 => "Clear sky",
            1 => "Mainly clear",
            2 => "Partly cloudy",
            3 => "Overcast",
            45 => "Fog",
            48 => "Depositing rime fog",
            51 => "Light drizzle",
            53 => "Moderate drizzle",
            55 => "Dense drizzle",
            56 => "Light freezing drizzle",
            57 => "Dense freezing drizzle",
            61 => "Slight rain",
            63 => "Moderate rain",
            65 => "Heavy rain",
            66 => "Light freezing rain",
            67 => "Heavy freezing rain",
            71 => "Slight snow fall",
            73 => "Moderate snow fall",
            75 => "Heavy snow fall",
            77 => "Snow grains",
            80 => "Slight rain showers",
            81 => "Moderate rain showers",
            82 => "Violent rain showers",
            85 => "Slight snow showers",
            86 => "Heavy snow showers",
            95 => "Thunderstorm",
            96 => "Thunderstorm with slight hail",
            99 => "Thunderstorm with heavy hail",
            _ => "Unknown",
        }
    }

    // Convert OpenMeteo API responses to internal models
    impl WeatherForecast {
        /// Create forecast from `OpenMeteo` API response
        #[must_use] 
        pub fn from_openmeteo(response: &ForecastResponse, location_name: String) -> Self {
            let location = Location::new(response.latitude, response.longitude, location_name);

            let mut forecasts = Vec::new();

            // Process hourly data if available
            if let Some(hourly) = &response.hourly {
                let len = hourly.time.len();

                for i in 0..len {
                    // Parse timestamp
                    let timestamp =
                        chrono::NaiveDateTime::parse_from_str(&hourly.time[i], "%Y-%m-%dT%H:%M").map_or_else(|_| Utc::now(), |dt| dt.and_utc());

                    // Extract data with safe indexing and default values
                    let temperature = *hourly
                        .temperature
                        .as_ref()
                        .and_then(|temps| temps.get(i))
                        .unwrap_or(&-999.0);

                    let wind_speed = *hourly
                        .wind_speed
                        .as_ref()
                        .and_then(|speeds| speeds.get(i))
                        .unwrap_or(&-999.0);

                    let wind_direction = *hourly
                        .wind_direction
                        .as_ref()
                        .and_then(|dirs| dirs.get(i))
                        .unwrap_or(&0);

                    let wind_gust = *hourly
                        .wind_gusts
                        .as_ref()
                        .and_then(|gusts| gusts.get(i))
                        .unwrap_or(&-999.0);

                    let precipitation = *hourly
                        .precipitation
                        .as_ref()
                        .and_then(|precip| precip.get(i))
                        .unwrap_or(&-999.0);
                    let cloud_cover = *hourly
                        .cloud_cover
                        .as_ref()
                        .and_then(|clouds| clouds.get(i))
                        .unwrap_or(&0);

                    let pressure = *hourly
                        .pressure
                        .as_ref()
                        .and_then(|press| press.get(i))
                        .unwrap_or(&-999.0);

                    let visibility = *hourly
                        .visibility
                        .as_ref()
                        .and_then(|vis| vis.get(i))
                        .unwrap_or(&999.0);

                    let weather_code = *hourly
                        .weather_code
                        .as_ref()
                        .and_then(|codes| codes.get(i))
                        .unwrap_or(&0);

                    let description = weather_code_to_description(weather_code).to_string();

                    let weather_data = WeatherData {
                        timestamp,
                        temperature,
                        wind_speed,
                        wind_direction,
                        wind_gust,
                        precipitation,
                        cloud_cover,
                        pressure,
                        visibility,
                        description,
                        icon: None, // OpenMeteo doesn't provide icon codes
                    };

                    forecasts.push(weather_data);
                }
            }

            Self {
                location,
                forecasts,
                retrieved_at: Utc::now(),
            }
        }
    }

    impl From<&GeocodingResult> for Location {
        fn from(result: &GeocodingResult) -> Self {
            Self {
                latitude: result.latitude,
                longitude: result.longitude,
                name: result.name.clone(),
                country: result.country.clone(),
            }
        }
    }
}

/// Add urlencoding dependency to Cargo.toml (needed for geocoding)
// Note: We'll need to add `urlencoding = "2.1"` to dependencies
#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn test_location_parser_coordinates() {
        // Test various coordinate formats
        assert!(matches!(
            LocationParser::parse("46.8182,8.2275").unwrap(),
            LocationInput::Coordinates(46.8182, 8.2275)
        ));

        assert!(matches!(
            LocationParser::parse("46.8182 8.2275").unwrap(),
            LocationInput::Coordinates(46.8182, 8.2275)
        ));

        assert!(matches!(
            LocationParser::parse("-46.8182, -8.2275").unwrap(),
            LocationInput::Coordinates(-46.8182, -8.2275)
        ));
    }

    #[test]
    fn test_location_parser_invalid_coordinates() {
        // Invalid latitude - should be treated as location name
        assert!(matches!(
            LocationParser::parse("91.0,8.0").unwrap(),
            LocationInput::Name(_)
        ));
        assert!(matches!(
            LocationParser::parse("-91.0,8.0").unwrap(),
            LocationInput::Name(_)
        ));

        // Invalid longitude - should be treated as location name
        assert!(matches!(
            LocationParser::parse("46.0,181.0").unwrap(),
            LocationInput::Name(_)
        ));
        assert!(matches!(
            LocationParser::parse("46.0,-181.0").unwrap(),
            LocationInput::Name(_)
        ));

        // Invalid format - should be treated as location name
        assert!(matches!(
            LocationParser::parse("46.0").unwrap(),
            LocationInput::Name(_)
        ));
        assert!(matches!(
            LocationParser::parse("46.0,8.0,0.0").unwrap(),
            LocationInput::Name(_)
        ));
    }

    #[test]
    fn test_location_parser_postal_codes() {
        assert!(matches!(
            LocationParser::parse("12345").unwrap(),
            LocationInput::PostalCode(_)
        ));

        assert!(matches!(
            LocationParser::parse("CH-8001").unwrap(),
            LocationInput::PostalCode(_)
        ));

        assert!(matches!(
            LocationParser::parse("SW1A 1AA").unwrap(),
            LocationInput::PostalCode(_)
        ));
    }

    #[test]
    fn test_location_parser_names() {
        assert!(matches!(
            LocationParser::parse("Interlaken").unwrap(),
            LocationInput::Name(_)
        ));

        assert!(matches!(
            LocationParser::parse("New York City").unwrap(),
            LocationInput::Name(_)
        ));

        assert!(matches!(
            LocationParser::parse("Chamonix-Mont-Blanc").unwrap(),
            LocationInput::Name(_)
        ));
    }

    #[test]
    fn test_postal_code_detection() {
        // US ZIP codes
        assert!(LocationParser::is_postal_code("12345"));
        assert!(LocationParser::is_postal_code("123456789"));

        // International codes
        assert!(LocationParser::is_postal_code("CH8001"));
        assert!(LocationParser::is_postal_code("SW1A1AA"));

        // Not postal codes
        assert!(!LocationParser::is_postal_code("Interlaken"));
        assert!(!LocationParser::is_postal_code("46.8182"));
        assert!(!LocationParser::is_postal_code("1234")); // Too short
        assert!(!LocationParser::is_postal_code("12345678901")); // Too long
    }

    #[test]
    fn test_geocoding_result_to_location() {
        let geocoding = GeocodingResult {
            name: "Interlaken".to_string(),
            local_names: None,
            lat: 46.8182,
            lon: 8.2275,
            country: "CH".to_string(),
            state: Some("BE".to_string()),
        };

        let location: Location = geocoding.into();
        assert_eq!(location.name, "Interlaken, BE");
        assert_eq!(location.latitude, 46.8182);
        assert_eq!(location.longitude, 8.2275);
        assert_eq!(location.country, Some("CH".to_string()));
    }
}
