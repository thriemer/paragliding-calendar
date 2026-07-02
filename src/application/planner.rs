use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Duration, TimeDelta, Utc};
use futures::future;

use tracing::{Span, instrument};

use crate::domain::{
    activities::{ActivitySuggestion, Plan, PlanningContext, TimeWindow, Timing},
    ports::{ActivitySource, CalendarProvider, SolverInput, WeekSolver},
};

pub struct Planner {
    sources: Vec<Arc<dyn ActivitySource>>,
    solver: Arc<dyn WeekSolver>,
    pub num_alternatives: usize,
}

impl Planner {
    pub fn new(
        sources: Vec<Arc<dyn ActivitySource>>,
        solver: Arc<dyn WeekSolver>,
    ) -> Self {
        Self {
            sources,
            solver,
            num_alternatives: 2,
        }
    }

    #[instrument(
        skip_all,
        fields(
            horizon_days = (ctx.horizon.end - ctx.horizon.start).num_days(),
            candidates_in = tracing::field::Empty,
            candidates_out = tracing::field::Empty,
            plans = tracing::field::Empty,
        )
    )]
    pub async fn plan(
        &self,
        ctx: &PlanningContext,
        calendar: &dyn CalendarProvider,
    ) -> Result<Vec<Plan>> {
        let per_source = future::join_all(self.sources.iter().map(|s| s.suggest(ctx))).await;

        let mut raw: Vec<ActivitySuggestion> = Vec::new();
        for r in per_source {
            match r {
                Ok(mut v) => raw.append(&mut v),
                Err(e) => tracing::warn!(error = %e, "activity source failed"),
            }
        }
        let candidates_in = raw.len();

        let mut candidates: Vec<ActivitySuggestion> = Vec::new();
        for s in raw {
            match &s.timing {
                Timing::Fixed { .. } => candidates.push(s),
                Timing::Flexible { window, min_duration } => {
                    if window.duration() >= *min_duration {
                        candidates.push(s);
                    }
                }
            }
        }

        let free_slots = slice_by_calendar(ctx.horizon, &ctx.conflict_calendars, calendar).await;

        let input = SolverInput {
            candidates,
            origin: ctx.home.clone(),
            free_slots,
            num_alternatives: self.num_alternatives,
        };
        let candidates_out = input.candidates.len();
        let plans = self.solver.solve(input).await?;

        Span::current().record("candidates_in", candidates_in);
        Span::current().record("candidates_out", candidates_out);
        Span::current().record("plans", plans.len());

        Ok(plans)
    }
}

async fn slice_by_calendar(
    window: TimeWindow,
    conflict_calendars: &Vec<String>,
    calendar: &dyn CalendarProvider,
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
    use crate::{
        application::solvers::GreedyDiversitySolver,
        domain::{
            activities::{ActivityKind, Score},
            location::Location,
            ports::{
                MockActivitySource, MockCalendarProvider, MockRoutingProvider, RoutingProvider,
            },
        },
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
        let duration_hours = (end_hour - start_hour).max(1) as f32;
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
                window_start: ts(start_hour),
                hourly: vec![v / duration_hours; duration_hours as usize],
                reasons: vec![],
            }),
        }
    }

    fn flexible_suggestion(start_hour: u32, end_hour: u32) -> ActivitySuggestion {
        let window_hours = (end_hour - start_hour).max(1) as f32;
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
            score: Some(Score {
                window_start: ts(start_hour),
                hourly: vec![0.5 / window_hours; window_hours as usize],
                reasons: vec![],
            }),
        }
    }

    fn always_free_calendar() -> MockCalendarProvider {
        let mut cal = MockCalendarProvider::new();
        cal.expect_is_busy().returning(|_, _, _| Ok(false));
        cal
    }

    fn fixed_travel() -> Arc<dyn RoutingProvider> {
        let mut r = MockRoutingProvider::new();
        r.expect_travel_time_matrix().returning(|locs| {
            Ok(vec![vec![Duration::minutes(30); locs.len()]; locs.len()])
        });
        Arc::new(r)
    }

    fn source_with(suggestions: Vec<ActivitySuggestion>) -> Arc<dyn ActivitySource> {
        let mut src = MockActivitySource::new();
        src.expect_suggest()
            .returning(move |_| Ok(suggestions.clone()));
        Arc::new(src)
    }

    fn solver(routing: Arc<dyn RoutingProvider>) -> Arc<dyn WeekSolver> {
        Arc::new(GreedyDiversitySolver::new(routing))
    }

    fn activities_in(plan: &Plan) -> Vec<&str> {
        plan.items.iter().map(|a| a.title.as_str()).collect()
    }

    #[tokio::test]
    async fn fixed_dropped_when_calendar_busy() {
        let routing = fixed_travel();
        let planner = Planner::new(
            vec![source_with(vec![fixed_suggestion(10, 12, None)])],
            solver(routing),
        );
        let mut cal = MockCalendarProvider::new();
        cal.expect_is_busy().returning(|_, _, _| Ok(true));

        let plans = planner.plan(&ctx(), &cal).await.unwrap();
        assert_eq!(plans.len(), 2);
        assert!(activities_in(&plans[0]).is_empty());
    }

    #[tokio::test]
    async fn fixed_kept_when_calendar_free() {
        let routing = fixed_travel();
        let planner = Planner::new(
            vec![source_with(vec![fixed_suggestion(10, 12, None)])],
            solver(routing),
        );
        let cal = always_free_calendar();

        let plans = planner.plan(&ctx(), &cal).await.unwrap();
        assert_eq!(activities_in(&plans[0]).len(), 1);
    }

    #[tokio::test]
    async fn flexible_dropped_when_window_below_min_duration() {
        let routing = fixed_travel();
        let planner = Planner::new(
            vec![source_with(vec![flexible_suggestion(10, 11)])],
            solver(routing),
        );
        let cal = always_free_calendar();

        let plans = planner.plan(&ctx(), &cal).await.unwrap();
        assert!(activities_in(&plans[0]).is_empty(), "1h window < 2h min_duration");
    }

    #[tokio::test]
    async fn flexible_kept_when_window_equals_min_after_travel() {
        let routing = fixed_travel();
        let planner = Planner::new(
            vec![source_with(vec![flexible_suggestion(10, 13)])],
            solver(routing),
        );
        let cal = always_free_calendar();

        let plans = planner.plan(&ctx(), &cal).await.unwrap();
        assert_eq!(activities_in(&plans[0]).len(), 1);
    }

    #[tokio::test]
    async fn two_diverse_plans_returned() {
        let routing = fixed_travel();
        let planner = Planner::new(
            vec![source_with(vec![
                fixed_suggestion(10, 12, Some(0.9)),
                fixed_suggestion(14, 16, Some(0.5)),
            ])],
            solver(routing),
        );
        let cal = always_free_calendar();

        let plans = planner.plan(&ctx(), &cal).await.unwrap();
        assert_eq!(plans.len(), 2);
    }

    #[tokio::test]
    async fn slice_by_calendar_busy_check_window_is_centered_on_each_hour() {
        let mut cal = MockCalendarProvider::new();
        cal.expect_is_busy().returning(|_, start, end| {
            assert_eq!(end - start, Duration::hours(1));
            assert_eq!((start + Duration::minutes(30)).minute(), 0);
            Ok(false)
        });

        let window = TimeWindow { start: ts(10), end: ts(12) };
        let _ = slice_by_calendar(window, &vec![], &cal).await;
    }

    #[tokio::test]
    async fn slice_by_calendar_returns_one_window_when_all_free() {
        let cal = always_free_calendar();
        let window = TimeWindow { start: ts(10), end: ts(15) };
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

        let window = TimeWindow { start: ts(10), end: ts(14) };
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
        let window = TimeWindow { start: ts(10), end: ts(15) };
        let out = slice_by_calendar(window, &vec![], &cal).await;
        assert!(out.is_empty());
    }
}
