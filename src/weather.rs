use crate::models::{weather::WeatherForecast, Location, WeatherData};
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use sunrise::{Coordinates, SolarDay, SolarEvent};

pub fn get_forecast(location: Location) -> Result<WeatherForecast> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&hourly=temperature_2m,windspeed_10m,winddirection_10m,windgusts_10m,precipitation,cloudcover,surface_pressure,visibility,weathercode&timezone=auto&forecast_days=7&wind_speed_unit=ms", location.latitude, location.longitude
    );

    let response = reqwest::blocking::get(url)?;

    let forecast_response: openmeteo::ForecastResponse = response
        .json()
        .with_context(|| "Failed to parse OpenMeteo forecast response")?;

    let forecast = WeatherForecast::from_openmeteo(&forecast_response, location);
    Ok(forecast)
}

pub fn geocode(location_name: &str) -> Result<Vec<Location>> {
    // OpenMeteo geocoding API (no API key required)
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=5&language=en&format=json",
        urlencoding::encode(location_name)
    );

    let response = reqwest::blocking::get(url)?;

    let openmeteo_response: openmeteo::GeocodingResponse = response
        .json()
        .with_context(|| "Failed to parse OpenMeteo geocoding response")?;

    // Convert OpenMeteo results to our existing GeocodingResult format
    let geocoding_results: Vec<Location> = openmeteo_response
        .results
        .unwrap_or_default()
        .into_iter()
        .map(|geocoding_result| geocoding_result.into())
        .collect();
    Ok(geocoding_results)
}

pub fn get_sunrise_sunset(location: &Location, date: NaiveDate) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    let coordinates = Coordinates::new(location.latitude, location.longitude)
        .with_context(|| format!("Invalid coordinates: lat={}, lng={}", location.latitude, location.longitude))?;
    
    let solar_day = SolarDay::new(coordinates, date);
    
    let sunrise = solar_day.event_time(SolarEvent::Sunrise);
    
    let sunset = solar_day.event_time(SolarEvent::Sunset);
    
    Ok((sunrise, sunset))
}

/// `OpenMeteo` API response structures and conversion utilities
mod openmeteo {
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

    impl Into<Location> for GeocodingResult{
        fn into(self) -> Location {
            Location { latitude: self.latitude, longitude: self.longitude, name: self.name, country: self.country.unwrap_or("Unknown".into()) }
        }
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
        pub fn from_openmeteo(response: &ForecastResponse, location: Location) -> Self {

            let mut forecasts = Vec::new();

            // Process hourly data if available
            if let Some(hourly) = &response.hourly {
                let len = hourly.time.len();

                for i in 0..len {
                    // Parse timestamp
                    let timestamp =
                        chrono::NaiveDateTime::parse_from_str(&hourly.time[i], "%Y-%m-%dT%H:%M")
                            .map_or_else(|_| Utc::now(), |dt| dt.and_utc());

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
                        wind_speed_ms: wind_speed,
                        wind_direction,
                        wind_gust_ms: wind_gust,
                        precipitation,
                        cloud_cover,
                        pressure,
                        visibility,
                        description,
                    };

                    forecasts.push(weather_data);
                }
            }

            Self {
                location,
                forecast: forecasts,
            }
        }
    }
}
