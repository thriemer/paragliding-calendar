//! Weather forecast model and factory methods

use super::{Location, WeatherData};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sunrise::{Coordinates, SolarDay, SolarEvent};

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

impl WeatherForecast {
    /// Create new forecast
    #[must_use] 
    pub fn new(location: Location, forecasts: Vec<WeatherData>) -> Self {
        Self {
            location,
            forecasts,
            retrieved_at: Utc::now(),
        }
    }

    /// Get current weather (first forecast item)
    #[must_use] 
    pub fn current_weather(&self) -> Option<&WeatherData> {
        self.forecasts.first()
    }

    /// Get weather for a specific day (returns all forecasts for that day)
    #[must_use] 
    pub fn daily_forecast(&self, day_offset: usize) -> Vec<&WeatherData> {
        if self.forecasts.is_empty() {
            return Vec::new();
        }

        let base_date = self.forecasts[0].timestamp.date_naive();
        let target_date = base_date + chrono::Duration::days(i64::try_from(day_offset).unwrap_or(0));

        self.forecasts
            .iter()
            .filter(|w| w.timestamp.date_naive() == target_date)
            .collect()
    }

    /// Check if forecast data is still fresh (not older than cache TTL)
    #[must_use] 
    pub fn is_fresh(&self, ttl_hours: u32) -> bool {
        let age = Utc::now() - self.retrieved_at;
        age.num_hours() < i64::from(ttl_hours)
    }

    /// Filter weather data to only include daylight hours (sunrise to sunset)
    /// Returns weather data points that fall between sunrise and sunset for the given date and location
    #[must_use]
    pub fn filter_daylight_hours(&self, date: NaiveDate, latitude: f64, longitude: f64) -> Vec<&WeatherData> {
        // Calculate sunrise and sunset for the given location and date
        let coords = match Coordinates::new(latitude, longitude) {
            Some(coords) => coords,
            None => {
                // Invalid coordinates, use approximate daylight hours (6 AM to 8 PM)
                return self.filter_approximate_daylight_hours(date);
            }
        };

        let solar_day = SolarDay::new(coords, date);
        
        // Get sunrise and sunset times 
        let sunrise_dt = solar_day.event_time(SolarEvent::Sunrise);
        let sunset_dt = solar_day.event_time(SolarEvent::Sunset);
        
        // Filter weather data to only include daylight hours
        self.forecasts
            .iter()
            .filter(|weather| {
                weather.timestamp >= sunrise_dt && weather.timestamp <= sunset_dt
            })
            .collect()
    }

    /// Fallback method for filtering daylight hours using approximate times (6 AM to 8 PM)
    fn filter_approximate_daylight_hours(&self, date: NaiveDate) -> Vec<&WeatherData> {
        let day_start = Utc.from_local_datetime(&date.and_hms_opt(6, 0, 0).unwrap()).earliest();
        let day_end = Utc.from_local_datetime(&date.and_hms_opt(20, 0, 0).unwrap()).earliest();
        
        match (day_start, day_end) {
            (Some(start), Some(end)) => {
                self.forecasts
                    .iter()
                    .filter(|weather| {
                        weather.timestamp >= start && weather.timestamp <= end
                    })
                    .collect()
            }
            _ => {
                // Last resort: return all weather for the day
                self.daily_forecast(0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                wind_gust: 13.1,
                precipitation: 0.0,
                cloud_cover: 20,
                pressure: 1013.0,
                visibility: 10.0,
                description: "Clear".to_string(),
                icon: None,
            },
            WeatherData {
                timestamp: base_time + chrono::Duration::days(1),
                temperature: 18.0,
                wind_speed: 7.0,
                wind_direction: 200,
                wind_gust: 9.1,
                precipitation: 0.2,
                cloud_cover: 40,
                pressure: 1015.0,
                visibility: 12.0,
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