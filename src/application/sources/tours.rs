use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{Datelike, Duration, NaiveDate};

use super::with_source_url;
use crate::application::preference_scorer::PreferenceScorer;
use crate::domain::{
    activities::{ActivitySuggestion, Score, TimeWindow, Timing, kind_from_category},
    plan::PlanningContext,
    ports::{ActivitySource, SettingsRepository, TourRepository, WeatherProvider},
    scoring::tours::weather_suitability,
    weather::{self, WeatherData},
};

/// Weather grid for tours. Tours don't need paragliding's 100 m accuracy, so we snap each tour to a
/// ~10 km cell (0.1° ≈ 7–11 km at mid-latitudes) before the forecast lookup. Nearby tours then share
/// one cached forecast instead of firing an API call each.
const WEATHER_GRID_DEG: f64 = 0.1;

/// Surfaces outdoor-active *tours* (hiking, biking, running, mountain climbing, kayaking) as
/// weather- and daylight-aware planner candidates. One source produces several `ActivityKind`s —
/// the kind is derived per tour from its category. Only loop tours are used for now (start == end
/// keeps routing simple).
pub struct TourActivitySource {
    tour_repo: Arc<dyn TourRepository>,
    settings_repo: Arc<dyn SettingsRepository>,
    weather: Arc<dyn WeatherProvider>,
    scorer: Arc<PreferenceScorer>,
}

impl TourActivitySource {
    pub fn new(
        tour_repo: Arc<dyn TourRepository>,
        settings_repo: Arc<dyn SettingsRepository>,
        weather: Arc<dyn WeatherProvider>,
        scorer: Arc<PreferenceScorer>,
    ) -> Self {
        Self {
            tour_repo,
            settings_repo,
            weather,
            scorer,
        }
    }
}

#[async_trait]
impl ActivitySource for TourActivitySource {
    async fn suggest(&self, ctx: &PlanningContext) -> Result<Vec<ActivitySuggestion>> {
        let settings = self.settings_repo.get().await?.unwrap_or_default();

        // Fetch all categories; loops-only + kind mapping filter in Rust. No result cap — radius +
        // loop filters are assumed to leave a small subset.
        let tours = self
            .tour_repo
            .find_within_radius(&ctx.home, settings.search_radius_km)
            .await?;

        // Learned preference multiplier per activity (base_pref + feature
        // weights, squashed to (0,1)); replaces the old editorial intrinsic
        // quality (PLAN.md Phase 4). One snapshot for the whole pass.
        let scorer = self.scorer.snapshot();
        let mut out = Vec::new();
        for (tour, _distance) in tours {
            if !tour.is_loop {
                continue;
            }
            let Some(kind) = kind_from_category(&tour.category) else {
                tracing::debug!(category = %tour.category, "Skipping tour of unmapped category");
                continue;
            };

            let grid_cell = tour.location.snapped_to_grid(WEATHER_GRID_DEG);
            let forecast = match self.weather.get_forecast(grid_cell, None).await {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!(tour = %tour.title, error = %e, "Failed to get weather forecast");
                    continue;
                }
            };

            // Group the forecast by calendar day so each in-season day yields one daylight window.
            let mut by_day: HashMap<NaiveDate, Vec<WeatherData>> = HashMap::new();
            for wd in forecast.forecast {
                by_day
                    .entry(wd.timestamp.date_naive())
                    .or_default()
                    .push(wd);
            }

            for (date, mut day) in by_day {
                if !tour.in_season(date.month()) {
                    continue;
                }
                let Ok((sunrise, sunset)) = weather::get_sunrise_sunset(&tour.location, date)
                else {
                    continue;
                };
                day.sort_by_key(|w| w.timestamp);
                let daylight: Vec<&WeatherData> = day
                    .iter()
                    .filter(|w| w.timestamp >= sunrise && w.timestamp <= sunset)
                    .collect();
                if daylight.is_empty() {
                    continue;
                }

                let quality = scorer.quality(kind, &tour.id);
                let mut hourly = Vec::with_capacity(daylight.len());
                let mut reasons = Vec::with_capacity(daylight.len());
                for wd in &daylight {
                    let (factor, reason) = weather_suitability(kind, wd);
                    hourly.push(quality * factor);
                    reasons.push(reason);
                }

                let window_start = daylight.first().unwrap().timestamp;
                let window_end = daylight.last().unwrap().timestamp + Duration::hours(1);
                let score = Score {
                    window_start,
                    hourly,
                    reasons,
                };

                out.push(ActivitySuggestion {
                    id: tour.id.clone(),
                    kind,
                    location: tour.location.clone(),
                    timing: Timing::ExactDuration {
                        window: TimeWindow {
                            start: window_start,
                            end: window_end,
                        },
                        duration: Duration::minutes(tour.duration_minutes as i64),
                    },
                    title: tour.title.clone(),
                    description: with_source_url(&tour.description, &tour.source_url),
                    score: Some(score),
                    allow_multiple: false,
                });
            }
        }

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        activities::ActivityKind,
        location::Location,
        ports::{MockSettingsRepository, MockTourRepository, MockWeatherProvider},
        settings::UserSettings,
        tour::Tour,
        weather::WeatherForecast,
    };
    use chrono::{TimeZone, Utc};

    fn home() -> Location {
        Location::new(50.7, 13.0, "Home".into(), "DE".into())
    }

    fn tour(category: &str, is_loop: bool, season: u16) -> Tour {
        Tour {
            id: "1".into(),
            title: "T".into(),
            category: category.into(),
            location: Location::new(50.75, 13.05, "T".into(), "DE".into()),
            description: String::new(),
            duration_minutes: 120,
            length_meters: 8000,
            ascent_meters: 200,
            descent_meters: 200,
            difficulty: 2,
            stamina: 2,
            landscape: 6,
            experience: 6,
            is_loop,
            season_bitmask: season,
            source_url: String::new(),
            image_urls: vec![],
            raw_json: String::new(),
        }
    }

    fn settings() -> UserSettings {
        UserSettings {
            location_name: "Home".into(),
            location_latitude: 50.7,
            location_longitude: 13.0,
            search_radius_km: 100.0,
            calendar_name: "Outdoor".into(),
            minimum_flyable_hours: 1,
            excluded_calendar_names: vec![],
        }
    }

    fn mock_settings() -> MockSettingsRepository {
        let mut s = MockSettingsRepository::new();
        s.expect_get().returning(|| Ok(Some(settings())));
        s
    }

    // A clear, mild June day 04:00–21:00 so the daylight filter keeps a healthy window.
    fn nice_forecast() -> WeatherForecast {
        let day = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        WeatherForecast {
            location: Location::new(50.75, 13.05, "T".into(), "DE".into()),
            forecast: (4..22)
                .map(|h| WeatherData {
                    timestamp: day + Duration::hours(h),
                    temperature: 18.0,
                    wind_speed_ms: 3.0,
                    wind_direction: 180,
                    wind_gust_ms: 4.0,
                    precipitation: 0.0,
                    cloud_cover: 10,
                    pressure: 1015.0,
                    visibility: Some(10.0),
                    description: String::new(),
                })
                .collect(),
        }
    }

    // Moderate wet + windy day: hiking tolerates it, kayaking (wind-on-water) does not.
    fn windy_wet_forecast() -> WeatherForecast {
        let day = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        WeatherForecast {
            location: Location::new(50.75, 13.05, "T".into(), "DE".into()),
            forecast: (4..22)
                .map(|h| WeatherData {
                    timestamp: day + Duration::hours(h),
                    temperature: 12.0,
                    wind_speed_ms: 10.0,
                    wind_direction: 180,
                    wind_gust_ms: 15.0,
                    precipitation: 3.0,
                    cloud_cover: 100,
                    pressure: 1000.0,
                    visibility: Some(5.0),
                    description: String::new(),
                })
                .collect(),
        }
    }

    fn ctx() -> PlanningContext {
        PlanningContext {
            home: home(),
            horizon: TimeWindow {
                start: Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap(),
                end: Utc.with_ymd_and_hms(2026, 6, 14, 0, 0, 0).unwrap(),
            },
            conflict_calendars: vec![],
        }
    }

    async fn run(tours: Vec<Tour>, forecast: WeatherForecast) -> Vec<ActivitySuggestion> {
        let mut repo = MockTourRepository::new();
        repo.expect_find_within_radius()
            .returning(move |_, _| Ok(tours.iter().cloned().map(|t| (t, 5.0)).collect()));
        let mut weather = MockWeatherProvider::new();
        weather
            .expect_get_forecast()
            .returning(move |_, _| Ok(forecast.clone()));
        let source = TourActivitySource::new(
            Arc::new(repo),
            Arc::new(mock_settings()),
            Arc::new(weather),
            Arc::new(PreferenceScorer::new()),
        );
        source.suggest(&ctx()).await.unwrap()
    }

    #[tokio::test]
    async fn non_loop_tour_is_skipped() {
        let out = run(
            vec![tour("Wanderung", false, 0b1111_1111_1111)],
            nice_forecast(),
        )
        .await;
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn out_of_season_tour_yields_nothing() {
        // Season = January only; horizon is June.
        let out = run(
            vec![tour("Wanderung", true, 0b0000_0000_0001)],
            nice_forecast(),
        )
        .await;
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn in_season_loop_produces_one_daylight_suggestion() {
        let out = run(
            vec![tour("Wanderung", true, 0b1111_1111_1111)],
            nice_forecast(),
        )
        .await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, ActivityKind::Hiking);
        let score = out[0].score.as_ref().unwrap();
        assert!(score.total() > 0.0, "nice weather should score > 0");
        assert_eq!(score.hourly.len(), score.reasons.len());
    }

    #[tokio::test]
    async fn kind_specific_weather_hiking_beats_kayaking_when_windy() {
        // Same windy/wet day: wind on open water crushes kayaking far more than hiking.
        let hike = run(
            vec![tour("Wanderung", true, 0b1111_1111_1111)],
            windy_wet_forecast(),
        )
        .await;
        let kayak = run(
            vec![tour("Kanu", true, 0b1111_1111_1111)],
            windy_wet_forecast(),
        )
        .await;
        let hike_fun = hike[0].score.as_ref().unwrap().total();
        let kayak_fun = kayak[0].score.as_ref().unwrap().total();
        assert!(
            hike_fun > kayak_fun,
            "hike {hike_fun} should beat kayak {kayak_fun} when windy"
        );
    }
}
