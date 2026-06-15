use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Duration;

use crate::{
    adapters::activities::paragliding::{repository::ParaglidingSiteRepository, site_evaluator},
    domain::{
        activities::{ActivityKind, ActivitySuggestion, PlanningContext, TimeWindow, Timing},
        paragliding::ParaglidingSiteProvider,
        ports::{ActivitySource, WeatherProvider},
    },
};

pub struct ParaglidingActivitySource {
    site_repo: Arc<ParaglidingSiteRepository>,
    weather: Arc<dyn WeatherProvider>,
}

impl ParaglidingActivitySource {
    pub fn new(
        site_repo: Arc<ParaglidingSiteRepository>,
        weather: Arc<dyn WeatherProvider>,
    ) -> Self {
        Self { site_repo, weather }
    }
}

#[async_trait]
impl ActivitySource for ParaglidingActivitySource {
    async fn suggest(&self, ctx: &PlanningContext) -> Result<Vec<ActivitySuggestion>> {
        let settings = self.site_repo.get_settings().await?.unwrap_or_default();
        let min_duration = Duration::hours(settings.minimum_flyable_hours as i64);

        let sites = self
            .site_repo
            .fetch_launches_within_radius(&ctx.home, settings.search_radius_km)
            .await;

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
                    out.push(ActivitySuggestion {
                        kind: ActivityKind::Paragliding,
                        location: launch.location.clone(),
                        timing: Timing::Flexible {
                            window: TimeWindow {
                                start: range.start,
                                end: range.end,
                            },
                            min_duration,
                        },
                        title: site.name.clone(),
                        description: String::new(),
                        score: None,
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
    use crate::{
        adapters::store::PersistentStore,
        domain::{
            location::Location,
            paragliding::{
                ParaglidingLaunch, ParaglidingSite, SiteType, UserSettings,
            },
            ports::MockWeatherProvider,
            weather::{WeatherData, WeatherForecast},
        },
    };
    use anyhow::anyhow;
    use chrono::{TimeZone, Utc};
    use mockall::predicate::*;
    use tempfile::TempDir;

    struct TestRepo {
        _dir: TempDir,
        repo: Arc<ParaglidingSiteRepository>,
    }

    fn fresh_repo() -> TestRepo {
        let dir = tempfile::tempdir().unwrap();
        let db = fjall::Database::builder(dir.path()).open().unwrap();
        let ks = db
            .keyspace("store", fjall::KeyspaceCreateOptions::default)
            .unwrap();
        let store = Arc::new(PersistentStore::from_keyspace(ks));
        let repo = Arc::new(ParaglidingSiteRepository::new(store));
        TestRepo { _dir: dir, repo }
    }

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
            visibility: 10.0,
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

    async fn seed_settings(repo: &ParaglidingSiteRepository) {
        repo.save_settings(&UserSettings {
            location_name: "Home".into(),
            location_latitude: 50.7,
            location_longitude: 13.0,
            search_radius_km: 100.0,
            calendar_name: "Paragliding".into(),
            minimum_flyable_hours: 1,
            excluded_calendar_names: vec![],
        })
        .await
        .unwrap();
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
        let r = fresh_repo();
        seed_settings(&r.repo).await;
        r.repo
            .save_site(site("S", None, vec![hang_launch()]))
            .await
            .unwrap();

        let mut weather = MockWeatherProvider::new();
        weather
            .expect_get_forecast()
            .returning(|_, _| Ok(bad_weather_forecast()));

        let source = ParaglidingActivitySource::new(r.repo.clone(), Arc::new(weather));
        let out = source.suggest(&ctx()).await.unwrap();
        assert!(out.is_empty(), "expected no suggestions, got {:?}", out);
    }

    #[tokio::test]
    async fn flyable_window_produces_one_suggestion() {
        let r = fresh_repo();
        seed_settings(&r.repo).await;
        r.repo
            .save_site(site("S", None, vec![hang_launch()]))
            .await
            .unwrap();

        let mut weather = MockWeatherProvider::new();
        weather
            .expect_get_forecast()
            .returning(|_, _| Ok(flyable_window_forecast()));

        let source = ParaglidingActivitySource::new(r.repo.clone(), Arc::new(weather));
        let out = source.suggest(&ctx()).await.unwrap();
        assert_eq!(out.len(), 1);
        let Timing::Flexible { window, .. } = &out[0].timing else {
            panic!("expected Flexible timing, got {:?}", out[0].timing);
        };
        let day = Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap();
        assert_eq!(window.start, day + chrono::Duration::hours(10));
        assert_eq!(window.end, day + chrono::Duration::hours(14));
        assert_eq!(out[0].title, "S");
    }

    #[tokio::test]
    async fn muted_site_is_skipped_without_calling_weather() {
        let r = fresh_repo();
        seed_settings(&r.repo).await;
        r.repo
            .save_site(site("Muted", Some(true), vec![hang_launch()]))
            .await
            .unwrap();

        let mut weather = MockWeatherProvider::new();
        weather.expect_get_forecast().times(0);

        let source = ParaglidingActivitySource::new(r.repo.clone(), Arc::new(weather));
        let out = source.suggest(&ctx()).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn site_without_launches_is_skipped() {
        let r = fresh_repo();
        seed_settings(&r.repo).await;
        r.repo
            .save_site(site("NoLaunches", None, vec![]))
            .await
            .unwrap();

        let mut weather = MockWeatherProvider::new();
        weather.expect_get_forecast().times(0);

        let source = ParaglidingActivitySource::new(r.repo.clone(), Arc::new(weather));
        let out = source.suggest(&ctx()).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn weather_error_skips_site_without_panicking() {
        let r = fresh_repo();
        seed_settings(&r.repo).await;
        r.repo
            .save_site(site("S", None, vec![hang_launch()]))
            .await
            .unwrap();

        let mut weather = MockWeatherProvider::new();
        weather
            .expect_get_forecast()
            .returning(|_, _| Err(anyhow!("upstream timeout")));

        let source = ParaglidingActivitySource::new(r.repo.clone(), Arc::new(weather));
        let out = source.suggest(&ctx()).await.unwrap();
        assert!(out.is_empty());
    }
}
