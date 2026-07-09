//! Genome, decoder and overflow-repair for the genetic solver (§3.3–§5 of
//! docs/genetic-planner-design.md). Phase 4: the data the GA (Phase 5) operates on plus the
//! deterministic `decode` and Lamarckian `repair` — no GA loop yet.

use std::collections::HashSet;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use rand::{rngs::StdRng, RngExt};

use crate::application::solvers::placement::{
    compute_total_drive, partition_segments, DriveMatrix, Segment,
};
use crate::domain::{
    activities::{ActivitySuggestion, OvernightKind, OvernightSpot, Plan, ScheduledActivity, Timing},
    location::Location,
    ports::SolverInput,
};

/// A genome: one gene-list per placement `Segment`, plus one overnight-spot choice per night
/// boundary (segments with `night_end`) — both in partition order (§3.3).
#[derive(Clone)]
pub struct Genome {
    pub segments: Vec<Vec<Gene>>,
    /// Chosen overnight spot per night, held by `Arc` like `GeneAction::Do(Arc<ActivitySuggestion>)`
    /// so richer future camp spots are shared cheaply rather than keyed by id.
    pub overnight: Vec<Arc<OvernightSpot>>,
}

/// Per-night `total_fun` penalty for sleeping away from home. Zero today (every overnight is home);
/// raise it once `overnight_candidates` returns campable spots so the GA prefers sleeping at home.
const NIGHT_AWAY_PENALTY: f32 = 0.0;

/// Candidate overnight spots for a night. Today only home; the future camping feature appends
/// reachable campable spots here — the single plug point, everything else already handles a pool.
pub fn overnight_candidates(home: &Location) -> Vec<Arc<OvernightSpot>> {
    vec![Arc::new(OvernightSpot::home(home.clone()))]
}

/// Number of night boundaries the given segments imply (= overnight choices a genome needs).
pub fn night_count(segments: &[Segment]) -> usize {
    segments.iter().filter(|s| s.night_end).count()
}

#[derive(Debug, Clone)]
pub struct Gene {
    pub action: GeneAction,
    /// Normalized `[0,1]`; the decoder maps it onto a real span so mutation can't produce an
    /// illegal value.
    pub duration: f32,
}

#[derive(Debug, Clone)]
pub enum GeneAction {
    /// Carries the activity by value; the same `Arc` may appear more than once (no dedup).
    Do(Arc<ActivitySuggestion>),
    /// Consumes time only, delaying whatever follows → implicit start-time control.
    Wait,
}

/// `duration <= EPS` snaps to the normalized minimum (0) during repair.
const EPS: f32 = 0.01;

/// Map a normalized `[0,1]` fraction onto `[lo, hi]` as a `Duration` (second granularity).
/// `norm` is clamped, so the result is always within `[lo, hi]`.
fn map_duration(norm: f32, lo: Duration, hi: Duration) -> Duration {
    let lo_s = lo.num_seconds() as f32;
    let hi_s = hi.num_seconds() as f32;
    let secs = lo_s + norm.clamp(0.0, 1.0) * (hi_s - lo_s);
    Duration::seconds(secs.round() as i64)
}

fn scheduled(act: &ActivitySuggestion, start: DateTime<Utc>, end: DateTime<Utc>) -> ScheduledActivity {
    let fun = act.score.as_ref().map(|s| s.fun_between(start, end)).unwrap_or(0.0);
    ScheduledActivity {
        kind: act.kind,
        location: Some(act.location.clone()),
        start,
        end,
        title: act.title.clone(),
        description: act.description.clone(),
        fun,
    }
}

struct WalkOutput {
    placed: Vec<ScheduledActivity>,
    /// Cursor location at the end of the walk (carried into the next segment).
    end_loc: Location,
    /// Index of the first gene that couldn't be placed; `None` once the walk is clean.
    unplaceable: Option<usize>,
}

/// Walk one segment's gene-list in order from `(seg.start, start_loc)` (§4 step 1). Stops at the
/// first `Do` whose window can't be satisfied from the current cursor, reporting its index.
fn walk_segment(
    genes: &[Gene],
    seg: &Segment,
    start_loc: &Location,
    end_loc: Option<&Location>,
    matrix: &DriveMatrix,
) -> WalkOutput {
    let span = seg.end - seg.start;
    let mut time = seg.start;
    let mut loc = start_loc.clone();
    let mut placed = Vec::new();

    for (i, gene) in genes.iter().enumerate() {
        match &gene.action {
            GeneAction::Wait => {
                time += map_duration(gene.duration, Duration::zero(), span);
            }
            GeneAction::Do(act) => {
                // Drive-out to the resolved end boundary (a pinned commitment, or the chosen
                // overnight spot at a night boundary) is reserved so the final activity can still
                // reach it in time; a carried (`None`) boundary needs no reservation.
                let drive_out = end_loc
                    .map(|e| matrix.get(&act.location, e))
                    .unwrap_or_else(Duration::zero);
                let drive_in = matrix.get(&loc, &act.location);

                let (start, end) = match &act.timing {
                    Timing::Flexible { window, min_duration } => {
                        let start = (time + drive_in).max(window.start);
                        let latest_feasible = window.end.min(seg.end - drive_out);
                        if latest_feasible - start < *min_duration {
                            return WalkOutput { placed, end_loc: loc, unplaceable: Some(i) };
                        }
                        let dur = map_duration(gene.duration, *min_duration, latest_feasible - start);
                        (start, start + dur)
                    }
                    // Pinned span; `duration`/`Wait` don't apply. Unplaceable if we can't arrive
                    // in time or the pinned end overruns the segment's drive-out reservation.
                    Timing::ExactDuration { window, duration } => {
                        let start = (time + drive_in).max(window.start);
                        let end = start + *duration;
                        if end > window.end.min(seg.end - drive_out) {
                            return WalkOutput { placed, end_loc: loc, unplaceable: Some(i) };
                        }
                        (start, end)
                    }
                    Timing::Fixed { start, end } => {
                        if time + drive_in > *start || *end + drive_out > seg.end {
                            return WalkOutput { placed, end_loc: loc, unplaceable: Some(i) };
                        }
                        (*start, *end)
                    }
                };

                placed.push(scheduled(act, start, end));
                time = end;
                loc = act.location.clone();
            }
        }
    }

    WalkOutput { placed, end_loc: loc, unplaceable: None }
}

/// Resolve a segment's start location: its pinned `start_loc`, or the location carried from the
/// previous segment (`None` = online commitment / availability gap / horizon start → home).
fn resolve_start(seg: &Segment, carried: &Location) -> Location {
    seg.start_loc.clone().unwrap_or_else(|| carried.clone())
}

/// Decode a (feasible, i.e. repaired) genome into a `Plan` (§4). Deterministic and pure.
/// Segments are decoded independently then chained through the fixed commitments for the drive
/// total. `Plan.items` is only the optional activities the GA placed — commitments are already
/// on the calendar.
pub fn decode(genome: &Genome, input: &SolverInput, matrix: &DriveMatrix) -> Plan {
    let segments = partition_segments(&input.free_slots, &input.fixed, &input.origin);
    let mut placed_all: Vec<ScheduledActivity> = Vec::new();
    let mut carried = input.origin.clone();
    let mut night_idx = 0;

    for (i, seg) in segments.iter().enumerate() {
        let start_loc = resolve_start(seg, &carried);
        let genes = genome.segments.get(i).map(Vec::as_slice).unwrap_or(&[]);
        // At a night boundary the day drives out to the chosen overnight spot; otherwise to the
        // pinned commitment location (or nothing, when carried).
        let overnight_loc: Option<Location> = if seg.night_end {
            genome.overnight.get(night_idx).map(|s| s.location.clone())
        } else {
            None
        };
        let end_loc = if seg.night_end { overnight_loc.clone() } else { seg.end_loc.clone() };
        let out = walk_segment(genes, seg, &start_loc, end_loc.as_ref(), matrix);
        carried = if seg.night_end {
            night_idx += 1;
            overnight_loc.unwrap_or(carried) // sleep at the overnight spot → depart there tomorrow
        } else {
            out.end_loc
        };
        placed_all.extend(out.placed);
    }

    let mut total_fun: f32 = placed_all.iter().map(|a| a.fun).sum();
    // Home-preference seam: penalize nights away from home. Zero today (all overnights are home).
    let nights_away = genome
        .overnight
        .iter()
        .filter(|s| s.kind != OvernightKind::Home)
        .count();
    total_fun -= NIGHT_AWAY_PENALTY * nights_away as f32;

    // Chain: placed stops interleaved with located commitments; `compute_total_drive` sums a home
    // round trip per day (online `None` commitments are skipped).
    let mut chain = placed_all.clone();
    chain.extend(input.fixed.iter().cloned());
    chain.sort_by_key(|a| a.start);
    let total_drive = compute_total_drive(matrix, &chain, &input.origin);

    Plan { items: placed_all, total_fun, total_drive }
}

/// Make an over-packed segment feasible by shrinking (then, at the floor, removing) genes until
/// the §4 walk places every gene (§5). Mutates `genes` in place (Lamarckian) so the fix persists
/// in the genome and `decode` afterwards walks clean.
fn repair_segment(
    genes: &mut Vec<Gene>,
    seg: &Segment,
    start_loc: &Location,
    end_loc: Option<&Location>,
    matrix: &DriveMatrix,
    rng: &mut StdRng,
) {
    loop {
        let idx = match walk_segment(genes, seg, start_loc, end_loc, matrix).unplaceable {
            None => return,
            Some(i) => i,
        };
        // Normalized min is 0 for every gene, so shrinkable room == the normalized duration.
        let candidates: Vec<usize> = genes
            .iter()
            .enumerate()
            .filter(|(_, g)| g.duration > 0.0)
            .map(|(i, _)| i)
            .collect();
        if candidates.is_empty() {
            genes.remove(idx);
            continue;
        }
        let pick = candidates[rng.random_range(0..candidates.len())];
        if genes[pick].duration <= EPS {
            genes[pick].duration = 0.0;
        } else {
            let frac = rng.random_range(0.25..0.75);
            genes[pick].duration -= frac * genes[pick].duration;
        }
    }
}

/// Repair every segment of a genome in partition order, carrying location across segments the
/// same way `decode` does (§5). Deterministic given a seeded `rng`.
pub fn repair(genome: &mut Genome, input: &SolverInput, matrix: &DriveMatrix, rng: &mut StdRng) {
    let segments = partition_segments(&input.free_slots, &input.fixed, &input.origin);
    let mut carried = input.origin.clone();
    let mut night_idx = 0;

    for (i, seg) in segments.iter().enumerate() {
        let start_loc = resolve_start(seg, &carried);
        let overnight_loc: Option<Location> = if seg.night_end {
            genome.overnight.get(night_idx).map(|s| s.location.clone())
        } else {
            None
        };
        let end_loc = if seg.night_end { overnight_loc.clone() } else { seg.end_loc.clone() };
        if let Some(genes) = genome.segments.get_mut(i) {
            repair_segment(genes, seg, &start_loc, end_loc.as_ref(), matrix, rng);
            // Recompute the carry from the repaired (clean) walk; a night boundary carries the
            // overnight spot regardless of where the last activity sat.
            let walked = walk_segment(genes, seg, &start_loc, end_loc.as_ref(), matrix).end_loc;
            carried = if seg.night_end { overnight_loc.unwrap_or(walked) } else { walked };
        } else if seg.night_end {
            carried = overnight_loc.unwrap_or(carried);
        }
        if seg.night_end {
            night_idx += 1;
        }
    }
}

/// Remove extra occurrences of non-repeatable activities across all segments.
/// Activities with `allow_multiple = true` are untouched. Identity is the activity `id`, not the
/// `Arc` pointer: one activity fans out into several per-day suggestions (distinct Arcs, same `id`),
/// so pointer identity would let the same tour/event be scheduled on two different days.
pub fn dedup_single_use(genome: &mut Genome) {
    let mut seen: HashSet<String> = HashSet::new();
    for seg in &mut genome.segments {
        seg.retain(|g| {
            if let GeneAction::Do(act) = &g.action {
                if !act.allow_multiple {
                    return seen.insert(act.id.clone());
                }
            }
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::solvers::placement::build_matrix;
    use crate::domain::{
        activities::{ActivityKind, Score, TimeWindow},
        ports::{MockRoutingProvider, RoutingProvider},
    };
    use chrono::TimeZone;
    use rand::SeedableRng;

    fn home() -> Location {
        Location::new(50.7, 13.0, "Home".into(), "DE".into())
    }
    fn site(name: &str, lat: f64) -> Location {
        Location::new(lat, 13.0, name.into(), "DE".into())
    }
    fn ts(h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 13, h, 0, 0).unwrap()
    }
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

    /// Flexible suggestion; `hourly` is per-clock-hour fun over `[start_h, end_h)`.
    fn flex(loc: Location, start_h: u32, end_h: u32, hourly: Vec<f32>, min_h: i64) -> Arc<ActivitySuggestion> {
        Arc::new(ActivitySuggestion {
            id: format!("flex-{}", loc.name),
            kind: ActivityKind::Paragliding,
            location: loc.clone(),
            timing: Timing::Flexible {
                window: TimeWindow { start: ts(start_h), end: ts(end_h) },
                min_duration: Duration::hours(min_h),
            },
            title: format!("flex-{}", loc.name),
            description: String::new(),
            score: Some(Score { window_start: ts(start_h), hourly, reasons: vec![] }),
            allow_multiple: false,
        })
    }

    fn do_gene(act: &Arc<ActivitySuggestion>, duration: f32) -> Gene {
        Gene { action: GeneAction::Do(act.clone()), duration }
    }
    /// Build a genome from segment gene-lists with no overnight choices (single-day test slots have
    /// no night boundaries).
    fn gseg(segments: Vec<Vec<Gene>>) -> Genome {
        Genome { segments, overnight: vec![] }
    }
    fn wait_gene(duration: f32) -> Gene {
        Gene { action: GeneAction::Wait, duration }
    }

    async fn matrix_for(input: &SolverInput, routing: &Arc<dyn RoutingProvider>) -> DriveMatrix {
        build_matrix(routing.as_ref(), input).await.unwrap()
    }

    fn input(candidates: Vec<Arc<ActivitySuggestion>>, fixed: Vec<ScheduledActivity>) -> SolverInput {
        SolverInput {
            candidates: candidates.iter().map(|a| (**a).clone()).collect(),
            origin: home(),
            free_slots: vec![TimeWindow { start: ts(8), end: ts(18) }],
            fixed,
            num_alternatives: 1,
        }
    }

    #[tokio::test]
    async fn decode_is_deterministic() {
        let routing = constant_routing(15);
        let a = flex(site("A", 50.75), 8, 18, vec![1.0; 10], 2);
        let inp = input(vec![a.clone()], vec![]);
        let matrix = matrix_for(&inp, &routing).await;
        let genome = gseg(vec![vec![do_gene(&a, 0.5)]]);
        let p1 = decode(&genome, &inp, &matrix);
        let p2 = decode(&genome, &inp, &matrix);
        assert_eq!(p1.items.len(), 1);
        assert_eq!(p1.items[0].start, p2.items[0].start);
        assert_eq!(p1.items[0].end, p2.items[0].end);
        assert_eq!(p1.total_fun, p2.total_fun);
    }

    #[tokio::test]
    async fn duration_maps_between_min_and_feasible_max() {
        let routing = constant_routing(0); // no drive → feasible_max = full window
        let a = flex(site("A", 50.75), 8, 18, vec![1.0; 10], 2);
        let inp = input(vec![a.clone()], vec![]);
        let matrix = matrix_for(&inp, &routing).await;

        // norm 0 → min_duration (2h)
        let p_min = decode(&gseg(vec![vec![do_gene(&a, 0.0)]]), &inp, &matrix);
        assert_eq!(p_min.items[0].end - p_min.items[0].start, Duration::hours(2));

        // norm 1 → feasible_max: window is 8..18 (10h), start clamps to 8, so full 10h.
        let p_max = decode(&gseg(vec![vec![do_gene(&a, 1.0)]]), &inp, &matrix);
        assert_eq!(p_max.items[0].end - p_max.items[0].start, Duration::hours(10));
    }

    #[tokio::test]
    async fn repair_shrinks_then_removes_when_overpacked() {
        let routing = constant_routing(0);
        // Three activities each min 2h into a 10h segment with three max-duration genes → 30h
        // requested, must shrink; with all durations forced past the floor, one gets removed.
        let a = flex(site("A", 50.75), 8, 18, vec![1.0; 10], 4);
        let b = flex(site("B", 50.8), 8, 18, vec![1.0; 10], 4);
        let c = flex(site("C", 50.85), 8, 18, vec![1.0; 10], 4);
        let inp = input(vec![a.clone(), b.clone(), c.clone()], vec![]);
        let matrix = matrix_for(&inp, &routing).await;

        // 3 × min 4h = 12h > 10h segment: even at duration 0 they can't all fit → a removal.
        let mut genome = gseg(vec![vec![do_gene(&a, 1.0), do_gene(&b, 1.0), do_gene(&c, 1.0)]]);
        let mut rng = StdRng::seed_from_u64(42);
        repair(&mut genome, &inp, &matrix, &mut rng);

        assert!(genome.segments[0].len() < 3, "an over-full segment must drop a gene");
        // Post-repair walk is clean: every remaining gene places.
        let segs = partition_segments(&inp.free_slots, &inp.fixed, &inp.origin);
        let out = walk_segment(&genome.segments[0], &segs[0], &home(), segs[0].end_loc.as_ref(), &matrix);
        assert!(out.unplaceable.is_none());
    }

    #[tokio::test]
    async fn post_repair_decode_places_every_remaining_gene() {
        let routing = constant_routing(10);
        let a = flex(site("A", 50.75), 8, 18, vec![1.0; 10], 2);
        let b = flex(site("B", 50.8), 8, 18, vec![1.0; 10], 2);
        let inp = input(vec![a.clone(), b.clone()], vec![]);
        let matrix = matrix_for(&inp, &routing).await;

        let mut genome = gseg(vec![vec![do_gene(&a, 1.0), do_gene(&b, 1.0)]]);
        let mut rng = StdRng::seed_from_u64(7);
        repair(&mut genome, &inp, &matrix, &mut rng);

        let placed = decode(&genome, &inp, &matrix).items.len();
        assert_eq!(placed, genome.segments[0].len(), "no drops after repair");
    }

    #[tokio::test]
    async fn wait_into_better_hours_increases_fun() {
        let routing = constant_routing(0);
        // Bad first two hours, great last hours. min 1h so a short placement is legal.
        let a = flex(site("A", 50.75), 8, 18, vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 10.0, 10.0], 1);
        let inp = input(vec![a.clone()], vec![]);
        let matrix = matrix_for(&inp, &routing).await;

        // No wait, short duration → lands in the bad early hours.
        let early = decode(&gseg(vec![vec![do_gene(&a, 0.0)]]), &inp, &matrix);
        // Wait most of the segment first → pushed into the good hours.
        let late = decode(&gseg(vec![vec![wait_gene(0.9), do_gene(&a, 0.0)]]), &inp, &matrix);
        assert!(late.total_fun > early.total_fun, "late {} > early {}", late.total_fun, early.total_fun);
    }

    #[tokio::test]
    async fn duplicate_gene_schedules_activity_twice_when_allow_multiple() {
        let routing = constant_routing(0);
        // allow_multiple: true — same Arc may appear more than once in a plan.
        let a = Arc::new(ActivitySuggestion {
            allow_multiple: true,
            ..(*flex(site("A", 50.75), 8, 18, vec![1.0; 10], 2)).clone()
        });
        let inp = input(vec![a.clone()], vec![]);
        let matrix = matrix_for(&inp, &routing).await;
        let genome = gseg(vec![vec![do_gene(&a, 0.0), do_gene(&a, 0.0)]]);
        let plan = decode(&genome, &inp, &matrix);
        assert_eq!(plan.items.len(), 2);
        assert!(plan.items.iter().all(|i| i.location.as_ref().unwrap().name == "A"));
    }

    #[tokio::test]
    async fn dedup_removes_non_repeatable_duplicate_leaves_repeatable() {
        let once = flex(site("A", 50.75), 8, 18, vec![1.0; 10], 2); // allow_multiple: false
        let many = Arc::new(ActivitySuggestion {
            allow_multiple: true,
            ..(*flex(site("B", 50.76), 8, 18, vec![1.0; 10], 2)).clone()
        });

        // Two copies of `once` and two copies of `many` across two segments.
        let mut genome = gseg(vec![
            vec![do_gene(&once, 0.0), do_gene(&many, 0.0)],
            vec![do_gene(&once, 0.0), do_gene(&many, 0.0)],
        ]);
        dedup_single_use(&mut genome);

        let once_count: usize = genome.segments.iter().flat_map(|s| s.iter()).filter(|g| matches!(&g.action, GeneAction::Do(a) if Arc::ptr_eq(a, &once))).count();
        let many_count: usize = genome.segments.iter().flat_map(|s| s.iter()).filter(|g| matches!(&g.action, GeneAction::Do(a) if Arc::ptr_eq(a, &many))).count();
        assert_eq!(once_count, 1, "non-repeatable should appear exactly once");
        assert_eq!(many_count, 2, "repeatable should keep both occurrences");
    }

    #[tokio::test]
    async fn dedup_collapses_same_id_across_distinct_arcs() {
        // The real fan-out bug: one tour becomes several per-day suggestions — distinct Arcs, same
        // `id`. Single-use dedup must treat them as one activity (else the hike lands twice).
        let thursday = flex(site("Hike", 50.75), 8, 18, vec![1.0; 10], 2);
        let monday = Arc::new(ActivitySuggestion {
            title: "different-window".into(), // same id, different day/window → still one activity
            ..(*flex(site("Hike", 50.75), 8, 18, vec![1.0; 10], 2)).clone()
        });
        assert_eq!(thursday.id, monday.id, "same underlying tour → same id");
        assert!(!Arc::ptr_eq(&thursday, &monday), "distinct Arcs (distinct per-day suggestions)");

        let mut genome = gseg(vec![vec![do_gene(&thursday, 0.0)], vec![do_gene(&monday, 0.0)]]);
        dedup_single_use(&mut genome);

        let kept: usize = genome.segments.iter().flat_map(|s| s.iter()).count();
        assert_eq!(kept, 1, "same-id single-use activity must be scheduled at most once");
    }

    #[tokio::test]
    async fn online_commitment_adds_zero_drive() {
        // Home→site drive is nonzero; the online commitment splits the day but pays no drive.
        let home_key = home().to_key();
        let routing = matrix_routing(move |from, to| {
            if from.to_key() == home_key || to.to_key() == home_key {
                Duration::minutes(30)
            } else {
                Duration::minutes(10)
            }
        });
        let commitment = ScheduledActivity {
            kind: ActivityKind::Commitment,
            location: None, // online
            start: ts(12),
            end: ts(13),
            title: "standup".into(),
            description: String::new(),
            fun: 0.0,
        };
        let a = flex(site("A", 50.75), 8, 18, vec![1.0; 10], 2);
        let inp = input(vec![a.clone()], vec![commitment]);
        let matrix = matrix_for(&inp, &routing).await;

        // Place one activity in the first (8..12) segment.
        let genome = gseg(vec![vec![do_gene(&a, 0.0)], vec![]]);
        let plan = decode(&genome, &inp, &matrix);
        assert_eq!(plan.items.len(), 1);
        // Drive is only home→A→home (2 × 30 min); the online commitment contributes nothing.
        assert_eq!(plan.total_drive, Duration::minutes(60));
    }
}
