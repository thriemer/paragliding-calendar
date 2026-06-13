use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use sunrise::{Coordinates, SolarDay, SolarEvent};

use crate::domain::location::Location;

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

}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherModel {
    pub id: String,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn kelvin_to_celsius_known_values() {
        assert!((WeatherData::kelvin_to_celsius(273.15) - 0.0).abs() < 0.001);
        assert!((WeatherData::kelvin_to_celsius(373.15) - 100.0).abs() < 0.001);
        assert!((WeatherData::kelvin_to_celsius(0.0) - -273.15).abs() < 0.001);
    }

    #[rstest]
    #[case(0, "N")]
    #[case(11, "N")]
    #[case(349, "N")]
    #[case(360, "N")]
    #[case(22, "NNE")]
    #[case(45, "NE")]
    #[case(67, "ENE")]
    #[case(90, "E")]
    #[case(112, "ESE")]
    #[case(135, "SE")]
    #[case(157, "SSE")]
    #[case(180, "S")]
    #[case(202, "SSW")]
    #[case(225, "SW")]
    #[case(247, "WSW")]
    #[case(270, "W")]
    #[case(292, "WNW")]
    #[case(315, "NW")]
    #[case(337, "NNW")]
    fn wind_direction_to_cardinal_cases(#[case] deg: u16, #[case] expected: &str) {
        assert_eq!(WeatherData::wind_direction_to_cardinal(deg), expected);
    }

    #[test]
    fn sunrise_sunset_returns_sunrise_before_sunset() {
        let loc = Location::new(50.7, 13.0, "Test".into(), "DE".into());
        let date = chrono::NaiveDate::from_ymd_opt(2026, 6, 13).unwrap();
        let (sunrise, sunset) = get_sunrise_sunset(&loc, date).unwrap();
        assert!(sunrise < sunset);
        assert_eq!(sunrise.date_naive(), date);
        assert_eq!(sunset.date_naive(), date);
    }
}
