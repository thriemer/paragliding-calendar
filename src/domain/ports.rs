use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};

use crate::domain::{
    activities::{ActivitySuggestion, PlanningContext},
    calendar::CalendarEvent,
    location::Location,
    weather::{WeatherForecast, WeatherModel},
};

#[cfg_attr(test, mockall::automock)]
#[async_trait(?Send)]
pub trait ActivitySource: Send + Sync {
    async fn suggest(&self, ctx: &PlanningContext) -> Result<Vec<ActivitySuggestion>>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait WeatherProvider: Send + Sync {
    async fn get_forecast(
        &self,
        source: Location,
        model: Option<String>,
    ) -> Result<WeatherForecast>;

    fn available_models(&self) -> Vec<WeatherModel>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait RoutingProvider: Send + Sync {
    async fn get_travel_time(
        &self,
        source: &Location,
        destination: &Location,
    ) -> Result<Duration>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait CalendarProvider {
    async fn is_busy(
        &self,
        calendars: &Vec<String>,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<bool>;
    async fn get_calendar_names(&self) -> Result<Vec<String>>;
    async fn clear_calendar(&mut self, name: &str) -> Result<()>;
    async fn create_event(&mut self, calendar: &str, event: CalendarEvent) -> Result<()>;
    async fn create_calendar(&mut self, name: &str) -> Result<()>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait GeoProvider: Send + Sync {
    async fn geocode(&self, location_name: &str) -> Result<Vec<Location>>;

    async fn fetch_elevation(&self, latitude: f64, longitude: f64) -> Result<f64>;
}
