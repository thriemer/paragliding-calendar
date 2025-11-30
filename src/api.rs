//! Weather API client for OpenMeteo integration  
//!
//! This module provides HTTP client functionality for retrieving weather data
//! from the OpenMeteo API with rate limiting, retry logic, and error handling.
//! Previously integrated with OpenWeatherMap, now uses OpenMeteo for API-key-free access.

use crate::config::TravelAiConfig;
use crate::models::{Location, WeatherData, WeatherForecast, openmeteo};
use crate::{ErrorCode, TravelAiError};
use anyhow::{Context, Result};
use chrono::Utc;
use reqwest::blocking::{Client, Response};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};
use tracing::{Level, debug, error, info, instrument, span, warn};

/// Rate limiter for API requests
#[derive(Debug)]
pub struct RateLimiter {
    /// Maximum requests per minute
    max_requests_per_minute: u32,
    /// Request timestamps within the current minute
    request_times: Vec<Instant>,
    /// Last cleanup time
    last_cleanup: Instant,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(max_requests_per_minute: u32) -> Self {
        Self {
            max_requests_per_minute,
            request_times: Vec::new(),
            last_cleanup: Instant::now(),
        }
    }

    /// Check if a request is allowed and record it
    pub fn allow_request(&mut self) -> bool {
        self.cleanup_old_requests();

        if self.request_times.len() >= self.max_requests_per_minute as usize {
            false
        } else {
            self.request_times.push(Instant::now());
            true
        }
    }

    /// Get time until next request is allowed
    pub fn time_until_next_request(&mut self) -> Duration {
        self.cleanup_old_requests();

        if self.request_times.len() < self.max_requests_per_minute as usize {
            Duration::from_secs(0)
        } else if let Some(oldest) = self.request_times.first() {
            let elapsed = oldest.elapsed();
            if elapsed >= Duration::from_secs(60) {
                Duration::from_secs(0)
            } else {
                Duration::from_secs(60) - elapsed
            }
        } else {
            Duration::from_secs(0)
        }
    }

    /// Remove requests older than 1 minute
    fn cleanup_old_requests(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_cleanup) >= Duration::from_secs(10) {
            let cutoff = now - Duration::from_secs(60);
            self.request_times.retain(|&time| time > cutoff);
            self.last_cleanup = now;
        }
    }
}

/// Weather API client for OpenWeatherMap
pub struct WeatherApiClient {
    /// HTTP client
    client: Client,
    /// API configuration
    config: TravelAiConfig,
    /// Rate limiter
    rate_limiter: RateLimiter,
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

        // OpenWeatherMap free tier: 60 requests per minute
        let rate_limiter = RateLimiter::new(60);

        Ok(Self {
            client,
            config,
            rate_limiter,
        })
    }

    /// Get current weather for a location using OpenMeteo API
    #[instrument(skip(self), fields(lat, lon))]
    pub fn get_current_weather(&mut self, lat: f64, lon: f64) -> Result<WeatherData> {
        let span = span!(Level::INFO, "get_current_weather", lat, lon);
        let _enter = span.enter();

        info!(
            "Getting current weather for coordinates: {:.4}, {:.4}",
            lat, lon
        );
        let start_time = Instant::now();

        // OpenMeteo API doesn't require API key, use forecast endpoint with current=true
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&current=temperature_2m,windspeed_10m,winddirection_10m,windgusts_10m,precipitation,cloudcover,surface_pressure,visibility,weathercode&wind_speed_unit=ms",
            lat, lon
        );

        debug!("OpenMeteo API request URL: {}", url);

        let response = self.make_request(&url)?;

        let parse_start = Instant::now();
        let forecast_response: openmeteo::ForecastResponse = response
            .json()
            .with_context(|| "Failed to parse OpenMeteo weather response")
            .map_err(|e| {
                error!("Failed to parse weather response: {}", e);
                TravelAiError::api_with_context(
                    "Invalid weather data received from OpenMeteo API",
                    ErrorCode::ApiInvalidResponse,
                    HashMap::from([("coordinates".to_string(), format!("{:.4},{:.4}", lat, lon))]),
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
                cloud_cover: Some(current.cloud_cover),
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
                HashMap::from([("coordinates".to_string(), format!("{:.4},{:.4}", lat, lon))]),
            )
            .into())
        }
    }

    /// Get 7-day weather forecast for a location using OpenMeteo API
    #[instrument(skip(self), fields(lat, lon))]
    pub fn get_forecast(&mut self, lat: f64, lon: f64) -> Result<WeatherForecast> {
        let span = span!(Level::INFO, "get_forecast", lat, lon);
        let _enter = span.enter();

        info!(
            "Getting 7-day forecast for coordinates: {:.4}, {:.4}",
            lat, lon
        );
        let start_time = Instant::now();

        // OpenMeteo API for hourly forecast data (7 days)
        let url = format!(
            "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&hourly=temperature_2m,windspeed_10m,winddirection_10m,windgusts_10m,precipitation,cloudcover,surface_pressure,visibility,weathercode&timezone=auto&forecast_days=7",
            lat, lon
        );

        let response = self.make_request(&url)?;

        let parse_start = Instant::now();
        let forecast_response: openmeteo::ForecastResponse = response
            .json()
            .with_context(|| "Failed to parse OpenMeteo forecast response")
            .map_err(|e| {
                error!("Failed to parse forecast response: {}", e);
                TravelAiError::api_with_context(
                    "Invalid forecast data received from OpenMeteo API",
                    ErrorCode::ApiInvalidResponse,
                    HashMap::from([("coordinates".to_string(), format!("{:.4},{:.4}", lat, lon))]),
                )
            })?;

        let parse_duration = parse_start.elapsed();
        let total_duration = start_time.elapsed();

        // Create forecast using our OpenMeteo conversion method
        let location_name = format!("{:.4}, {:.4}", lat, lon); // Default name, will be updated by geocoding
        let forecast = WeatherForecast::from_openmeteo(&forecast_response, location_name);

        info!(
            "Successfully retrieved forecast with {} data points in {:.3}s (parse: {:.3}s)",
            forecast.forecasts.len(),
            total_duration.as_secs_f64(),
            parse_duration.as_secs_f64()
        );

        if total_duration.as_secs() > 5 {
            warn!(
                "Slow forecast API response: {:.3}s",
                total_duration.as_secs_f64()
            );
        }

        Ok(forecast)
    }

    /// Get geocoding information for a location name using OpenMeteo API
    #[instrument(skip(self), fields(location = location_name))]
    pub fn geocode(&mut self, location_name: &str) -> Result<Vec<GeocodingResult>> {
        let span = span!(Level::INFO, "geocode", location = location_name);
        let _enter = span.enter();

        info!("Geocoding location: '{}'", location_name);
        let start_time = Instant::now();

        // OpenMeteo geocoding API (no API key required)
        let url = format!(
            "https://geocoding-api.open-meteo.com/v1/search?name={}&count=5&language=en&format=json",
            urlencoding::encode(location_name)
        );

        let response = self.make_request(&url)?;

        let parse_start = Instant::now();
        let openmeteo_response: openmeteo::GeocodingResponse = response
            .json()
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

    /// Get reverse geocoding information for coordinates using OpenMeteo API
    pub fn reverse_geocode(&mut self, lat: f64, lon: f64) -> Result<Vec<GeocodingResult>> {
        // OpenMeteo doesn't have a reverse geocoding API, so we return a basic result
        let geocoding_result = GeocodingResult {
            name: format!("{:.4}, {:.4}", lat, lon),
            local_names: None,
            lat,
            lon,
            country: "Unknown".to_string(),
            state: None,
        };

        Ok(vec![geocoding_result])
    }

    /// Make a request with rate limiting and retry logic
    #[instrument(skip(self, url), fields(url = %url.split("appid=").next().unwrap_or(url)))]
    fn make_request(&mut self, url: &str) -> Result<Response> {
        let span = span!(Level::DEBUG, "make_request");
        let _enter = span.enter();

        let mut attempt = 0;
        let max_attempts = self.config.weather.max_retries + 1;
        let request_start = Instant::now();

        debug!("Starting HTTP request (max attempts: {})", max_attempts);

        while attempt < max_attempts {
            let attempt_start = Instant::now();

            // Rate limiting
            if !self.rate_limiter.allow_request() {
                let wait_time = self.rate_limiter.time_until_next_request();
                if wait_time > Duration::from_secs(0) {
                    warn!(
                        "Rate limit exceeded, waiting {:.1}s",
                        wait_time.as_secs_f64()
                    );
                    if attempt == 0 {
                        return Err(TravelAiError::api_with_context(
                            format!(
                                "Rate limit exceeded. Please wait {} seconds.",
                                wait_time.as_secs()
                            ),
                            ErrorCode::ApiRateLimit,
                            HashMap::from([(
                                "wait_time".to_string(),
                                wait_time.as_secs().to_string(),
                            )]),
                        )
                        .into());
                    }
                    thread::sleep(wait_time);
                }
                continue;
            }

            debug!(
                "Making HTTP request (attempt {}/{})",
                attempt + 1,
                max_attempts
            );

            // Make the request
            match self.client.get(url).send() {
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
                            "Invalid API key. Please check your OpenWeatherMap API key.",
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
                            thread::sleep(Duration::from_secs(retry_after));
                            attempt += 1;
                            continue;
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
                    } else {
                        let error_msg = format!(
                            "API request failed with status: {} - {}",
                            status,
                            status.canonical_reason().unwrap_or("Unknown error")
                        );

                        warn!("HTTP error on attempt {}: {}", attempt + 1, error_msg);

                        if attempt < max_attempts - 1 {
                            // Exponential backoff for server errors
                            let backoff = Duration::from_millis(1000 * (2_u64.pow(attempt)));
                            debug!("Exponential backoff: waiting {:.1}s", backoff.as_secs_f64());
                            thread::sleep(backoff);
                            attempt += 1;
                            continue;
                        } else {
                            error!("API request failed after all attempts: {}", error_msg);
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
                        thread::sleep(backoff);
                        attempt += 1;
                        continue;
                    } else {
                        error!("Network error after {} attempts: {}", max_attempts, e);
                        return Err(TravelAiError::api_with_context(
                            format!("Network error after {} attempts: {}", max_attempts, e),
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

/// Geocoding result from OpenWeatherMap API
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
                "Latitude must be between -90 and 90, got: {}",
                lat
            ))
            .into());
        }

        if !(-180.0..=180.0).contains(&lon) {
            return Err(TravelAiError::validation(format!(
                "Longitude must be between -180 and 180, got: {}",
                lon
            ))
            .into());
        }

        Ok((lat, lon))
    }

    /// Check if input looks like a postal code
    fn is_postal_code(input: &str) -> bool {
        // Simple heuristic: contains mostly digits, optionally with country prefix
        let normalized = input.replace(' ', "").replace('-', "");

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

/// Add urlencoding dependency to Cargo.toml (needed for geocoding)
// Note: We'll need to add `urlencoding = "2.1"` to dependencies

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter() {
        let mut limiter = RateLimiter::new(2);

        // Should allow first 2 requests
        assert!(limiter.allow_request());
        assert!(limiter.allow_request());

        // Should deny 3rd request
        assert!(!limiter.allow_request());

        // Check time until next request
        let wait_time = limiter.time_until_next_request();
        assert!(wait_time > Duration::from_secs(0));
    }

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

