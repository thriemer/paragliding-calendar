use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use futures::future;

use tracing::{Span, instrument};

use crate::domain::{
    activities::{
        ActivityKind, ActivitySuggestion, Plan, PlanningContext, ScheduledActivity, TimeWindow,
        Timing,
    },
    calendar::CalendarEvent,
    ports::{ActivitySource, CalendarProvider, GeoProvider, SolverInput, WeekSolver},
};

pub struct Planner {
    sources: Vec<Arc<dyn ActivitySource>>,
    solver: Arc<dyn WeekSolver>,
    geo: Arc<dyn GeoProvider>,
    pub num_alternatives: usize,
}

impl Planner {
    pub fn new(
        sources: Vec<Arc<dyn ActivitySource>>,
        solver: Arc<dyn WeekSolver>,
        geo: Arc<dyn GeoProvider>,
    ) -> Self {
        Self {
            sources,
            solver,
            geo,
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
    /// Returns the alternative plans **plus** the fixed commitments they were planned around —
    /// the renderer (`calendar_job`) needs the commitments to route drives through them instead
    /// of straight across a meeting window.
    pub async fn plan(
        &self,
        ctx: &PlanningContext,
        calendar: &dyn CalendarProvider,
    ) -> Result<(Vec<Plan>, Vec<ScheduledActivity>)> {
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
                Timing::ExactDuration { window, duration } => {
                    if window.duration() >= *duration {
                        candidates.push(s);
                    }
                }
            }
        }

        // One event fetch feeds both the fixed commitments and the free slots, so a commitment and
        // its hole line up by construction. A calendar hiccup degrades to "no commitments / all
        // free" rather than aborting the plan (matching the old per-hour is_busy fallback).
        let events = calendar
            .get_events(&ctx.conflict_calendars, ctx.horizon.start, ctx.horizon.end)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "calendar get_events failed; planning with no commitments");
                Vec::new()
            });

        let fixed = self.commitments_from_events(&events).await;
        let free_slots = free_slots_from_events(ctx.horizon, &events);

        let input = SolverInput {
            candidates,
            origin: ctx.home.clone(),
            free_slots,
            fixed,
            num_alternatives: self.num_alternatives,
        };
        let candidates_out = input.candidates.len();
        // Keep a copy of the commitments to hand back for rendering; `solve` consumes `input`.
        let commitments = input.fixed.clone();
        let plans = self.solver.solve(input).await?;

        Span::current().record("candidates_in", candidates_in);
        Span::current().record("candidates_out", candidates_out);
        Span::current().record("plans", plans.len());

        Ok((plans, commitments))
    }

    /// Turn calendar events into fixed commitments (`fun = 0`). A located event geocodes to
    /// `Some(location)`; an event with no location, an empty geocode, or a geocode error becomes
    /// `None` (online) with a warning — never an error, since real calendar locations are junk half
    /// the time ("Teams-Meeting", "Konferenzraum 2. OG").
    async fn commitments_from_events(&self, events: &[CalendarEvent]) -> Vec<ScheduledActivity> {
        future::join_all(events.iter().map(|e| async move {
            let location = match &e.location {
                Some(text) if !text.trim().is_empty() => match self.geo.geocode(text).await {
                    Ok(hits) => {
                        let first = hits.into_iter().next();
                        if first.is_none() {
                            tracing::warn!(location = %text, "commitment location did not geocode; treating as online");
                        }
                        first
                    }
                    Err(err) => {
                        tracing::warn!(location = %text, error = %err, "geocode failed; treating commitment as online");
                        None
                    }
                },
                _ => None,
            };
            ScheduledActivity {
                kind: ActivityKind::Commitment,
                location,
                start: e.start_time,
                end: e.end_time,
                title: e.title.clone(),
                description: e.body.clone().unwrap_or_default(),
                fun: 0.0,
            }
        }))
        .await
    }
}

/// Free slots = the horizon with every event span subtracted. Overlapping events merge naturally
/// via the advancing cursor, giving exact-boundary windows (no hour quantization).
fn free_slots_from_events(horizon: TimeWindow, events: &[CalendarEvent]) -> Vec<TimeWindow> {
    let mut spans: Vec<(DateTime<Utc>, DateTime<Utc>)> = events
        .iter()
        .map(|e| (e.start_time.max(horizon.start), e.end_time.min(horizon.end)))
        .filter(|(s, e)| s < e)
        .collect();
    spans.sort_by_key(|(s, _)| *s);

    let mut slots = Vec::new();
    let mut cursor = horizon.start;
    for (s, e) in spans {
        if s > cursor {
            slots.push(TimeWindow { start: cursor, end: s });
        }
        cursor = cursor.max(e);
    }
    if cursor < horizon.end {
        slots.push(TimeWindow {
            start: cursor,
            end: horizon.end,
        });
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::solvers::Nsga2Solver,
        domain::{
            activities::Score,
            location::Location,
            ports::{
                MockActivitySource, MockCalendarProvider, MockGeoProvider, MockRoutingProvider,
                RoutingProvider,
            },
        },
    };
    use chrono::{TimeDelta, TimeZone};

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
            id: format!("fixed-{start_hour}-{end_hour}"),
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
            allow_multiple: false,
        }
    }

    fn flexible_suggestion(start_hour: u32, end_hour: u32) -> ActivitySuggestion {
        let window_hours = (end_hour - start_hour).max(1) as f32;
        ActivitySuggestion {
            id: format!("flex-{start_hour}-{end_hour}"),
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
            allow_multiple: false,
        }
    }

    /// No calendar events → the whole horizon is free.
    fn empty_calendar() -> MockCalendarProvider {
        let mut cal = MockCalendarProvider::new();
        cal.expect_get_events().returning(|_, _, _| Ok(vec![]));
        cal
    }

    /// One event spanning the whole queried range → no free slots at all.
    fn full_calendar() -> MockCalendarProvider {
        let mut cal = MockCalendarProvider::new();
        cal.expect_get_events().returning(|_, start, end| {
            Ok(vec![CalendarEvent {
                title: "all-day busy".into(),
                start_time: start,
                end_time: end,
                is_all_day: false,
                location: None,
                body: None,
                color_id: None,
            }])
        });
        cal
    }

    fn commitment_event(loc: Option<&str>) -> CalendarEvent {
        CalendarEvent {
            title: "meeting".into(),
            start_time: ts(10),
            end_time: ts(12),
            is_all_day: false,
            location: loc.map(str::to_string),
            body: None,
            color_id: None,
        }
    }

    /// A geo mock that is never expected to be called (used when there are no located events).
    fn no_geo() -> Arc<dyn GeoProvider> {
        Arc::new(MockGeoProvider::new())
    }

    fn fixed_travel() -> Arc<dyn RoutingProvider> {
        let mut r = MockRoutingProvider::new();
        r.expect_travel_time_matrix().returning(|locs| {
            Ok(vec![vec![TimeDelta::minutes(30); locs.len()]; locs.len()])
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
        let mut s = Nsga2Solver::new(routing);
        s.pop_size = 20;
        s.generations = 10;
        Arc::new(s)
    }

    fn activities_in(plan: &Plan) -> Vec<&str> {
        plan.items.iter().map(|a| a.title.as_str()).collect()
    }

    #[tokio::test]
    async fn fixed_dropped_when_calendar_busy() {
        let planner = Planner::new(
            vec![source_with(vec![fixed_suggestion(10, 12, None)])],
            solver(fixed_travel()),
            no_geo(),
        );

        let (plans, _) = planner.plan(&ctx(), &full_calendar()).await.unwrap();
        assert!(plans.is_empty(), "no free slots → no plans");
    }

    #[tokio::test]
    async fn fixed_kept_when_calendar_free() {
        let planner = Planner::new(
            vec![source_with(vec![fixed_suggestion(10, 12, None)])],
            solver(fixed_travel()),
            no_geo(),
        );

        let (plans, _) = planner.plan(&ctx(), &empty_calendar()).await.unwrap();
        assert_eq!(activities_in(&plans[0]).len(), 1);
    }

    #[tokio::test]
    async fn flexible_dropped_when_window_below_min_duration() {
        let planner = Planner::new(
            vec![source_with(vec![flexible_suggestion(10, 11)])],
            solver(fixed_travel()),
            no_geo(),
        );

        let (plans, _) = planner.plan(&ctx(), &empty_calendar()).await.unwrap();
        assert!(plans.is_empty(), "1h window < 2h min_duration → candidates dropped");
    }

    #[tokio::test]
    async fn flexible_kept_when_window_equals_min_after_travel() {
        let planner = Planner::new(
            vec![source_with(vec![flexible_suggestion(10, 13)])],
            solver(fixed_travel()),
            no_geo(),
        );

        let (plans, _) = planner.plan(&ctx(), &empty_calendar()).await.unwrap();
        assert_eq!(activities_in(&plans[0]).len(), 1);
    }

    #[tokio::test]
    async fn two_diverse_plans_returned() {
        let planner = Planner::new(
            vec![source_with(vec![
                fixed_suggestion(10, 12, Some(0.9)),
                fixed_suggestion(14, 16, Some(0.5)),
            ])],
            solver(fixed_travel()),
            no_geo(),
        );

        let (plans, _) = planner.plan(&ctx(), &empty_calendar()).await.unwrap();
        assert_eq!(plans.len(), 1, "only one distinct trade-off (both fixed events placed)");
    }

    fn geocoding_planner(geo: MockGeoProvider) -> Planner {
        Planner::new(vec![], solver(fixed_travel()), Arc::new(geo))
    }

    #[tokio::test]
    async fn located_event_geocodes_to_some_commitment() {
        let mut geo = MockGeoProvider::new();
        geo.expect_geocode().returning(|_| Ok(vec![site_loc()]));
        let planner = geocoding_planner(geo);

        let fixed = planner
            .commitments_from_events(&[commitment_event(Some("Dresden"))])
            .await;
        assert_eq!(fixed.len(), 1);
        assert_eq!(fixed[0].kind, ActivityKind::Commitment);
        assert_eq!(fixed[0].fun, 0.0);
        assert_eq!(fixed[0].location.as_ref().unwrap().name, "Site");
    }

    #[tokio::test]
    async fn location_less_event_is_online() {
        let planner = geocoding_planner(MockGeoProvider::new()); // geocode never called
        let fixed = planner
            .commitments_from_events(&[commitment_event(None)])
            .await;
        assert_eq!(fixed.len(), 1);
        assert!(fixed[0].location.is_none());
    }

    #[tokio::test]
    async fn geocode_failure_degrades_to_online() {
        let mut geo = MockGeoProvider::new();
        geo.expect_geocode()
            .returning(|_| Err(anyhow::anyhow!("boom")));
        let planner = geocoding_planner(geo);

        let fixed = planner
            .commitments_from_events(&[commitment_event(Some("Konferenzraum 2. OG"))])
            .await;
        assert!(fixed[0].location.is_none(), "geocode error must not propagate");
    }

    #[tokio::test]
    async fn geocode_empty_degrades_to_online() {
        let mut geo = MockGeoProvider::new();
        geo.expect_geocode().returning(|_| Ok(vec![]));
        let planner = geocoding_planner(geo);

        let fixed = planner
            .commitments_from_events(&[commitment_event(Some("Teams-Meeting"))])
            .await;
        assert!(fixed[0].location.is_none());
    }

    #[test]
    fn free_slots_split_around_a_midday_event() {
        let horizon = TimeWindow { start: ts(8), end: ts(18) };
        let event = CalendarEvent {
            title: "lunch".into(),
            start_time: ts(12),
            end_time: ts(13),
            is_all_day: false,
            location: None,
            body: None,
            color_id: None,
        };
        let slots = free_slots_from_events(horizon, &[event]);
        assert_eq!(slots.len(), 2);
        assert_eq!((slots[0].start, slots[0].end), (ts(8), ts(12)));
        assert_eq!((slots[1].start, slots[1].end), (ts(13), ts(18)));
    }

    #[test]
    fn free_slots_whole_horizon_when_no_events() {
        let horizon = TimeWindow { start: ts(8), end: ts(18) };
        let slots = free_slots_from_events(horizon, &[]);
        assert_eq!(slots.len(), 1);
        assert_eq!((slots[0].start, slots[0].end), (ts(8), ts(18)));
    }
}
