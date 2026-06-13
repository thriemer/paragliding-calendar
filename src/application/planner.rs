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
                            && adjusted.duration() > *min_duration
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
