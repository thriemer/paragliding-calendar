use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Duration, TimeDelta, Utc};
use futures::future;

use crate::domain::{
    activities::{ActivitySuggestion, PlanningContext, TimeWindow, Timing},
    ports::{ActivitySource, CalendarProvider, RoutingProvider},
};

pub struct Planner {
    sources: Vec<Arc<dyn ActivitySource>>,
    routing: Arc<dyn RoutingProvider>,
}

impl Planner {
    pub fn new(
        sources: Vec<Arc<dyn ActivitySource>>,
        routing: Arc<dyn RoutingProvider>,
    ) -> Self {
        Self { sources, routing }
    }

    pub async fn plan<C: CalendarProvider + Send + Sync>(
        &self,
        ctx: &PlanningContext,
        calendar: &C,
    ) -> Result<Vec<ActivitySuggestion>> {
        let per_source = future::join_all(self.sources.iter().map(|s| s.suggest(ctx))).await;

        let mut raw: Vec<ActivitySuggestion> = Vec::new();
        for r in per_source {
            match r {
                Ok(mut v) => raw.append(&mut v),
                Err(e) => tracing::warn!(error = %e, "activity source failed"),
            }
        }

        let mut out = Vec::new();
        for s in raw {
            match &s.timing {
                Timing::Fixed { start, end } => {
                    let busy = calendar
                        .is_busy(&ctx.conflict_calendars, *start, *end)
                        .await
                        .unwrap_or(false);
                    if !busy {
                        out.push(s);
                    }
                }
                Timing::Flexible {
                    window,
                    min_duration,
                } => {
                    let sub_windows =
                        slice_by_calendar(*window, &ctx.conflict_calendars, calendar).await;
                    if sub_windows.is_empty() {
                        continue;
                    }

                    let travel = self
                        .routing
                        .get_travel_time(&ctx.home, &s.location)
                        .await?;

                    for w in sub_windows {
                        let adjusted = TimeWindow {
                            start: w.start + travel,
                            end: w.end - travel,
                        };
                        if adjusted.end > adjusted.start
                            && adjusted.duration() >= *min_duration
                        {
                            out.push(ActivitySuggestion {
                                timing: Timing::Flexible {
                                    window: adjusted,
                                    min_duration: *min_duration,
                                },
                                ..s.clone()
                            });
                        }
                    }
                }
            }
        }

        out.sort_by(|a, b| {
            let av = a.score.as_ref().map(|s| s.value);
            let bv = b.score.as_ref().map(|s| s.value);
            match (av, bv) {
                (Some(x), Some(y)) => y.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });

        Ok(out)
    }
}

async fn slice_by_calendar<C: CalendarProvider + Send + Sync>(
    window: TimeWindow,
    conflict_calendars: &Vec<String>,
    calendar: &C,
) -> Vec<TimeWindow> {
    let hour = TimeDelta::hours(1);
    let mut hours: Vec<DateTime<Utc>> = Vec::new();
    let mut t = window.start;
    while t <= window.end {
        hours.push(t);
        t += hour;
    }

    let busy_flags: Vec<bool> = future::join_all(hours.iter().map(|ts| async move {
        calendar
            .is_busy(
                conflict_calendars,
                *ts - Duration::minutes(30),
                *ts + Duration::minutes(30),
            )
            .await
            .unwrap_or(false)
    }))
    .await;

    let mut windows = Vec::new();
    let mut current: Option<Vec<DateTime<Utc>>> = None;
    for (ts, busy) in hours.into_iter().zip(busy_flags) {
        if busy {
            if let Some(run) = current.take()
                && let Some(w) = run_to_window(&run)
            {
                windows.push(w);
            }
        } else {
            current.get_or_insert_with(Vec::new).push(ts);
        }
    }
    if let Some(run) = current
        && let Some(w) = run_to_window(&run)
    {
        windows.push(w);
    }

    windows
}

fn run_to_window(run: &[DateTime<Utc>]) -> Option<TimeWindow> {
    let start = *run.first()?;
    let end = *run.last()?;
    Some(TimeWindow { start, end })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        activities::{ActivityKind, Score},
        location::Location,
        ports::{MockActivitySource, MockCalendarProvider, MockRoutingProvider},
    };
    use chrono::{TimeZone, Timelike};

    fn home() -> Location {
        Location::new(50.7, 13.0, "Home".into(), "DE".into())
    }

    fn site_loc() -> Location {
        Location::new(50.75, 13.05, "Site".into(), "DE".into())
    }

    fn ts(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 13, hour, 0, 0).unwrap()
    }

    fn ctx() -> PlanningContext {
        PlanningContext {
            home: home(),
            horizon: TimeWindow {
                start: ts(0),
                end: ts(0) + TimeDelta::days(1),
            },
            conflict_calendars: vec!["work".into()],
        }
    }

    fn fixed_suggestion(start_hour: u32, end_hour: u32, score: Option<f32>) -> ActivitySuggestion {
        ActivitySuggestion {
            kind: ActivityKind::Paragliding,
            location: site_loc(),
            timing: Timing::Fixed {
                start: ts(start_hour),
                end: ts(end_hour),
            },
            title: format!("fixed-{start_hour}-{end_hour}"),
            description: String::new(),
            score: score.map(|v| Score {
                value: v,
                reasons: vec![],
            }),
        }
    }

    fn flexible_suggestion(start_hour: u32, end_hour: u32) -> ActivitySuggestion {
        ActivitySuggestion {
            kind: ActivityKind::Paragliding,
            location: site_loc(),
            timing: Timing::Flexible {
                window: TimeWindow {
                    start: ts(start_hour),
                    end: ts(end_hour),
                },
                min_duration: TimeDelta::hours(2),
            },
            title: format!("flex-{start_hour}-{end_hour}"),
            description: String::new(),
            score: None,
        }
    }

    fn always_free_calendar() -> MockCalendarProvider {
        let mut cal = MockCalendarProvider::new();
        cal.expect_is_busy().returning(|_, _, _| Ok(false));
        cal
    }

    fn fixed_travel() -> Arc<dyn RoutingProvider> {
        let mut r = MockRoutingProvider::new();
        r.expect_get_travel_time()
            .returning(|_, _| Ok(Duration::minutes(30)));
        Arc::new(r)
    }

    fn source_with(suggestions: Vec<ActivitySuggestion>) -> Arc<dyn ActivitySource> {
        let mut src = MockActivitySource::new();
        src.expect_suggest()
            .returning(move |_| Ok(suggestions.clone()));
        Arc::new(src)
    }

    #[tokio::test]
    async fn fixed_timing_dropped_when_busy() {
        let planner = Planner::new(
            vec![source_with(vec![fixed_suggestion(10, 12, None)])],
            fixed_travel(),
        );
        let mut cal = MockCalendarProvider::new();
        cal.expect_is_busy().returning(|_, _, _| Ok(true));

        let out = planner.plan(&ctx(), &cal).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn fixed_timing_kept_when_free() {
        let planner = Planner::new(
            vec![source_with(vec![fixed_suggestion(10, 12, None)])],
            fixed_travel(),
        );
        let cal = always_free_calendar();

        let out = planner.plan(&ctx(), &cal).await.unwrap();
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0].timing, Timing::Fixed { .. }));
    }

    #[tokio::test]
    async fn flexible_dropped_when_fully_busy() {
        let planner = Planner::new(
            vec![source_with(vec![flexible_suggestion(10, 16)])],
            fixed_travel(),
        );
        let mut cal = MockCalendarProvider::new();
        cal.expect_is_busy().returning(|_, _, _| Ok(true));

        let out = planner.plan(&ctx(), &cal).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn flexible_kept_with_travel_time_eaten_from_both_ends() {
        let planner = Planner::new(
            vec![source_with(vec![flexible_suggestion(10, 16)])],
            fixed_travel(),
        );
        let cal = always_free_calendar();

        let out = planner.plan(&ctx(), &cal).await.unwrap();
        assert_eq!(out.len(), 1);
        let Timing::Flexible { window, .. } = &out[0].timing else {
            panic!("expected Flexible");
        };
        assert_eq!(window.start, ts(10) + Duration::minutes(30));
        assert_eq!(window.end, ts(16) - Duration::minutes(30));
    }

    #[tokio::test]
    async fn flexible_dropped_when_remaining_window_below_min_duration() {
        let planner = Planner::new(
            vec![source_with(vec![flexible_suggestion(10, 12)])],
            fixed_travel(),
        );
        let cal = always_free_calendar();

        let out = planner.plan(&ctx(), &cal).await.unwrap();
        assert!(out.is_empty(), "2h window minus 60m travel < 2h min_duration");
    }

    #[tokio::test]
    async fn flexible_kept_when_remaining_window_equals_min_duration_exactly() {
        let planner = Planner::new(
            vec![source_with(vec![flexible_suggestion(10, 13)])],
            fixed_travel(),
        );
        let cal = always_free_calendar();

        let out = planner.plan(&ctx(), &cal).await.unwrap();
        assert_eq!(
            out.len(),
            1,
            "3h window - 60m travel = 2h, which meets min_duration (inclusive)",
        );
    }

    #[tokio::test]
    async fn flexible_dropped_when_travel_eats_entire_window() {
        let mut routing = MockRoutingProvider::new();
        routing
            .expect_get_travel_time()
            .returning(|_, _| Ok(Duration::hours(1)));
        let planner = Planner::new(
            vec![source_with(vec![flexible_suggestion(10, 12)])],
            Arc::new(routing),
        );
        let cal = always_free_calendar();

        let out = planner.plan(&ctx(), &cal).await.unwrap();
        assert!(
            out.is_empty(),
            "2h window - 60m travel each side = adjusted.end == adjusted.start; nothing left to fly",
        );
    }

    #[tokio::test]
    async fn slice_by_calendar_busy_check_window_is_centered_on_each_hour() {
        let mut cal = MockCalendarProvider::new();
        cal.expect_is_busy().returning(|_, start, end| {
            assert_eq!(
                end - start,
                Duration::hours(1),
                "is_busy window must be a full hour (start + 30m to end - 30m around each ts)",
            );
            assert_eq!(
                (start + Duration::minutes(30)).minute(),
                0,
                "the window must center on hour boundaries (start = t - 30m)",
            );
            Ok(false)
        });

        let window = TimeWindow {
            start: ts(10),
            end: ts(12),
        };
        let _ = slice_by_calendar(window, &vec![], &cal).await;
    }

    #[tokio::test]
    async fn sort_orders_by_score_descending_with_none_last() {
        let planner = Planner::new(
            vec![source_with(vec![
                fixed_suggestion(10, 12, Some(0.5)),
                fixed_suggestion(13, 15, None),
                fixed_suggestion(16, 18, Some(0.9)),
            ])],
            fixed_travel(),
        );
        let cal = always_free_calendar();

        let out = planner.plan(&ctx(), &cal).await.unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].score.as_ref().map(|s| s.value), Some(0.9));
        assert_eq!(out[1].score.as_ref().map(|s| s.value), Some(0.5));
        assert!(out[2].score.is_none());
    }

    #[tokio::test]
    async fn slice_by_calendar_returns_one_window_when_all_free() {
        let cal = always_free_calendar();
        let window = TimeWindow {
            start: ts(10),
            end: ts(15),
        };

        let out = slice_by_calendar(window, &vec![], &cal).await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].start, ts(10));
        assert_eq!(out[0].end, ts(15));
    }

    #[tokio::test]
    async fn slice_by_calendar_breaks_window_at_busy_hour() {
        let mut cal = MockCalendarProvider::new();
        cal.expect_is_busy().returning(|_, start, _| {
            Ok((start + Duration::minutes(30)).hour() == 12)
        });

        let window = TimeWindow {
            start: ts(10),
            end: ts(14),
        };
        let out = slice_by_calendar(window, &vec![], &cal).await;
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].start, ts(10));
        assert_eq!(out[0].end, ts(11));
        assert_eq!(out[1].start, ts(13));
        assert_eq!(out[1].end, ts(14));
    }

    #[tokio::test]
    async fn slice_by_calendar_returns_empty_when_all_busy() {
        let mut cal = MockCalendarProvider::new();
        cal.expect_is_busy().returning(|_, _, _| Ok(true));

        let window = TimeWindow {
            start: ts(10),
            end: ts(15),
        };
        let out = slice_by_calendar(window, &vec![], &cal).await;
        assert!(out.is_empty());
    }
}
