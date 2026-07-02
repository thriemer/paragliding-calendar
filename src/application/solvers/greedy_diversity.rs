use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};

use crate::domain::{
    activities::{ActivityKind, Plan, ScheduledActivity, TimeWindow, Timing},
    location::Location,
    ports::{RoutingProvider, SolverInput, WeekSolver},
};

/// Pairwise drive times over the deduped `{origin} ∪ {candidate locations}` set, fetched
/// once per solve via the routing matrix so placement is pure lookups.
struct DriveMatrix {
    index: HashMap<String, usize>,
    times: Vec<Vec<Duration>>,
}

impl DriveMatrix {
    fn get(&self, from: &Location, to: &Location) -> Duration {
        let (fk, tk) = (from.to_key(), to.to_key());
        if fk == tk {
            return Duration::zero();
        }
        match (self.index.get(&fk), self.index.get(&tk)) {
            (Some(&i), Some(&j)) => self.times[i][j],
            // Every location the solver queries is in the set by construction.
            _ => Duration::zero(),
        }
    }
}

pub struct GreedyDiversitySolver {
    routing: Arc<dyn RoutingProvider>,
    pub drive_weight: f32,
    pub buffer_between_items: Duration,
}

impl GreedyDiversitySolver {
    pub fn new(routing: Arc<dyn RoutingProvider>) -> Self {
        Self {
            routing,
            drive_weight: 0.01,
            buffer_between_items: Duration::minutes(15),
        }
    }

    /// Dedup origin + candidate locations by `to_key()` and fetch the full matrix once.
    async fn build_matrix(&self, input: &SolverInput) -> Result<DriveMatrix> {
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut locs: Vec<Location> = Vec::new();
        for loc in std::iter::once(&input.origin)
            .chain(input.candidates.iter().map(|c| &c.location))
        {
            index.entry(loc.to_key()).or_insert_with(|| {
                locs.push(loc.clone());
                locs.len() - 1
            });
        }

        let times = if locs.len() > 1 {
            self.routing.travel_time_matrix(&locs).await?
        } else {
            vec![vec![Duration::zero(); locs.len()]; locs.len()]
        };
        Ok(DriveMatrix { index, times })
    }
}

#[async_trait]
impl WeekSolver for GreedyDiversitySolver {
    async fn solve(&self, input: SolverInput) -> Result<Vec<Plan>> {
        // One matrix fetch up front warms every pair; placement below is pure lookups.
        let matrix = self.build_matrix(&input).await?;

        let mut plans = Vec::with_capacity(input.num_alternatives);
        let mut used_keys: Vec<(ActivityKind, String)> = Vec::new();

        for _ in 0..input.num_alternatives {
            let plan = build_plan(self, &input, &used_keys, &matrix);
            for a in &plan.items {
                used_keys.push((a.kind, a.location.to_key()));
            }
            plans.push(plan);
        }

        Ok(plans)
    }
}

fn build_plan(
    cfg: &GreedyDiversitySolver,
    input: &SolverInput,
    used_keys: &[(ActivityKind, String)],
    matrix: &DriveMatrix,
) -> Plan {
    let mut ranked: Vec<(f32, usize)> = Vec::with_capacity(input.candidates.len());
    for (i, c) in input.candidates.iter().enumerate() {
        let key = (c.kind, c.location.to_key());
        if used_keys.iter().any(|k| *k == key) {
            continue;
        }
        let fun = c.score.as_ref().map(|s| s.total()).unwrap_or(0.0);
        let drive_min = matrix.get(&input.origin, &c.location).num_minutes() as f32 * 2.0;
        let cost = -fun + cfg.drive_weight * drive_min;
        ranked.push((cost, i));
    }
    ranked.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut items: Vec<ScheduledActivity> = Vec::new();
    for (_, idx) in ranked {
        let c = &input.candidates[idx];
        let placed = match &c.timing {
            Timing::Fixed { start, end } => place_fixed(
                matrix,
                *start,
                *end,
                &c.location,
                &items,
                &input.origin,
                &input.free_slots,
                cfg.buffer_between_items,
            ),
            Timing::Flexible {
                window,
                min_duration,
            } => place_flexible(
                matrix,
                *window,
                *min_duration,
                &c.location,
                &items,
                &input.origin,
                &input.free_slots,
                cfg.buffer_between_items,
            ),
        };
        if let Some((start, end)) = placed {
            let fun = c
                .score
                .as_ref()
                .map(|s| s.fun_between(start, end))
                .unwrap_or(0.0);
            items.push(ScheduledActivity {
                kind: c.kind,
                location: c.location.clone(),
                start,
                end,
                title: c.title.clone(),
                description: c.description.clone(),
                fun,
            });
            items.sort_by_key(|a| a.start);
        }
    }

    let total_fun: f32 = items.iter().map(|a| a.fun).sum();
    let total_drive = compute_total_drive(matrix, &items, &input.origin);

    Plan {
        items,
        total_fun,
        total_drive,
    }
}

fn place_fixed(
    matrix: &DriveMatrix,
    cand_start: DateTime<Utc>,
    cand_end: DateTime<Utc>,
    cand_loc: &Location,
    placed: &[ScheduledActivity],
    home: &Location,
    free_slots: &[TimeWindow],
    buffer: Duration,
) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let mut prev: Option<&ScheduledActivity> = None;
    let mut next: Option<&ScheduledActivity> = None;
    for a in placed {
        if a.end <= cand_start {
            if prev.map(|x| x.end < a.end).unwrap_or(true) {
                prev = Some(a);
            }
        } else if a.start >= cand_end {
            if next.map(|x| x.start > a.start).unwrap_or(true) {
                next = Some(a);
            }
        } else {
            return None;
        }
    }

    let prev_loc = prev.map(|a| &a.location).unwrap_or(home);
    let next_loc = next.map(|a| &a.location).unwrap_or(home);

    let drive_in = matrix.get(prev_loc, cand_loc);
    let drive_out = matrix.get(cand_loc, next_loc);
    let phys_start = cand_start - drive_in;
    let phys_end = cand_end + drive_out;

    if let Some(a) = prev
        && phys_start < a.end + buffer
    {
        return None;
    }
    if let Some(a) = next
        && phys_end + buffer > a.start
    {
        return None;
    }

    let in_free = free_slots
        .iter()
        .any(|fs| fs.start <= phys_start && fs.end >= phys_end);
    if !in_free {
        return None;
    }

    Some((cand_start, cand_end))
}

fn place_flexible(
    matrix: &DriveMatrix,
    window: TimeWindow,
    min_duration: Duration,
    cand_loc: &Location,
    placed: &[ScheduledActivity],
    home: &Location,
    free_slots: &[TimeWindow],
    buffer: Duration,
) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    for fs in free_slots {
        let mut in_fs: Vec<&ScheduledActivity> = placed
            .iter()
            .filter(|a| a.end > fs.start && a.start < fs.end)
            .collect();
        in_fs.sort_by_key(|a| a.start);

        let mut prev_loc: &Location = home;
        let mut phys_lower = fs.start;

        for i in 0..=in_fs.len() {
            let (next_loc, phys_upper): (&Location, DateTime<Utc>) = if i < in_fs.len() {
                let a = in_fs[i];
                (&a.location, a.start - buffer)
            } else {
                (home, fs.end)
            };

            let drive_in = matrix.get(prev_loc, cand_loc);
            let drive_out = matrix.get(cand_loc, next_loc);
            let activity_start_min = (phys_lower + drive_in).max(window.start);
            let activity_end_max = (phys_upper - drive_out).min(window.end);

            if activity_end_max - activity_start_min >= min_duration {
                return Some((activity_start_min, activity_end_max));
            }

            if i < in_fs.len() {
                let a = in_fs[i];
                prev_loc = &a.location;
                phys_lower = a.end + buffer;
            }
        }
    }
    None
}

fn compute_total_drive(
    matrix: &DriveMatrix,
    items: &[ScheduledActivity],
    home: &Location,
) -> Duration {
    let mut total = Duration::zero();
    let mut prev: &Location = home;
    for a in items {
        total = total + matrix.get(prev, &a.location);
        prev = &a.location;
    }
    total = total + matrix.get(prev, home);
    total
}

// ponytail: skipped 2-opt repair pass; add when greedy ordering misses obvious
// improvements (two items where swapping reduces total drive).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        activities::{ActivityKind, ActivitySuggestion, Score},
        location::Location,
        ports::MockRoutingProvider,
    };
    use chrono::TimeZone;

    fn home() -> Location {
        Location::new(50.7, 13.0, "Home".into(), "DE".into())
    }
    fn site_a() -> Location {
        Location::new(50.75, 13.05, "A".into(), "DE".into())
    }
    fn site_b() -> Location {
        Location::new(50.8, 13.1, "B".into(), "DE".into())
    }
    fn ts(h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 13, h, 0, 0).unwrap()
    }

    fn flex(
        loc: Location,
        kind: ActivityKind,
        start_h: u32,
        end_h: u32,
        fun: f32,
    ) -> ActivitySuggestion {
        let window_hours = (end_h - start_h).max(1) as f32;
        ActivitySuggestion {
            kind,
            location: loc.clone(),
            timing: Timing::Flexible {
                window: TimeWindow {
                    start: ts(start_h),
                    end: ts(end_h),
                },
                min_duration: Duration::hours(2),
            },
            title: format!("flex-{}-{start_h}-{end_h}", loc.name),
            description: String::new(),
            score: Some(Score {
                window_start: ts(start_h),
                hourly: vec![fun / window_hours; window_hours as usize],
                reasons: vec![],
            }),
        }
    }

    /// Mock whose matrix is filled by a per-pair function of (from, to).
    fn matrix_routing(
        f: impl Fn(&Location, &Location) -> Duration + Send + Sync + 'static,
    ) -> Arc<dyn RoutingProvider> {
        let f = Arc::new(f);
        let mut r = MockRoutingProvider::new();
        r.expect_travel_time_matrix().returning(move |locs| {
            let f = f.clone();
            Ok(locs
                .iter()
                .map(|a| locs.iter().map(|b| f(a, b)).collect())
                .collect())
        });
        Arc::new(r)
    }

    fn constant_routing(minutes: i64) -> Arc<dyn RoutingProvider> {
        matrix_routing(move |_, _| Duration::minutes(minutes))
    }

    #[tokio::test]
    async fn picks_higher_fun_when_overlapping() {
        let solver = GreedyDiversitySolver::new(constant_routing(15));
        let input = SolverInput {
            candidates: vec![
                flex(site_a(), ActivityKind::Paragliding, 10, 14, 0.5),
                flex(site_b(), ActivityKind::Paragliding, 10, 14, 0.9),
            ],
            origin: home(),
            free_slots: vec![TimeWindow {
                start: ts(8),
                end: ts(18),
            }],
            num_alternatives: 1,
        };
        let plans = solver.solve(input).await.unwrap();
        assert_eq!(plans[0].items.len(), 1);
        assert_eq!(plans[0].items[0].location.name, "B");
    }

    #[tokio::test]
    async fn second_plan_avoids_first_plans_kind_location() {
        let solver = GreedyDiversitySolver::new(constant_routing(15));
        let input = SolverInput {
            candidates: vec![
                flex(site_a(), ActivityKind::Paragliding, 10, 14, 0.9),
                flex(site_b(), ActivityKind::Paragliding, 10, 14, 0.5),
            ],
            origin: home(),
            free_slots: vec![TimeWindow {
                start: ts(8),
                end: ts(16),
            }],
            num_alternatives: 2,
        };
        let plans = solver.solve(input).await.unwrap();
        assert_eq!(plans.len(), 2);
        let pick_a: Vec<_> = plans[0].items.iter().map(|a| &a.location.name).collect();
        let pick_b: Vec<_> = plans[1].items.iter().map(|a| &a.location.name).collect();
        assert_eq!(pick_a, vec!["A"]);
        assert_eq!(pick_b, vec!["B"]);
    }

    #[tokio::test]
    async fn alt_excludes_primary_picks_even_when_both_fit() {
        let solver = GreedyDiversitySolver::new(constant_routing(15));
        let input = SolverInput {
            candidates: vec![
                flex(site_a(), ActivityKind::Paragliding, 8, 11, 0.9),
                flex(site_b(), ActivityKind::Paragliding, 13, 16, 0.8),
            ],
            origin: home(),
            free_slots: vec![TimeWindow {
                start: ts(6),
                end: ts(20),
            }],
            num_alternatives: 2,
        };
        let plans = solver.solve(input).await.unwrap();
        assert_eq!(plans[0].items.len(), 2);
        assert!(plans[1].items.is_empty());
    }

    #[tokio::test]
    async fn distant_sites_cannot_overlap_their_drives() {
        let solver = GreedyDiversitySolver::new(constant_routing(5 * 60));
        let input = SolverInput {
            candidates: vec![
                flex(site_a(), ActivityKind::Paragliding, 10, 14, 0.9),
                flex(site_b(), ActivityKind::Paragliding, 10, 14, 0.5),
            ],
            origin: home(),
            free_slots: vec![TimeWindow {
                start: ts(0),
                end: ts(23),
            }],
            num_alternatives: 1,
        };
        let plans = solver.solve(input).await.unwrap();
        assert_eq!(plans[0].items.len(), 1);
    }

    #[tokio::test]
    async fn back_to_back_nearby_sites_skip_returning_home() {
        let home_key = home().to_key();
        let routing = matrix_routing(move |from, to| {
            let f = from.to_key();
            let t = to.to_key();
            if f == home_key || t == home_key {
                Duration::hours(2)
            } else {
                Duration::minutes(15)
            }
        });
        let solver = GreedyDiversitySolver::new(routing);
        let input = SolverInput {
            candidates: vec![
                flex(site_a(), ActivityKind::Paragliding, 8, 10, 0.9),
                flex(site_b(), ActivityKind::Paragliding, 11, 13, 0.8),
            ],
            origin: home(),
            free_slots: vec![TimeWindow {
                start: ts(6),
                end: ts(20),
            }],
            num_alternatives: 1,
        };
        let plans = solver.solve(input).await.unwrap();
        assert_eq!(plans[0].items.len(), 2);
        let a = plans[0].items.iter().find(|a| a.location.name == "A").unwrap();
        let b = plans[0].items.iter().find(|a| a.location.name == "B").unwrap();
        let gap = b.start - a.end;
        // With chaining (A→B is 15 min) the gap is ~1 h; going home would be ~4.5 h.
        assert!(gap >= Duration::minutes(15));
        assert!(gap < Duration::hours(2));
    }
}
