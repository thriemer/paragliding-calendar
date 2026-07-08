use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{Datelike, Duration, NaiveDate};

use crate::adapters::activities::outdooractive_link;
use crate::domain::{
    activities::{ActivityKind, ActivitySuggestion, PlanningContext, Score, TimeWindow, Timing},
    hiking::OutdoorTour,
    ports::{ActivitySource, OutdoorTourRepository, SettingsRepository, WeatherProvider},
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
    tour_repo: Arc<dyn OutdoorTourRepository>,
    settings_repo: Arc<dyn SettingsRepository>,
    weather: Arc<dyn WeatherProvider>,
}

impl TourActivitySource {
    pub fn new(
        tour_repo: Arc<dyn OutdoorTourRepository>,
        settings_repo: Arc<dyn SettingsRepository>,
        weather: Arc<dyn WeatherProvider>,
    ) -> Self {
        Self { tour_repo, settings_repo, weather }
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
                by_day.entry(wd.timestamp.date_naive()).or_default().push(wd);
            }

            for (date, mut day) in by_day {
                if !tour.in_season(date.month()) {
                    continue;
                }
                let Ok((sunrise, sunset)) = weather::get_sunrise_sunset(&tour.location, date) else {
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

                let quality = intrinsic_quality(&tour);
                let mut hourly = Vec::with_capacity(daylight.len());
                let mut reasons = Vec::with_capacity(daylight.len());
                for wd in &daylight {
                    let (factor, reason) = weather_suitability(kind, wd);
                    hourly.push(quality * factor);
                    reasons.push(reason);
                }

                let window_start = daylight.first().unwrap().timestamp;
                let window_end = daylight.last().unwrap().timestamp + Duration::hours(1);
                let score = Score { window_start, hourly, reasons };

                out.push(ActivitySuggestion {
                    kind,
                    location: tour.location.clone(),
                    timing: Timing::ExactDuration {
                        window: TimeWindow { start: window_start, end: window_end },
                        duration: Duration::minutes(tour.duration_minutes as i64),
                    },
                    title: tour.title.clone(),
                    description: outdooractive_link(&tour.description, &tour.id),
                    score: Some(score),
                });
            }
        }

        Ok(out)
    }
}

/// Maps the outdoor-active German category title to an `ActivityKind`. Unmapped / out-of-scope
/// categories (winter sports, motorised, equestrian, skating) return `None` and are skipped.
/// Substring match on the lowercased title so minor title variants still land.
pub fn kind_from_category(category: &str) -> Option<ActivityKind> {
    let c = category.to_lowercase();
    let has = |needle: &str| c.contains(needle);

    // Order matters: check biking/running before generic "wander" fallthrough isn't needed since
    // categories are disjoint, but keep the more specific keywords first regardless.
    if has("mountainbike") || has("radtour") || has("radweg") || has("rennrad") || has("gravel") {
        Some(ActivityKind::Biking)
    } else if has("trailrunning") || has("jogging") {
        Some(ActivityKind::Running)
    } else if has("bergtour") || has("klettersteig") || has("hochtour") || has("alpinklettern") {
        Some(ActivityKind::MountainClimbing)
    } else if has("kanu") || has("kajak") || has("paddel") {
        Some(ActivityKind::Kayaking)
    } else if has("wander")        // Wanderung, Winterwandern, Fernwanderweg
        || has("themenweg")
        || has("pilgerweg")
        || has("stadtrundgang")
        || has("schneeschuh")
        || has("nordic walking")
    {
        Some(ActivityKind::Hiking)
    } else {
        None
    }
}

/// Base per-hour fun from the editorial ratings (the dataset carries no user rating). `landscape`
/// (scenery) and `experience` are 0–6; a floor keeps unrated tours from scoring flat zero.
fn intrinsic_quality(tour: &OutdoorTour) -> f32 {
    let rated = (tour.landscape as f32 + tour.experience as f32) / 12.0; // 0..1
    0.2 + 0.8 * rated
}

/// Per-hour weather sensitivity params, tuned per activity.
struct Sensitivity {
    /// Precipitation (mm/h) at which fun hits zero.
    rain_kill: f32,
    /// Wind (m/s) tolerated with no penalty, and where fun hits zero.
    wind_ok: f32,
    wind_kill: f32,
    /// Comfort temperature and the half-width (°C) of the tolerable band.
    ideal_temp: f32,
    temp_span: f32,
}

// ponytail: hand-tuned starting values, one knob-set per kind. Adjust from real forecasts/feedback
// rather than adding config until there's a reason to.
fn sensitivity(kind: ActivityKind) -> Sensitivity {
    match kind {
        // Wet rock / exposed ridges — rain and wind matter most.
        ActivityKind::MountainClimbing => {
            Sensitivity { rain_kill: 2.0, wind_ok: 8.0, wind_kill: 15.0, ideal_temp: 15.0, temp_span: 18.0 }
        }
        // Wind on open water is the killer.
        ActivityKind::Kayaking => {
            Sensitivity { rain_kill: 6.0, wind_ok: 5.0, wind_kill: 12.0, ideal_temp: 20.0, temp_span: 18.0 }
        }
        // Dislikes rain; heat-sensitive (narrow, cool comfort band).
        ActivityKind::Running => {
            Sensitivity { rain_kill: 6.0, wind_ok: 10.0, wind_kill: 20.0, ideal_temp: 12.0, temp_span: 14.0 }
        }
        ActivityKind::Biking => {
            Sensitivity { rain_kill: 4.0, wind_ok: 8.0, wind_kill: 18.0, ideal_temp: 18.0, temp_span: 18.0 }
        }
        // Hiking (and the default) — most weather-tolerant.
        _ => Sensitivity { rain_kill: 8.0, wind_ok: 12.0, wind_kill: 25.0, ideal_temp: 15.0, temp_span: 20.0 },
    }
}

/// A 0..1 fun multiplier for the hour plus a human-readable reason (rides into the calendar body).
/// Rain and wind gate hard (multiply toward zero); temperature softens.
fn weather_suitability(kind: ActivityKind, wd: &WeatherData) -> (f32, String) {
    let s = sensitivity(kind);

    let rain = (1.0 - wd.precipitation / s.rain_kill).clamp(0.0, 1.0);
    let wind = if wd.wind_speed_ms <= s.wind_ok {
        1.0
    } else {
        (1.0 - (wd.wind_speed_ms - s.wind_ok) / (s.wind_kill - s.wind_ok)).clamp(0.0, 1.0)
    };
    let temp = (1.0 - (wd.temperature - s.ideal_temp).abs() / s.temp_span).clamp(0.0, 1.0);

    let factor = rain * wind * (0.5 + 0.5 * temp);
    let reason = format!(
        "{:.0} °C, {:.1} mm rain, {:.0} m/s wind",
        wd.temperature, wd.precipitation, wd.wind_speed_ms
    );
    (factor, reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        location::Location,
        paragliding::UserSettings,
        ports::{MockOutdoorTourRepository, MockSettingsRepository, MockWeatherProvider},
        weather::WeatherForecast,
    };
    use chrono::{TimeZone, Utc};

    fn home() -> Location {
        Location::new(50.7, 13.0, "Home".into(), "DE".into())
    }

    fn tour(category: &str, is_loop: bool, season: u16) -> OutdoorTour {
        OutdoorTour {
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

    #[test]
    fn category_mapping_covers_the_five_kinds() {
        assert_eq!(kind_from_category("Wanderung"), Some(ActivityKind::Hiking));
        assert_eq!(kind_from_category("Winterwandern"), Some(ActivityKind::Hiking));
        assert_eq!(kind_from_category("Mountainbike"), Some(ActivityKind::Biking));
        assert_eq!(kind_from_category("Radtour"), Some(ActivityKind::Biking));
        assert_eq!(kind_from_category("Trailrunning"), Some(ActivityKind::Running));
        assert_eq!(kind_from_category("Bergtour"), Some(ActivityKind::MountainClimbing));
        assert_eq!(kind_from_category("Kanu"), Some(ActivityKind::Kayaking));
        // Out of scope → skipped.
        assert_eq!(kind_from_category("Skitour"), None);
        assert_eq!(kind_from_category("Motorrad"), None);
        assert_eq!(kind_from_category("Reiten"), None);
    }

    async fn run(tours: Vec<OutdoorTour>, forecast: WeatherForecast) -> Vec<ActivitySuggestion> {
        let mut repo = MockOutdoorTourRepository::new();
        repo.expect_find_within_radius()
            .returning(move |_, _| Ok(tours.iter().cloned().map(|t| (t, 5.0)).collect()));
        let mut weather = MockWeatherProvider::new();
        weather.expect_get_forecast().returning(move |_, _| Ok(forecast.clone()));
        let source =
            TourActivitySource::new(Arc::new(repo), Arc::new(mock_settings()), Arc::new(weather));
        source.suggest(&ctx()).await.unwrap()
    }

    #[tokio::test]
    async fn non_loop_tour_is_skipped() {
        let out = run(vec![tour("Wanderung", false, 0b1111_1111_1111)], nice_forecast()).await;
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn out_of_season_tour_yields_nothing() {
        // Season = January only; horizon is June.
        let out = run(vec![tour("Wanderung", true, 0b0000_0000_0001)], nice_forecast()).await;
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn in_season_loop_produces_one_daylight_suggestion() {
        let out = run(vec![tour("Wanderung", true, 0b1111_1111_1111)], nice_forecast()).await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, ActivityKind::Hiking);
        let score = out[0].score.as_ref().unwrap();
        assert!(score.total() > 0.0, "nice weather should score > 0");
        assert_eq!(score.hourly.len(), score.reasons.len());
    }

    #[tokio::test]
    async fn kind_specific_weather_hiking_beats_kayaking_when_windy() {
        // Same windy/wet day: wind on open water crushes kayaking far more than hiking.
        let hike = run(vec![tour("Wanderung", true, 0b1111_1111_1111)], windy_wet_forecast()).await;
        let kayak = run(vec![tour("Kanu", true, 0b1111_1111_1111)], windy_wet_forecast()).await;
        let hike_fun = hike[0].score.as_ref().unwrap().total();
        let kayak_fun = kayak[0].score.as_ref().unwrap().total();
        assert!(hike_fun > kayak_fun, "hike {hike_fun} should beat kayak {kayak_fun} when windy");
    }
}
