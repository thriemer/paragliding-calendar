//! Weather data model and display methods

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
    pub wind_gust: f32,
    /// Precipitation amount in mm
    pub precipitation: f32,
    /// Cloud cover percentage (0-100, optional)
    pub cloud_cover: u8,
    /// Atmospheric pressure in hPa
    pub pressure: f32,
    /// Visibility in kilometers (optional)
    pub visibility: f32,
    /// Human-readable description of weather conditions
    pub description: String,
    /// Weather condition icon ID from API
    pub icon: Option<String>,
}

impl WeatherData {
    /// Convert temperature from Kelvin to Celsius
    #[must_use] 
    pub fn kelvin_to_celsius(kelvin: f32) -> f32 {
        kelvin - 273.15
    }

    /// Convert wind direction from degrees to cardinal direction
    #[must_use] 
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
    #[must_use] 
    pub fn format_temperature(&self) -> String {
        format!("{:.1}°C", self.temperature)
    }

    /// Format wind information
    #[must_use] 
    pub fn format_wind(&self) -> String {
        let direction = Self::wind_direction_to_cardinal(self.wind_direction);
        format!(
            "{:.1} m/s {} (gusts {:.1} m/s)",
            self.wind_speed, direction, self.wind_gust
        )
    }

    /// Check if conditions are suitable for paragliding (basic heuristic)
    #[must_use] 
    pub fn is_suitable_for_paragliding(&self) -> bool {
        // Basic safety criteria for paragliding
        // - Wind speed between 2-15 m/s
        // - No heavy precipitation
        // - Reasonable visibility

        let wind_ok = self.wind_speed >= 2.0 && self.wind_speed <= 15.0;
        let precipitation_ok = self.precipitation < 1.0; // Less than 1mm
        let visibility_ok = self.visibility >= 5.0; // At least 5km

        wind_ok && precipitation_ok && visibility_ok
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
    fn test_paragliding_suitability() {
        let mut weather = WeatherData {
            timestamp: Utc::now(),
            temperature: 15.0,
            wind_speed: 8.0, // Good wind speed
            wind_direction: 180,
            wind_gust: 9.1,
            precipitation: 0.0, // No rain
            cloud_cover: 30,
            pressure: 1013.0,
            visibility: 15.0, // Good visibility
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
}