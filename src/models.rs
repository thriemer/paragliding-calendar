//! Data models for weather information and API responses
//!
//! This module contains all the data structures used for representing weather data,
//! including both the internal models and the external API response types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Core weather data structure for internal use
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WeatherData {
    /// Timestamp for this weather observation
    pub timestamp: DateTime<Utc>,
    /// Temperature in Celsius
    pub temperature: f32,
    /// Wind speed in m/s
    pub wind_speed: f32,
    /// Wind direction in degrees (0-360, where 0/360 is North)
    pub wind_direction: u16,
    /// Wind gust speed in m/s (optional)
    pub wind_gust: Option<f32>,
    /// Precipitation amount in mm
    pub precipitation: f32,
    /// Cloud cover percentage (0-100, optional)
    pub cloud_cover: Option<u8>,
    /// Atmospheric pressure in hPa
    pub pressure: f32,
    /// Visibility in kilometers (optional)
    pub visibility: Option<f32>,
    /// Human-readable description of weather conditions
    pub description: String,
    /// Weather condition icon ID from API
    pub icon: Option<String>,
}

/// Location coordinates
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Location {
    /// Latitude in decimal degrees
    pub latitude: f64,
    /// Longitude in decimal degrees  
    pub longitude: f64,
    /// Location name (city, region, etc.)
    pub name: String,
    /// Country code (ISO 3166-1 alpha-2)
    pub country: Option<String>,
}

/// Weather forecast containing multiple weather data points
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WeatherForecast {
    /// Location for this forecast
    pub location: Location,
    /// List of weather data points (sorted by timestamp)
    pub forecasts: Vec<WeatherData>,
    /// When this forecast was retrieved
    pub retrieved_at: DateTime<Utc>,
}

/// OpenMeteo API response structures
pub mod openmeteo {
    use super::*;

    /// Current weather and forecast response from OpenMeteo API
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

    /// Hourly weather data from OpenMeteo
    #[derive(Debug, Deserialize)]
    pub struct HourlyData {
        pub time: Vec<String>,
        #[serde(rename = "temperature_2m")]
        pub temperature: Option<Vec<Option<f32>>>,
        #[serde(rename = "windspeed_10m")]
        pub wind_speed: Option<Vec<Option<f32>>>,
        #[serde(rename = "winddirection_10m")]
        pub wind_direction: Option<Vec<Option<u16>>>,
        #[serde(rename = "windgusts_10m")]
        pub wind_gusts: Option<Vec<Option<f32>>>,
        pub precipitation: Option<Vec<Option<f32>>>,
        #[serde(rename = "cloudcover")]
        pub cloud_cover: Option<Vec<Option<u8>>>,
        #[serde(rename = "surface_pressure")]
        pub pressure: Option<Vec<Option<f32>>>,
        pub visibility: Option<Vec<Option<f32>>>,
        #[serde(rename = "weathercode")]
        pub weather_code: Option<Vec<Option<u8>>>,
    }

    /// Daily weather data from OpenMeteo
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

    /// Current weather data from OpenMeteo (when available)
    #[derive(Debug, Deserialize)]
    pub struct CurrentData {
        #[serde(rename = "temperature_2m")]
        pub temperature: f32,
        #[serde(rename = "windspeed_10m")]
        pub wind_speed: f32,
        #[serde(rename = "winddirection_10m")]
        pub wind_direction: u16,
        #[serde(rename = "windgusts_10m")]
        pub wind_gusts: Option<f32>,
        pub precipitation: f32,
        #[serde(rename = "cloudcover")]
        pub cloud_cover: u8,
        #[serde(rename = "surface_pressure")]
        pub pressure: f32,
        pub visibility: Option<f32>,
        #[serde(rename = "weathercode")]
        pub weather_code: u8,
    }

    /// Geocoding response from OpenMeteo
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

    /// Convert OpenMeteo weather code to human-readable description
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
}

impl WeatherData {
    /// Convert temperature from Kelvin to Celsius
    pub fn kelvin_to_celsius(kelvin: f32) -> f32 {
        kelvin - 273.15
    }

    /// Convert wind direction from degrees to cardinal direction
    pub fn wind_direction_to_cardinal(degrees: u16) -> &'static str {
        match degrees {
            0..=11 | 349..=360 => "N",
            12..=33 => "NNE",
            34..=56 => "NE",
            57..=78 => "ENE",
            79..=101 => "E",
            102..=123 => "ESE",
            124..=146 => "SE",
            147..=168 => "SSE",
            169..=191 => "S",
            192..=213 => "SSW",
            214..=236 => "SW",
            237..=258 => "WSW",
            259..=281 => "W",
            282..=303 => "WNW",
            304..=326 => "NW",
            327..=348 => "NNW",
            _ => "Unknown",
        }
    }

    /// Format temperature with unit
    pub fn format_temperature(&self) -> String {
        format!("{:.1}°C", self.temperature)
    }

    /// Format wind information
    pub fn format_wind(&self) -> String {
        let direction = Self::wind_direction_to_cardinal(self.wind_direction);
        if let Some(gust) = self.wind_gust {
            format!(
                "{:.1} m/s {} (gusts {:.1} m/s)",
                self.wind_speed, direction, gust
            )
        } else {
            format!("{:.1} m/s {}", self.wind_speed, direction)
        }
    }

    /// Check if conditions are suitable for paragliding (basic heuristic)
    pub fn is_suitable_for_paragliding(&self) -> bool {
        // Basic safety criteria for paragliding
        // - Wind speed between 2-15 m/s
        // - No heavy precipitation
        // - Reasonable visibility

        let wind_ok = self.wind_speed >= 2.0 && self.wind_speed <= 15.0;
        let precipitation_ok = self.precipitation < 1.0; // Less than 1mm
        let visibility_ok = self.visibility.unwrap_or(10.0) >= 5.0; // At least 5km

        wind_ok && precipitation_ok && visibility_ok
    }
}

impl Location {
    /// Create a new location
    pub fn new(latitude: f64, longitude: f64, name: String) -> Self {
        Self {
            latitude,
            longitude,
            name,
            country: None,
        }
    }

    /// Create location with country
    pub fn with_country(latitude: f64, longitude: f64, name: String, country: String) -> Self {
        Self {
            latitude,
            longitude,
            name,
            country: Some(country),
        }
    }

    /// Format location as coordinates string
    pub fn format_coordinates(&self) -> String {
        format!("{:.4}, {:.4}", self.latitude, self.longitude)
    }

    /// Round coordinates for cache key generation
    pub fn rounded_coordinates(&self, precision: u32) -> (f64, f64) {
        let multiplier = 10_f64.powi(precision as i32);
        let lat = (self.latitude * multiplier).round() / multiplier;
        let lon = (self.longitude * multiplier).round() / multiplier;
        (lat, lon)
    }

    /// Generate cache key for this location
    pub fn cache_key(&self, date: &str) -> String {
        let (lat, lon) = self.rounded_coordinates(2); // Round to 2 decimal places
        format!("weather:{:.2}:{:.2}:{}", lat, lon, date)
    }
}

impl WeatherForecast {
    /// Create new forecast
    pub fn new(location: Location, forecasts: Vec<WeatherData>) -> Self {
        Self {
            location,
            forecasts,
            retrieved_at: Utc::now(),
        }
    }

    /// Get current weather (first forecast item)
    pub fn current_weather(&self) -> Option<&WeatherData> {
        self.forecasts.first()
    }

    /// Get weather for a specific day (returns all forecasts for that day)
    pub fn daily_forecast(&self, day_offset: usize) -> Vec<&WeatherData> {
        if self.forecasts.is_empty() {
            return Vec::new();
        }

        let base_date = self.forecasts[0].timestamp.date_naive();
        let target_date = base_date + chrono::Duration::days(day_offset as i64);

        self.forecasts
            .iter()
            .filter(|w| w.timestamp.date_naive() == target_date)
            .collect()
    }

    /// Check if forecast data is still fresh (not older than cache TTL)
    pub fn is_fresh(&self, ttl_hours: u32) -> bool {
        let age = Utc::now() - self.retrieved_at;
        age.num_hours() < ttl_hours as i64
    }
}

// Convert OpenWeatherMap API responses to internal models
impl From<&openweather::CurrentWeatherResponse> for WeatherData {
    fn from(response: &openweather::CurrentWeatherResponse) -> Self {
        let weather = response.weather.first();

        Self {
            timestamp: DateTime::from_timestamp(response.dt, 0).unwrap_or_else(Utc::now),
            temperature: WeatherData::kelvin_to_celsius(response.main.temp),
            wind_speed: response.wind.as_ref().map(|w| w.speed).unwrap_or(0.0),
            wind_direction: response.wind.as_ref().and_then(|w| w.deg).unwrap_or(0),
            wind_gust: response.wind.as_ref().and_then(|w| w.gust),
            precipitation: 0.0, // Current weather doesn't include precipitation amount
            cloud_cover: response.clouds.as_ref().map(|c| c.all),
            pressure: response.main.pressure,
            visibility: response.visibility.map(|v| v as f32 / 1000.0), // Convert m to km
            description: weather
                .map(|w| w.description.clone())
                .unwrap_or_else(|| "Unknown".to_string()),
            icon: weather.map(|w| w.icon.clone()),
        }
    }
}

impl From<&openweather::ForecastItem> for WeatherData {
    fn from(item: &openweather::ForecastItem) -> Self {
        let weather = item.weather.first();

        // Calculate precipitation from rain and snow
        let precipitation = item
            .rain
            .as_ref()
            .and_then(|r| r.three_hour.or(r.one_hour))
            .unwrap_or(0.0)
            + item
                .snow
                .as_ref()
                .and_then(|s| s.three_hour.or(s.one_hour))
                .unwrap_or(0.0);

        Self {
            timestamp: DateTime::from_timestamp(item.dt, 0).unwrap_or_else(Utc::now),
            temperature: WeatherData::kelvin_to_celsius(item.main.temp),
            wind_speed: item.wind.as_ref().map(|w| w.speed).unwrap_or(0.0),
            wind_direction: item.wind.as_ref().and_then(|w| w.deg).unwrap_or(0),
            wind_gust: item.wind.as_ref().and_then(|w| w.gust),
            precipitation,
            cloud_cover: item.clouds.as_ref().map(|c| c.all),
            pressure: item.main.pressure,
            visibility: item.visibility.map(|v| v as f32 / 1000.0), // Convert m to km
            description: weather
                .map(|w| w.description.clone())
                .unwrap_or_else(|| "Unknown".to_string()),
            icon: weather.map(|w| w.icon.clone()),
        }
    }
}

impl From<&openweather::CityInfo> for Location {
    fn from(city: &openweather::CityInfo) -> Self {
        Self::with_country(
            city.coord.lat,
            city.coord.lon,
            city.name.clone(),
            city.country.clone(),
        )
    }
}

// Convert OpenMeteo API responses to internal models
impl WeatherForecast {
    /// Create forecast from OpenMeteo API response
    pub fn from_openmeteo(response: &openmeteo::ForecastResponse, location_name: String) -> Self {
        let location = Location::new(response.latitude, response.longitude, location_name);

        let mut forecasts = Vec::new();

        // Process hourly data if available
        if let Some(hourly) = &response.hourly {
            let len = hourly.time.len();

            for i in 0..len {
                // Parse timestamp
                let timestamp = chrono::DateTime::parse_from_rfc3339(&hourly.time[i])
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                // Extract data with safe indexing and default values
                let temperature = hourly
                    .temperature
                    .as_ref()
                    .and_then(|temps| temps.get(i))
                    .and_then(|&temp| temp)
                    .unwrap_or(0.0);

                let wind_speed = hourly
                    .wind_speed
                    .as_ref()
                    .and_then(|speeds| speeds.get(i))
                    .and_then(|&speed| speed)
                    .unwrap_or(0.0);

                let wind_direction = hourly
                    .wind_direction
                    .as_ref()
                    .and_then(|dirs| dirs.get(i))
                    .and_then(|&dir| dir)
                    .unwrap_or(0);

                let wind_gust = hourly
                    .wind_gusts
                    .as_ref()
                    .and_then(|gusts| gusts.get(i))
                    .and_then(|&gust| gust);

                let precipitation = hourly
                    .precipitation
                    .as_ref()
                    .and_then(|precip| precip.get(i))
                    .and_then(|&p| p)
                    .unwrap_or(0.0);

                let cloud_cover = hourly
                    .cloud_cover
                    .as_ref()
                    .and_then(|clouds| clouds.get(i))
                    .and_then(|&cloud| cloud);

                let pressure = hourly
                    .pressure
                    .as_ref()
                    .and_then(|press| press.get(i))
                    .and_then(|&p| p)
                    .unwrap_or(1013.0);

                let visibility = hourly
                    .visibility
                    .as_ref()
                    .and_then(|vis| vis.get(i))
                    .and_then(|&v| v);

                let weather_code = hourly
                    .weather_code
                    .as_ref()
                    .and_then(|codes| codes.get(i))
                    .and_then(|&code| code)
                    .unwrap_or(0);

                let description = openmeteo::weather_code_to_description(weather_code).to_string();

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

impl From<&openmeteo::GeocodingResult> for Location {
    fn from(result: &openmeteo::GeocodingResult) -> Self {
        Self {
            latitude: result.latitude,
            longitude: result.longitude,
            name: result.name.clone(),
            country: result.country.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kelvin_to_celsius() {
        assert_eq!(WeatherData::kelvin_to_celsius(273.15), 0.0);
        assert_eq!(WeatherData::kelvin_to_celsius(293.15), 20.0);
    }

    #[test]
    fn test_wind_direction_to_cardinal() {
        assert_eq!(WeatherData::wind_direction_to_cardinal(0), "N");
        assert_eq!(WeatherData::wind_direction_to_cardinal(90), "E");
        assert_eq!(WeatherData::wind_direction_to_cardinal(180), "S");
        assert_eq!(WeatherData::wind_direction_to_cardinal(270), "W");
        assert_eq!(WeatherData::wind_direction_to_cardinal(45), "NE");
    }

    #[test]
    fn test_location_cache_key() {
        let location = Location::new(46.8182, 8.2275, "Interlaken".to_string());
        let key = location.cache_key("2023-12-01");
        assert_eq!(key, "weather:46.82:8.23:2023-12-01");
    }

    #[test]
    fn test_paragliding_suitability() {
        let mut weather = WeatherData {
            timestamp: Utc::now(),
            temperature: 15.0,
            wind_speed: 8.0, // Good wind speed
            wind_direction: 180,
            wind_gust: None,
            precipitation: 0.0, // No rain
            cloud_cover: Some(30),
            pressure: 1013.0,
            visibility: Some(15.0), // Good visibility
            description: "Clear sky".to_string(),
            icon: None,
        };

        assert!(weather.is_suitable_for_paragliding());

        // Test unsuitable conditions
        weather.wind_speed = 20.0; // Too windy
        assert!(!weather.is_suitable_for_paragliding());

        weather.wind_speed = 8.0;
        weather.precipitation = 5.0; // Heavy rain
        assert!(!weather.is_suitable_for_paragliding());
    }

    #[test]
    fn test_location_rounded_coordinates() {
        let location = Location::new(46.818234, 8.227456, "Test".to_string());
        let (lat, lon) = location.rounded_coordinates(2);
        assert_eq!(lat, 46.82);
        assert_eq!(lon, 8.23);
    }

    #[test]
    fn test_weather_forecast_daily() {
        let location = Location::new(46.8182, 8.2275, "Interlaken".to_string());
        let base_time = Utc::now();

        let forecasts = vec![
            WeatherData {
                timestamp: base_time,
                temperature: 15.0,
                wind_speed: 5.0,
                wind_direction: 180,
                wind_gust: None,
                precipitation: 0.0,
                cloud_cover: Some(20),
                pressure: 1013.0,
                visibility: Some(10.0),
                description: "Clear".to_string(),
                icon: None,
            },
            WeatherData {
                timestamp: base_time + chrono::Duration::days(1),
                temperature: 18.0,
                wind_speed: 7.0,
                wind_direction: 200,
                wind_gust: None,
                precipitation: 0.2,
                cloud_cover: Some(40),
                pressure: 1015.0,
                visibility: Some(12.0),
                description: "Partly cloudy".to_string(),
                icon: None,
            },
        ];

        let forecast = WeatherForecast::new(location, forecasts);

        // Test current weather
        assert!(forecast.current_weather().is_some());
        assert_eq!(forecast.current_weather().unwrap().temperature, 15.0);

        // Test daily forecast
        let today = forecast.daily_forecast(0);
        assert_eq!(today.len(), 1);
        assert_eq!(today[0].temperature, 15.0);

        let tomorrow = forecast.daily_forecast(1);
        assert_eq!(tomorrow.len(), 1);
        assert_eq!(tomorrow[0].temperature, 18.0);
    }
}

