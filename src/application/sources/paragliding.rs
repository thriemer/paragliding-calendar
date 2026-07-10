use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Duration;

use crate::domain::{
    activities::{ActivityKind, ActivitySuggestion, Score, TimeWindow, Timing},
    plan::PlanningContext,
    ports::{ActivitySource, SettingsRepository, SiteRepository, WeatherProvider},
    scoring::paragliding as site_evaluator,
};

pub struct ParaglidingActivitySource {
    site_repo: Arc<dyn SiteRepository>,
    settings_repo: Arc<dyn SettingsRepository>,
    weather: Arc<dyn WeatherProvider>,
}

impl ParaglidingActivitySource {
    pub fn new(
        site_repo: Arc<dyn SiteRepository>,
        settings_repo: Arc<dyn SettingsRepository>,
        weather: Arc<dyn WeatherProvider>,
    ) -> Self {
        Self {
            site_repo,
            settings_repo,
            weather,
        }
    }
}

#[async_trait]
impl ActivitySource for ParaglidingActivitySource {
    async fn suggest(&self, ctx: &PlanningContext) -> Result<Vec<ActivitySuggestion>> {
        let settings = self.settings_repo.get().await?.unwrap_or_default();
        let min_duration = Duration::hours(settings.minimum_flyable_hours as i64);

        let sites = self
            .site_repo
            .find_within_radius(&ctx.home, settings.search_radius_km)
            .await?;

        let mut out = Vec::new();
        for (site, _distance) in sites {
            if site.mute_alerts == Some(true) {
                tracing::debug!(site = %site.name, "Skipping muted site");
                continue;
            }
            let Some(launch) = site.launches.first() else {
                continue;
            };

            let forecast = match self
                .weather
                .get_forecast(launch.location.clone(), site.preferred_weather_model.clone())
                .await
            {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!(
                        site = %site.name,
                        lat = %launch.location.latitude,
                        lon = %launch.location.longitude,
                        error = %e,
                        "Failed to get weather forecast"
                    );
                    continue;
                }
            };

            let eval = site_evaluator::evaluate_site(&site, &forecast).await;
            for day in eval.daily_summaries {
                for range in day.ranges {
                    let range_scores: Vec<&site_evaluator::HourlyScore> = day
                        .hourly_scores
                        .iter()
                        .filter(|h| h.timestamp >= range.start && h.timestamp <= range.end)
                        .collect();

                    let hourly: Vec<f32> = range_scores.iter().map(|h| h.score).collect();
                    let reasons: Vec<String> =
                        range_scores.iter().map(|h| h.reason.clone()).collect();
                    let score = Score {
                        window_start: range.start,
                        hourly,
                        reasons,
                    };

                    out.push(ActivitySuggestion {
                        // Never used for dedup (allow_multiple below); site name is a stable label.
                        id: site.name.clone(),
                        kind: ActivityKind::Paragliding,
                        location: launch.location.clone(),
                        timing: Timing::Flexible {
                            window: TimeWindow {
                                start: range.start,
                                // `range.end` is the *start* of the last flyable hour; the
                                // window stays open until that hour is over.
                                end: range.end + Duration::hours(1),
                            },
                            min_duration,
                        },
                        title: site.name.clone(),
                        // Per-hour scoring reasons ride along and end up in the calendar
                        // event body, so the event explains *why* the site is flyable.
                        description: score.reasons.join("\n"),
                        score: Some(score),
                        allow_multiple: true,
                    });
                }
            }
        }

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        location::Location,
        paragliding::{ParaglidingLaunch, ParaglidingSite, SiteType},
        ports::{MockSettingsRepository, MockSiteRepository, MockWeatherProvider},
        settings::UserSettings,
        weather::{WeatherData, WeatherForecast},
    };
    use anyhow::anyhow;
    use chrono::{TimeZone, Utc};

    fn home() -> Location {
        Location::new(50.7, 13.0, "Home".into(), "DE".into())
    }

    fn site_loc() -> Location {
        Location::new(50.75, 13.05, "Site".into(), "DE".into())
    }

    fn site(name: &str, mute: Option<bool>, launches: Vec<ParaglidingLaunch>) -> ParaglidingSite {
        ParaglidingSite {
            name: name.into(),
            launches,
            landings: vec![],
            country: Some("DE".into()),
            data_source: "test".into(),
            parking_location: None,
            mute_alerts: mute,
            rating: None,
            preferred_weather_model: None,
        }
    }

    fn hang_launch() -> ParaglidingLaunch {
        ParaglidingLaunch {
            site_type: SiteType::Hang,
            location: site_loc(),
            direction_degrees_start: 0.0,
            direction_degrees_stop: 360.0,
            elevation: 500.0,
        }
    }

    fn default_settings() -> UserSettings {
        UserSettings {
            location_name: "Home".into(),
            location_latitude: 50.7,
            location_longitude: 13.0,
            search_radius_km: 100.0,
            calendar_name: "Paragliding".into(),
            minimum_flyable_hours: 1,
            excluded_calendar_names: vec![],
        }
    }

    fn weather_at(ts: chrono::DateTime<Utc>, wind_speed_ms: f32) -> WeatherData {
        WeatherData {
            timestamp: ts,
            temperature: 20.0,
            wind_speed_ms,
            wind_direction: 180,
            wind_gust_ms: wind_speed_ms,
            precipitation: 0.0,
            cloud_cover: 0,
            pressure: 1013.0,
            visibility: Some(10.0),
            description: String::new(),
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

    fn mock_settings() -> MockSettingsRepository {
        let mut settings = MockSettingsRepository::new();
        settings
            .expect_get()
            .returning(|| Ok(Some(default_settings())));
        settings
    }

    fn bad_weather_forecast() -> WeatherForecast {
        let day = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        WeatherForecast {
            location: site_loc(),
            forecast: (4..22)
                .map(|h| weather_at(day + chrono::Duration::hours(h), 50.0))
                .collect(),
        }
    }

    fn flyable_window_forecast() -> WeatherForecast {
        let day = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        WeatherForecast {
            location: site_loc(),
            forecast: (4..22)
                .map(|h| {
                    let wind = if (10..=14).contains(&h) { 3.0 } else { 50.0 };
                    weather_at(day + chrono::Duration::hours(h), wind)
                })
                .collect(),
        }
    }

    #[tokio::test]
    async fn all_bad_weather_returns_no_suggestions() {
        let mut site_repo = MockSiteRepository::new();
        site_repo
            .expect_find_within_radius()
            .returning(|_, _| Ok(vec![(site("S", None, vec![hang_launch()]), 5.0)]));

        let mut weather = MockWeatherProvider::new();
        weather
            .expect_get_forecast()
            .returning(|_, _| Ok(bad_weather_forecast()));

        let source = ParaglidingActivitySource::new(
            Arc::new(site_repo),
            Arc::new(mock_settings()),
            Arc::new(weather),
        );
        let out = source.suggest(&ctx()).await.unwrap();
        assert!(out.is_empty(), "expected no suggestions, got {:?}", out);
    }

    #[tokio::test]
    async fn flyable_window_produces_one_suggestion() {
        let mut site_repo = MockSiteRepository::new();
        site_repo
            .expect_find_within_radius()
            .returning(|_, _| Ok(vec![(site("S", None, vec![hang_launch()]), 5.0)]));

        let mut weather = MockWeatherProvider::new();
        weather
            .expect_get_forecast()
            .returning(|_, _| Ok(flyable_window_forecast()));

        let source = ParaglidingActivitySource::new(
            Arc::new(site_repo),
            Arc::new(mock_settings()),
            Arc::new(weather),
        );
        let out = source.suggest(&ctx()).await.unwrap();
        assert_eq!(out.len(), 1);
        let Timing::Flexible { window, .. } = &out[0].timing else {
            panic!("expected Flexible timing, got {:?}", out[0].timing);
        };
        let day = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        assert_eq!(window.start, day + chrono::Duration::hours(10));
        assert_eq!(window.end, day + chrono::Duration::hours(15));
        assert_eq!(out[0].title, "S");
        let score = out[0].score.as_ref().expect("expected a score");
        assert!((score.total() - 4.91).abs() < 0.1, "expected score ~4.91, got {}", score.total());
        assert_eq!(score.hourly.len(), 5, "expected one hourly bucket per hour");
        assert_eq!(score.reasons.len(), 5, "expected one reason per hour");
    }

    #[tokio::test]
    async fn muted_site_is_skipped_without_calling_weather() {
        let mut site_repo = MockSiteRepository::new();
        site_repo
            .expect_find_within_radius()
            .returning(|_, _| Ok(vec![(site("Muted", Some(true), vec![hang_launch()]), 5.0)]));

        let mut weather = MockWeatherProvider::new();
        weather.expect_get_forecast().times(0);

        let source = ParaglidingActivitySource::new(
            Arc::new(site_repo),
            Arc::new(mock_settings()),
            Arc::new(weather),
        );
        let out = source.suggest(&ctx()).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn site_without_launches_is_skipped() {
        let mut site_repo = MockSiteRepository::new();
        site_repo
            .expect_find_within_radius()
            .returning(|_, _| Ok(vec![(site("NoLaunches", None, vec![]), 5.0)]));

        let mut weather = MockWeatherProvider::new();
        weather.expect_get_forecast().times(0);

        let source = ParaglidingActivitySource::new(
            Arc::new(site_repo),
            Arc::new(mock_settings()),
            Arc::new(weather),
        );
        let out = source.suggest(&ctx()).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn weather_error_skips_site_without_panicking() {
        let mut site_repo = MockSiteRepository::new();
        site_repo
            .expect_find_within_radius()
            .returning(|_, _| Ok(vec![(site("S", None, vec![hang_launch()]), 5.0)]));

        let mut weather = MockWeatherProvider::new();
        weather
            .expect_get_forecast()
            .returning(|_, _| Err(anyhow!("upstream timeout")));

        let source = ParaglidingActivitySource::new(
            Arc::new(site_repo),
            Arc::new(mock_settings()),
            Arc::new(weather),
        );
        let out = source.suggest(&ctx()).await.unwrap();
        assert!(out.is_empty());
    }
}
