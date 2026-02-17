use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use sunrise::{Coordinates, SolarDay, SolarEvent};

use crate::location::Location;

pub mod open_meteo;

pub fn get_sunrise_sunset(
    location: &Location,
    date: NaiveDate,
) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
    let coordinates =
        Coordinates::new(location.latitude, location.longitude).with_context(|| {
            format!(
                "Invalid coordinates: lat={}, lng={}",
                location.latitude, location.longitude
            )
        })?;

    let solar_day = SolarDay::new(coordinates, date);

    let sunrise = solar_day.event_time(SolarEvent::Sunrise).unwrap_or(
        date.and_time(NaiveTime::from_hms_opt(6, 0, 0).unwrap())
            .and_utc(),
    );

    let sunset = solar_day.event_time(SolarEvent::Sunset).unwrap_or(
        date.and_time(NaiveTime::from_hms_opt(19, 0, 0).unwrap())
            .and_utc(),
    );

    Ok((sunrise, sunset))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WeatherForecast {
    pub location: Location,
    pub forecast: Vec<WeatherData>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WeatherData {
    /// Timestamp for this weather observation
    pub timestamp: DateTime<Utc>,
    /// Temperature in Celsius
    pub temperature: f32,
    /// Wind speed in m/s
    pub wind_speed_ms: f32,
    /// Wind direction in degrees (0-360, where 0/360 is North)
    pub wind_direction: u16,
    /// Wind gust speed in m/s
    pub wind_gust_ms: f32,
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
}

impl WeatherData {
    pub fn kelvin_to_celsius(kelvin: f32) -> f32 {
        kelvin - 273.15
    }

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
            self.wind_speed_ms, direction, self.wind_gust_ms
        )
    }

    /// Format weather description with proper capitalization
    #[must_use]
    pub fn format_description(&self) -> String {
        self.description.clone()
    }

    /// Format atmospheric pressure with unit
    #[must_use]
    pub fn format_pressure(&self) -> String {
        format!("{:.1} hPa", self.pressure)
    }
}
