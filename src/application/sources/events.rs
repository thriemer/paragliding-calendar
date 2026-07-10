use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use super::with_source_url;
use crate::domain::{
    activities::{ActivityKind, ActivitySuggestion, Score, Timing},
    plan::PlanningContext,
    ports::{ActivitySource, HappeningRepository, SettingsRepository},
    scoring::events::{BASE_FUN_PER_HOUR, MAX_ATTEND},
};

/// Surfaces generic dated events (festivals, theatre, kids' activities, …) as fixed-time planner
/// candidates. Events are not outdoor/weather-bound, so scoring is a flat per-hour fun.
pub struct EventActivitySource {
    event_repo: Arc<dyn HappeningRepository>,
    settings_repo: Arc<dyn SettingsRepository>,
}

impl EventActivitySource {
    pub fn new(
        event_repo: Arc<dyn HappeningRepository>,
        settings_repo: Arc<dyn SettingsRepository>,
    ) -> Self {
        Self { event_repo, settings_repo }
    }
}

#[async_trait]
impl ActivitySource for EventActivitySource {
    async fn suggest(&self, ctx: &PlanningContext) -> Result<Vec<ActivitySuggestion>> {
        let settings = self.settings_repo.get().await?.unwrap_or_default();

        let events = self
            .event_repo
            .find_within_radius_and_time(
                &ctx.home,
                settings.search_radius_km,
                ctx.horizon.start,
                ctx.horizon.end,
            )
            .await?;

        let mut out = Vec::new();
        for (event, _distance) in events {
            let Some(location) = event.location.clone() else {
                tracing::debug!(event = %event.title, "Skipping event without a location");
                continue;
            };

            for date in &event.dates {
                // The repo filters by event, not per-date; keep only dates that overlap the horizon.
                if date.time_to <= ctx.horizon.start || date.time_from >= ctx.horizon.end {
                    continue;
                }

                let start = date.time_from;
                let end = date.time_to.min(start + MAX_ATTEND);
                let hours = ((end - start).num_minutes() as f32 / 60.0).ceil().max(1.0) as usize;

                let reason = date
                    .date_text
                    .clone()
                    .or_else(|| event.description_short.clone())
                    .unwrap_or_default();
                let score = Score {
                    window_start: start,
                    hourly: vec![BASE_FUN_PER_HOUR; hours],
                    reasons: vec![reason.clone()],
                };

                out.push(ActivitySuggestion {
                    id: event.id.clone(),
                    kind: ActivityKind::Event,
                    location: location.clone(),
                    timing: Timing::Fixed { start, end },
                    title: event.title.clone(),
                    description: with_source_url(&reason, &event.source_url),
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
        location::Location,
        happening::{HappeningDate, Happening},
        ports::{MockHappeningRepository, MockSettingsRepository},
        settings::UserSettings,
    };
    use chrono::{Duration, TimeZone, Utc};

    fn home() -> Location {
        Location::new(50.7, 13.0, "Home".into(), "DE".into())
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

    fn ctx() -> PlanningContext {
        PlanningContext {
            home: home(),
            horizon: crate::domain::activities::TimeWindow {
                start: Utc.with_ymd_and_hms(2026, 6, 13, 0, 0, 0).unwrap(),
                end: Utc.with_ymd_and_hms(2026, 6, 20, 0, 0, 0).unwrap(),
            },
            conflict_calendars: vec![],
        }
    }

    fn event(location: Option<Location>) -> Happening {
        let day = Utc.with_ymd_and_hms(2026, 6, 14, 0, 0, 0).unwrap();
        Happening {
            id: "e1".into(),
            title: "Festival".into(),
            location,
            category_id: None,
            category_title: Some("Musik".into()),
            category_keys: vec![],
            description_short: Some("A festival".into()),
            description_long: None,
            homepage: None,
            address: None,
            organizer: None,
            schedule_rules: None,
            dates: vec![HappeningDate {
                time_from: day + Duration::hours(19),
                time_to: day + Duration::hours(23),
                date_text: Some("Sat evening".into()),
            }],
            source_url: String::new(),
            data: serde_json::Value::Null,
        }
    }

    async fn run(events: Vec<Happening>) -> Vec<ActivitySuggestion> {
        let mut repo = MockHappeningRepository::new();
        repo.expect_find_within_radius_and_time()
            .returning(move |_, _, _, _| Ok(events.iter().cloned().map(|e| (e, 5.0)).collect()));
        let source = EventActivitySource::new(Arc::new(repo), Arc::new(mock_settings()));
        source.suggest(&ctx()).await.unwrap()
    }

    #[tokio::test]
    async fn located_event_becomes_one_fixed_suggestion() {
        let out = run(vec![event(Some(Location::new(50.8, 13.1, "Venue".into(), "DE".into())))]).await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, ActivityKind::Event);
        let day = Utc.with_ymd_and_hms(2026, 6, 14, 0, 0, 0).unwrap();
        let Timing::Fixed { start, end } = out[0].timing else {
            panic!("expected Fixed timing, got {:?}", out[0].timing);
        };
        assert_eq!(start, day + Duration::hours(19));
        assert_eq!(end, day + Duration::hours(23)); // 4h == MAX_ATTEND, not clamped shorter
    }

    #[tokio::test]
    async fn location_less_event_is_skipped() {
        let out = run(vec![event(None)]).await;
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn multi_day_event_is_clamped_to_max_attend() {
        let mut e = event(Some(Location::new(50.8, 13.1, "Venue".into(), "DE".into())));
        let day = Utc.with_ymd_and_hms(2026, 6, 14, 0, 0, 0).unwrap();
        e.dates[0].time_from = day + Duration::hours(10);
        e.dates[0].time_to = day + Duration::hours(72); // 3-day festival
        let out = run(vec![e]).await;
        assert_eq!(out.len(), 1);
        let Timing::Fixed { start, end } = out[0].timing else { panic!() };
        assert_eq!(end - start, MAX_ATTEND);
    }
}
