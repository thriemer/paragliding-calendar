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

/// The latest you may still be en route on `date`: you must reach your overnight location (home or,
/// later, a campsite) at least one hour before sunset there. Falls back to 18:00 UTC (19:00 sunset
/// − 1h) only if the coordinates are invalid — `get_sunrise_sunset` already handles missing solar
/// events internally.
pub fn overnight_deadline(location: &Location, date: NaiveDate) -> DateTime<Utc> {
    match get_sunrise_sunset(location, date) {
        Ok((_, sunset)) => sunset - chrono::Duration::hours(1),
        Err(_) => date
            .and_time(NaiveTime::from_hms_opt(18, 0, 0).unwrap())
            .and_utc(),
    }
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
    /// Visibility in meters; `None` when the model doesn't provide it (unused in scoring)
    pub visibility: Option<f32>,
    /// Human-readable description of weather conditions
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherModel {
    pub id: String,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

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
