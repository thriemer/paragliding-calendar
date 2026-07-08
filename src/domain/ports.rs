use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};

use crate::domain::{
    activities::{ActivitySuggestion, Plan, PlanningContext, ScheduledActivity, TimeWindow},
    calendar::CalendarEvent,
    location::Location,
    paragliding::ParaglidingSite,
    weather::{WeatherForecast, WeatherModel},
};

pub struct SolverInput {
    pub candidates: Vec<ActivitySuggestion>,
    pub origin: Location,
    pub free_slots: Vec<TimeWindow>,
    /// Fixed calendar commitments (fun = 0) the plan must schedule around.
    pub fixed: Vec<ScheduledActivity>,
    pub num_alternatives: usize,
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait WeekSolver: Send + Sync {
    async fn solve(&self, input: SolverInput) -> Result<Vec<Plan>>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
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

    /// All pairwise drive times among `locations`, row-major and aligned to input order:
    /// `matrix[i][j]` is the drive from `locations[i]` to `locations[j]` (diagonal = zero).
    /// Default builds it pairwise via `get_travel_time` for providers without a matrix API.
    async fn travel_time_matrix(&self, locations: &[Location]) -> Result<Vec<Vec<Duration>>> {
        let mut rows = Vec::with_capacity(locations.len());
        for from in locations {
            let mut row = Vec::with_capacity(locations.len());
            for to in locations {
                if from.to_key() == to.to_key() {
                    row.push(Duration::zero());
                } else {
                    row.push(self.get_travel_time(from, to).await?);
                }
            }
            rows.push(row);
        }
        Ok(rows)
    }
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait CalendarProvider: Send + Sync {
    /// Concrete events in `[start, end]` across `calendars`, expanded from recurrences.
    async fn get_events(
        &self,
        calendars: &[String],
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<CalendarEvent>>;
    async fn get_calendar_names(&self) -> Result<Vec<String>>;
    async fn clear_calendar(&self, name: &str) -> Result<()>;
    async fn create_event(&self, calendar: &str, event: CalendarEvent) -> Result<()>;
    async fn create_calendar(&self, name: &str) -> Result<()>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait GeoProvider: Send + Sync {
    async fn geocode(&self, location_name: &str) -> Result<Vec<Location>>;

    async fn fetch_elevation(&self, latitude: f64, longitude: f64) -> Result<f64>;
}

use crate::domain::{hiking::OutdoorTour, outdooractive::OutdoorEvent, paragliding::UserSettings};

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait OutdoorTourRepository: Send + Sync {
    async fn count(&self) -> Result<i64>;
    async fn save_batch(&self, tours: Vec<OutdoorTour>) -> Result<usize>;
    async fn find_within_radius(
        &self,
        center: &Location,
        radius_km: f64,
    ) -> Result<Vec<(OutdoorTour, f64)>>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait SiteRepository: Send + Sync {
    async fn save(&self, site: ParaglidingSite) -> Result<()>;
    async fn delete(&self, name: &str) -> Result<()>;
    async fn find_all(&self) -> Result<Vec<ParaglidingSite>>;
    async fn find_within_radius(
        &self,
        center: &Location,
        radius_km: f64,
    ) -> Result<Vec<(ParaglidingSite, f64)>>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait EventRepository: Send + Sync {
    async fn count(&self) -> Result<i64>;
    async fn save_batch(&self, events: Vec<OutdoorEvent>) -> Result<usize>;
    async fn find_within_radius_and_time(
        &self,
        center: &Location,
        radius_km: f64,
        time_from: DateTime<Utc>,
        time_to: DateTime<Utc>,
    ) -> Result<Vec<(OutdoorEvent, f64)>>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait SettingsRepository: Send + Sync {
    async fn get(&self) -> Result<Option<UserSettings>>;
    async fn save(&self, settings: &UserSettings) -> Result<()>;
}
