//! Paragliding Calendar Service
//!
//! This service orchestrates the creation of paragliding events in a calendar
//! based on weather conditions, site evaluations, and user availability.

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};

use crate::{
    cache::Cache,
    calendar::{CalendarEvent, CalendarProvider},
    location::Location,
    paragliding::{ParaglidingSite, ParaglidingSiteProvider, site_evaluator},
    weather::open_meteo,
};

/// Represents a time window when paragliding is feasible
#[derive(Debug, Clone)]
pub struct FlyableWindow {
    pub site: ParaglidingSite,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// Configuration for paragliding calendar creation
pub struct CalendarConfig {
    pub search_radius_km: f64,
    pub minimum_flyable_duration: Duration,
}

impl Default for CalendarConfig {
    fn default() -> Self {
        Self {
            search_radius_km: 150.0,
            minimum_flyable_duration: Duration::hours(2),
        }
    }
}

pub struct ParaglidingCalendarService {
    cache: Cache,
}

impl ParaglidingCalendarService {
    pub fn new(cache: Cache) -> Self {
        Self { cache }
    }

    /// Create calendar events for paragliding based on location and configuration
    pub async fn create_events_for_location<P, C>(
        &self,
        provider: &P,
        location: &Location,
        calendar_provider: &mut C,
        calendar_name: &str,
        config: CalendarConfig,
    ) -> Result<Vec<CalendarEvent>>
    where
        P: ParaglidingSiteProvider,
        C: CalendarProvider,
    {
        // Prepare the calendar
        calendar_provider.create_calendar(calendar_name).await?;

        let mut other_calendar_names = calendar_provider.get_calendar_names().await?;
        other_calendar_names.retain(|n| n != calendar_name);

        // Find nearby sites with weather
        let sites_with_weather = self
            .find_sites_with_weather(provider, location, config.search_radius_km)
            .await;

        // Find flyable windows
        let flyable_windows = self
            .find_flyable_windows(
                location,
                &sites_with_weather,
                calendar_provider,
                &other_calendar_names,
                config.minimum_flyable_duration,
            )
            .await?;

        // Convert to calendar events
        let events = flyable_windows
            .into_iter()
            .map(|window| CalendarEvent {
                summary: window.site.name.clone(),
                start_time: window.start,
                end_time: window.end,
                is_all_day: false,
                location: None,
            })
            .collect();

        Ok(events)
    }

    /// Find nearby paragliding sites with weather forecasts
    async fn find_sites_with_weather<P>(
        &self,
        provider: &P,
        location: &Location,
        radius_km: f64,
    ) -> Vec<(site_evaluator::SiteEvaluationResult, ParaglidingSite)>
    where
        P: ParaglidingSiteProvider,
    {
        let nearby_sites = provider
            .fetch_launches_within_radius(location, radius_km)
            .await;

        let mut result = vec![];

        for (site, _distance) in nearby_sites.iter() {
            if site.mute_alerts == Some(true) {
                tracing::info!("Skipping muted site: {}", site.name);
                continue;
            }

            if let Some(launch) = site.launches.first() {
                let weather_model = site.preferred_weather_model.as_deref();
                match open_meteo::get_forecast(
                    launch.location.clone(),
                    weather_model,
                    self.cache.clone(),
                )
                .await
                {
                    Ok(forecast) => {
                        let evaluation = site_evaluator::evaluate_site(site, &forecast).await;
                        result.push((evaluation, site.clone()));
                    }
                    Err(e) => {
                        tracing::warn!(
                            site = %site.name,
                            lat = %launch.location.latitude,
                            lon = %launch.location.longitude,
                            error = %e,
                            "Failed to get weather forecast"
                        );
                    }
                }
            }
        }
        result
    }

    /// Find flyable windows considering weather and calendar availability
    async fn find_flyable_windows<C>(
        &self,
        location: &Location,
        sites_with_weather: &[(site_evaluator::SiteEvaluationResult, ParaglidingSite)],
        calendar_provider: &C,
        other_calendars: &[String],
        minimum_duration: Duration,
    ) -> Result<Vec<FlyableWindow>>
    where
        C: CalendarProvider,
    {
        let mut windows = Vec::new();

        for (eval, site) in sites_with_weather {
            let drive_to_site = Duration::seconds(
                crate::routing::get_travel_time(
                    location,
                    &site.launches[0].location,
                    self.cache.clone(),
                )
                .await? as i64,
            );

            for mut daily_summary in eval.daily_summaries.clone() {
                // Filter hourly scores based on calendar availability
                let available_scores = self
                    .filter_available_hours(
                        &daily_summary.hourly_scores,
                        calendar_provider,
                        other_calendars,
                    )
                    .await?;

                daily_summary.hourly_scores = available_scores;
                daily_summary.calculate_flyable_time_ranges();

                // Adjust ranges for travel time and filter by minimum duration
                for mut range in daily_summary.ranges {
                    // Assumption that driving time is symmetrical
                    range.start += drive_to_site;
                    range.end -= drive_to_site;

                    if range.is_longer_than(minimum_duration + minimum_duration) {
                        windows.push(FlyableWindow {
                            site: site.clone(),
                            start: range.start,
                            end: range.end,
                        });
                    }
                }
            }
        }

        Ok(windows)
    }

    /// Filter hourly scores based on calendar availability
    async fn filter_available_hours<C>(
        &self,
        hourly_scores: &[site_evaluator::HourlyScore],
        calendar_provider: &C,
        other_calendars: &[String],
    ) -> Result<Vec<site_evaluator::HourlyScore>>
    where
        C: CalendarProvider,
    {
        use futures::future;

        let mut scores: Vec<(site_evaluator::HourlyScore, bool)> = future::join_all(
            hourly_scores
                .iter()
                .map(|h| async {
                    let h = h.clone();
                    let recommended = h.is_flyable
                        && !calendar_provider
                            .is_busy(
                                &other_calendars.to_vec(),
                                h.timestamp - Duration::minutes(30),
                                h.timestamp + Duration::minutes(30),
                            )
                            .await
                            .unwrap_or(false); // Default to false if busy check fails
                    (h, recommended)
                })
                .collect::<Vec<_>>(),
        )
        .await;

        scores.retain(|(_, recommendation)| *recommendation);
        let filtered_scores = scores.into_iter().map(|(score, _)| score).collect();

        Ok(filtered_scores)
    }
}
