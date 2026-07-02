//! NSGA-II `WeekSolver` (§6 of docs/genetic-planner-design.md). A multi-objective genetic
//! algorithm producing a Pareto front of trip plans trading `total_fun` (↑) against
//! `total_drive` (↓), scheduling optional activities around fixed calendar commitments. Built
//! on the Phase 4 genome/decoder/repair (`genome.rs`) and the shared `placement.rs` matrix.

use std::cmp::Ordering;
use std::sync::Arc;

use anyhow::{bail, Result};
use async_trait::async_trait;
use chrono::Duration;
use rand::{rngs::StdRng, RngExt, SeedableRng};

use tracing::{debug, info, instrument, Span};

use crate::application::solvers::genome::{
    decode, night_count, overnight_candidates, repair, Gene, GeneAction, Genome,
};
use crate::domain::activities::OvernightSpot;
use crate::application::solvers::greedy_diversity::{build_plan, GreedyDiversitySolver};
use crate::application::solvers::placement::{build_matrix, partition_segments, DriveMatrix, Segment};
use crate::domain::{
    activities::{ActivitySuggestion, Plan, Timing},
    ports::{RoutingProvider, SolverInput, WeekSolver},
};

pub struct Nsga2Solver {
    routing: Arc<dyn RoutingProvider>,
    pub pop_size: usize,
    pub generations: usize,
    /// Per-operator probability applied per segment during mutation.
    pub mutation_rate: f32,
    /// Fixed RNG seed → reproducible output.
    pub seed: u64,
}

impl Nsga2Solver {
    pub fn new(routing: Arc<dyn RoutingProvider>) -> Self {
        Self {
            routing,
            pop_size: 100,
            generations: 100,
            mutation_rate: 0.3,
            seed: 42,
        }
    }
}

/// A genome and its decoded plan (objectives live on the plan).
#[derive(Clone)]
struct Individual {
    genome: Genome,
    plan: Plan,
}

impl Individual {
    fn new(genome: Genome, input: &SolverInput, matrix: &DriveMatrix) -> Self {
        let plan = decode(&genome, input, matrix);
        Self { genome, plan }
    }
}

#[async_trait]
impl WeekSolver for Nsga2Solver {
    #[instrument(
        skip_all,
        fields(
            candidates = input.candidates.len(),
            free_slots = input.free_slots.len(),
            fixed = input.fixed.len(),
            num_alternatives = input.num_alternatives,
            pop_size = self.pop_size,
            generations = self.generations,
            segments = tracing::field::Empty,
            alternatives = tracing::field::Empty,
        )
    )]
    async fn solve(&self, input: SolverInput) -> Result<Vec<Plan>> {
        let matrix = build_matrix(self.routing.as_ref(), &input).await?;
        validate_fixed(&input, &matrix)?;

        let segments = partition_segments(&input.free_slots, &input.fixed, &input.origin);
        Span::current().record("segments", segments.len());
        let pool: Vec<Arc<ActivitySuggestion>> =
            input.candidates.iter().cloned().map(Arc::new).collect();
        // Overnight-spot pool: home only today (see `overnight_candidates`). One choice per night.
        let overnight_pool = overnight_candidates(&input.origin);
        let mut rng = StdRng::seed_from_u64(self.seed);

        // Initial population: random genomes + one seeded from the greedy solver's plan (the
        // anti-regression floor — the front is never worse than greedy on fun).
        let mut pop: Vec<Individual> = Vec::with_capacity(self.pop_size);
        pop.push(Individual::new(
            greedy_seed(&self.routing, &input, &matrix, &segments, &overnight_pool),
            &input,
            &matrix,
        ));
        while pop.len() < self.pop_size {
            let g = random_genome(&pool, &segments, &overnight_pool, &mut rng);
            pop.push(Individual::new(g, &input, &matrix));
        }

        for _ in 0..self.generations {
            let (rank, crowd) = rank_and_crowding(&pop);
            let mut offspring = Vec::with_capacity(self.pop_size);
            for _ in 0..self.pop_size {
                let p1 = tournament(&pop, &rank, &crowd, &mut rng);
                let p2 = tournament(&pop, &rank, &crowd, &mut rng);
                let mut child = crossover(&pop[p1].genome, &pop[p2].genome, &mut rng);
                mutate(&mut child, &pool, &overnight_pool, self.mutation_rate, &mut rng);
                repair(&mut child, &input, &matrix, &mut rng);
                offspring.push(Individual::new(child, &input, &matrix));
            }
            // (μ+λ) elitist: fill the next generation from parents ∪ offspring by rank, then
            // crowding distance on the boundary front.
            pop.extend(offspring);
            pop = select_next(pop, self.pop_size);
        }

        let plans = pick_alternatives(&pop, input.num_alternatives);
        Span::current().record("alternatives", plans.len());
        for (i, p) in plans.iter().enumerate() {
            info!(
                rank = i,
                total_fun = p.total_fun,
                drive_min = p.total_drive.num_minutes(),
                items = p.items.len(),
                "alternative plan"
            );
        }
        Ok(plans)
    }
}

/// Err (no panic) if two consecutive located commitments can't be driven between in their gap.
fn validate_fixed(input: &SolverInput, matrix: &DriveMatrix) -> Result<()> {
    let mut located: Vec<_> = input
        .fixed
        .iter()
        .filter(|c| c.location.is_some())
        .collect();
    located.sort_by_key(|c| c.start);
    // ponytail: consecutive-pair check; ignores an online commitment eating into the gap between
    // two located ones. Refine if that under-reports infeasibility in practice.
    for pair in located.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let drive = matrix.get(a.location.as_ref().unwrap(), b.location.as_ref().unwrap());
        if drive > b.start - a.end {
            bail!(
                "un-driveable commitments: {} → {} needs {} min but only {} min between them",
                a.title,
                b.title,
                drive.num_minutes(),
                (b.start - a.end).num_minutes()
            );
        }
    }
    Ok(())
}

// ---- initialization ----------------------------------------------------------------------

fn random_gene(pool: &[Arc<ActivitySuggestion>], rng: &mut StdRng) -> Gene {
    let duration = rng.random_range(0.0..1.0);
    if pool.is_empty() || rng.random_range(0.0..1.0) < 0.3 {
        Gene { action: GeneAction::Wait, duration }
    } else {
        let act = pool[rng.random_range(0..pool.len())].clone();
        Gene { action: GeneAction::Do(act), duration }
    }
}

fn random_genome(
    pool: &[Arc<ActivitySuggestion>],
    segments: &[Segment],
    overnight_pool: &[Arc<OvernightSpot>],
    rng: &mut StdRng,
) -> Genome {
    let segment_genes = segments
        .iter()
        .map(|_| {
            let n = rng.random_range(0..=3);
            (0..n).map(|_| random_gene(pool, rng)).collect()
        })
        .collect();
    let overnight = (0..night_count(segments))
        .map(|_| overnight_pool[rng.random_range(0..overnight_pool.len())].clone())
        .collect();
    Genome { segments: segment_genes, overnight }
}

/// Translate the greedy solver's chosen plan into a genome by reproducing each placed activity's
/// duration segment-by-segment (§6 anti-regression floor). With uniform hourly scores this decodes
/// to the same total fun as greedy; with commitments interfering, repair heals any overlap.
fn greedy_seed(
    routing: &Arc<dyn RoutingProvider>,
    input: &SolverInput,
    matrix: &DriveMatrix,
    segments: &[Segment],
    overnight_pool: &[Arc<OvernightSpot>],
) -> Genome {
    let greedy = GreedyDiversitySolver::new(routing.clone());
    let plan = build_plan(&greedy, input, &[], matrix);

    let mut items = plan.items;
    items.sort_by_key(|a| a.start);
    let mut segment_genes: Vec<Vec<Gene>> = vec![Vec::new(); segments.len()];
    let mut carried = input.origin.clone();
    let mut cursor: Vec<_> = segments.iter().map(|s| (s.start, s.start_loc.clone())).collect();

    for item in &items {
        let seg_idx = match segments.iter().position(|s| s.start <= item.start && item.start < s.end) {
            Some(i) => i,
            None => continue,
        };
        // Titles are not unique — one site yields a same-titled suggestion per day/flyable
        // range — so also require the candidate's own timing to contain the placed span.
        let Some(act) = input
            .candidates
            .iter()
            .find(|c| {
                c.title == item.title
                    && match &c.timing {
                        Timing::Flexible { window, .. } => {
                            window.start <= item.start && item.end <= window.end
                        }
                        Timing::Fixed { start, end } => *start == item.start && *end == item.end,
                    }
            })
            .cloned()
            .map(Arc::new)
        else {
            continue;
        };
        let seg = &segments[seg_idx];
        let start_loc = cursor[seg_idx].1.clone().unwrap_or_else(|| carried.clone());
        let (time, loc) = (cursor[seg_idx].0, start_loc);

        let greedy_dur = item.end - item.start;
        let duration = match &act.timing {
            Timing::Flexible { window, min_duration } => {
                let drive_in = matrix.get(&loc, &act.location);
                let start = (time + drive_in).max(window.start);
                // Night boundaries drive out to the overnight spot (home for the seed); otherwise
                // to the pinned commitment.
                let end_loc = if seg.night_end {
                    Some(&input.origin)
                } else {
                    seg.end_loc.as_ref()
                };
                let drive_out = end_loc
                    .map(|e| matrix.get(&act.location, e))
                    .unwrap_or_else(Duration::zero);
                let feasible_max = window.end.min(seg.end - drive_out) - start;
                normalize(greedy_dur, *min_duration, feasible_max)
            }
            // Pinned; decode ignores the duration.
            Timing::Fixed { .. } => 0.0,
        };
        segment_genes[seg_idx].push(Gene { action: GeneAction::Do(act.clone()), duration });
        cursor[seg_idx] = (item.end, Some(act.location.clone()));
        carried = act.location.clone();
    }
    // The seed sleeps at home every night (overnight_pool[0] is home) — the anti-regression floor.
    let overnight = vec![overnight_pool[0].clone(); night_count(segments)];
    Genome { segments: segment_genes, overnight }
}

/// Back-solve the normalized `[0,1]` duration that maps onto `actual` within `[min, max]`.
fn normalize(actual: Duration, min: Duration, max: Duration) -> f32 {
    let span = (max - min).num_seconds() as f32;
    if span <= 0.0 {
        return 0.0;
    }
    (((actual - min).num_seconds() as f32) / span).clamp(0.0, 1.0)
}

// ---- domination, sorting, crowding -------------------------------------------------------

/// `a` dominates `b`: no worse on both objectives and strictly better on one. Fun compared via
/// `total_cmp` (may be NaN); drive as integer seconds.
fn dominates(a: &Plan, b: &Plan) -> bool {
    let fun = a.total_fun.total_cmp(&b.total_fun);
    let (da, db) = (a.total_drive.num_seconds(), b.total_drive.num_seconds());
    let no_worse = fun != Ordering::Less && da <= db;
    let strictly_better = fun == Ordering::Greater || da < db;
    no_worse && strictly_better
}

fn non_dominated_sort(pop: &[Individual]) -> Vec<Vec<usize>> {
    let n = pop.len();
    let mut dominated: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut dom_count = vec![0usize; n];
    let mut fronts: Vec<Vec<usize>> = vec![Vec::new()];

    for p in 0..n {
        for q in 0..n {
            if p == q {
                continue;
            }
            if dominates(&pop[p].plan, &pop[q].plan) {
                dominated[p].push(q);
            } else if dominates(&pop[q].plan, &pop[p].plan) {
                dom_count[p] += 1;
            }
        }
        if dom_count[p] == 0 {
            fronts[0].push(p);
        }
    }

    let mut i = 0;
    while !fronts[i].is_empty() {
        let mut next = Vec::new();
        for &p in &fronts[i] {
            for &q in dominated[p].clone().iter() {
                dom_count[q] -= 1;
                if dom_count[q] == 0 {
                    next.push(q);
                }
            }
        }
        i += 1;
        fronts.push(next);
    }
    fronts.pop(); // trailing empty
    fronts
}

/// Crowding distance per member of one front (larger = more isolated = more diverse).
fn crowding(front: &[usize], pop: &[Individual]) -> Vec<f32> {
    let m = front.len();
    let mut dist = vec![0f32; m];
    if m <= 2 {
        return vec![f32::INFINITY; m];
    }

    // Objective: fun.
    let mut order: Vec<usize> = (0..m).collect();
    order.sort_by(|&x, &y| pop[front[x]].plan.total_fun.total_cmp(&pop[front[y]].plan.total_fun));
    dist[order[0]] = f32::INFINITY;
    dist[order[m - 1]] = f32::INFINITY;
    let range = (pop[front[order[m - 1]]].plan.total_fun - pop[front[order[0]]].plan.total_fun).max(1e-9);
    for k in 1..m - 1 {
        let d = pop[front[order[k + 1]]].plan.total_fun - pop[front[order[k - 1]]].plan.total_fun;
        dist[order[k]] += d / range;
    }

    // Objective: drive (seconds).
    let drive = |i: usize| pop[front[i]].plan.total_drive.num_seconds() as f32;
    order.sort_by(|&x, &y| drive(x).total_cmp(&drive(y)));
    dist[order[0]] = f32::INFINITY;
    dist[order[m - 1]] = f32::INFINITY;
    let range = (drive(order[m - 1]) - drive(order[0])).max(1e-9);
    for k in 1..m - 1 {
        dist[order[k]] += (drive(order[k + 1]) - drive(order[k - 1])) / range;
    }
    dist
}

/// Per-individual rank (front index) and crowding distance, for tournament selection.
fn rank_and_crowding(pop: &[Individual]) -> (Vec<usize>, Vec<f32>) {
    let fronts = non_dominated_sort(pop);
    let mut rank = vec![0usize; pop.len()];
    let mut crowd = vec![0f32; pop.len()];
    for (r, front) in fronts.iter().enumerate() {
        let cd = crowding(front, pop);
        for (k, &idx) in front.iter().enumerate() {
            rank[idx] = r;
            crowd[idx] = cd[k];
        }
    }
    (rank, crowd)
}

/// Binary tournament: lower rank wins; ties broken by larger crowding distance.
fn tournament(pop: &[Individual], rank: &[usize], crowd: &[f32], rng: &mut StdRng) -> usize {
    let a = rng.random_range(0..pop.len());
    let b = rng.random_range(0..pop.len());
    if rank[a] < rank[b] || (rank[a] == rank[b] && crowd[a] > crowd[b]) {
        a
    } else {
        b
    }
}

/// Elitist environmental selection: take whole fronts by rank, then the least-crowded members of
/// the boundary front until `target` are chosen.
fn select_next(pop: Vec<Individual>, target: usize) -> Vec<Individual> {
    let fronts = non_dominated_sort(&pop);
    let mut chosen: Vec<usize> = Vec::with_capacity(target);
    for front in &fronts {
        if chosen.len() + front.len() <= target {
            chosen.extend_from_slice(front);
        } else {
            let cd = crowding(front, &pop);
            let mut order: Vec<usize> = (0..front.len()).collect();
            order.sort_by(|&x, &y| cd[y].total_cmp(&cd[x])); // crowding desc
            for &k in order.iter().take(target - chosen.len()) {
                chosen.push(front[k]);
            }
            break;
        }
    }
    // Move chosen individuals out of `pop` without cloning.
    let mut opt: Vec<Option<Individual>> = pop.into_iter().map(Some).collect();
    chosen.into_iter().map(|i| opt[i].take().unwrap()).collect()
}

/// The recommended plan (max fun on front 0 — ≥ greedy by the seed floor) first, then the rest of
/// the front by crowding distance (spread), padded from subsequent fronts, deduped by trade-off.
fn pick_alternatives(pop: &[Individual], num: usize) -> Vec<Plan> {
    let fronts = non_dominated_sort(pop);

    // Ordered candidate indices into `pop`: best-fun of front 0, then every front by crowding desc.
    let mut order: Vec<usize> = Vec::new();
    if let Some(front0) = fronts.first()
        && let Some(&best) = front0.iter().max_by(|&&a, &&b| {
            pop[a].plan.total_fun.total_cmp(&pop[b].plan.total_fun).then_with(|| {
                pop[b].plan.total_drive.num_seconds().cmp(&pop[a].plan.total_drive.num_seconds())
            })
        }) {
            order.push(best);
        }
    for front in &fronts {
        let cd = crowding(front, pop);
        let mut idx: Vec<usize> = (0..front.len()).collect();
        idx.sort_by(|&x, &y| cd[y].total_cmp(&cd[x]));
        order.extend(idx.into_iter().map(|k| front[k]));
    }

    let mut out: Vec<Plan> = Vec::new();
    let mut seen: Vec<(i64, i64)> = Vec::new();
    for i in order {
        let plan = &pop[i].plan;
        // The empty schedule is the zero-drive corner of the Pareto front (never dominated, and an
        // infinite-crowding boundary point), so it gets picked as an "alternative" — but a
        // do-nothing plan is useless to show. Skip it.
        if plan.items.is_empty() {
            continue;
        }
        let sig = ((plan.total_fun * 1000.0) as i64, plan.total_drive.num_seconds());
        if seen.contains(&sig) {
            continue;
        }
        seen.push(sig);
        out.push(plan.clone());
        if out.len() == num {
            break;
        }
    }
    // `distinct` < `num` means the population converged onto fewer trade-off points than requested
    // — the usual reason a secondary plan doesn't appear, not a bug downstream.
    debug!(
        fronts = fronts.len(),
        front0 = fronts.first().map(|f| f.len()).unwrap_or(0),
        pop = pop.len(),
        requested = num,
        distinct = out.len(),
        "pick_alternatives: distinct trade-off points after objective dedup"
    );
    out
}

// ---- operators ---------------------------------------------------------------------------

/// Per-segment cut-and-splice: prefix of A + suffix of B, no dedup (§6). Overnight choices are
/// inherited per night from one parent or the other.
fn crossover(a: &Genome, b: &Genome, rng: &mut StdRng) -> Genome {
    let segments = a
        .segments
        .iter()
        .zip(b.segments.iter())
        .map(|(sa, sb)| {
            let cut_a = rng.random_range(0..=sa.len());
            let cut_b = rng.random_range(0..=sb.len());
            let mut child: Vec<Gene> = sa[..cut_a].to_vec();
            child.extend_from_slice(&sb[cut_b..]);
            child
        })
        .collect();
    let overnight = a
        .overnight
        .iter()
        .zip(b.overnight.iter())
        .map(|(oa, ob)| if rng.random_range(0.0..1.0) < 0.5 { oa.clone() } else { ob.clone() })
        .collect();
    Genome { segments, overnight }
}

fn mutate(
    genome: &mut Genome,
    pool: &[Arc<ActivitySuggestion>],
    overnight_pool: &[Arc<OvernightSpot>],
    rate: f32,
    rng: &mut StdRng,
) {
    let hit = |rng: &mut StdRng| rng.random_range(0.0..1.0) < rate;

    for seg in genome.segments.iter_mut() {
        for gene in seg.iter_mut() {
            if hit(rng) {
                gene.duration = (gene.duration + rng.random_range(-0.2..0.2)).clamp(0.0, 1.0);
            }
        }
        if hit(rng) {
            let g = random_gene(pool, rng);
            let at = rng.random_range(0..=seg.len());
            seg.insert(at, g);
        }
        if hit(rng) && !seg.is_empty() {
            let at = rng.random_range(0..seg.len());
            seg.remove(at);
        }
        if hit(rng) && seg.len() >= 2 {
            let (i, j) = (rng.random_range(0..seg.len()), rng.random_range(0..seg.len()));
            seg.swap(i, j);
        }
    }

    // Move a gene across segments.
    if genome.segments.len() >= 2 && hit(rng) {
        let from = rng.random_range(0..genome.segments.len());
        if !genome.segments[from].is_empty() {
            let at = rng.random_range(0..genome.segments[from].len());
            let gene = genome.segments[from].remove(at);
            let to = rng.random_range(0..genome.segments.len());
            let pos = rng.random_range(0..=genome.segments[to].len());
            genome.segments[to].insert(pos, gene);
        }
    }

    // Reassign a night's overnight spot. ponytail: inert while `overnight_pool` is just home
    // (one candidate = same pick); the camping feature makes this a real optimization lever.
    if !genome.overnight.is_empty() && overnight_pool.len() > 1 && hit(rng) {
        let night = rng.random_range(0..genome.overnight.len());
        genome.overnight[night] = overnight_pool[rng.random_range(0..overnight_pool.len())].clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        activities::{ActivityKind, Score, ScheduledActivity, TimeWindow},
        location::Location,
        ports::MockRoutingProvider,
    };
    use chrono::{DateTime, TimeZone, Utc};

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
            Ok(locs.iter().map(|a| locs.iter().map(|b| f(a, b)).collect()).collect())
        });
        Arc::new(r)
    }
    fn constant_routing(minutes: i64) -> Arc<dyn RoutingProvider> {
        matrix_routing(move |_, _| Duration::minutes(minutes))
    }

    fn flex(loc: Location, start_h: u32, end_h: u32, fun: f32) -> ActivitySuggestion {
        let hours = (end_h - start_h).max(1) as f32;
        ActivitySuggestion {
            kind: ActivityKind::Paragliding,
            location: loc.clone(),
            timing: Timing::Flexible {
                window: TimeWindow { start: ts(start_h), end: ts(end_h) },
                min_duration: Duration::hours(2),
            },
            title: format!("flex-{}", loc.name),
            description: String::new(),
            score: Some(Score { window_start: ts(start_h), hourly: vec![fun / hours; hours as usize], reasons: vec![] }),
        }
    }

    fn base_input(candidates: Vec<ActivitySuggestion>, num_alternatives: usize) -> SolverInput {
        SolverInput {
            candidates,
            origin: home(),
            free_slots: vec![TimeWindow { start: ts(8), end: ts(18) }],
            fixed: vec![],
            num_alternatives,
        }
    }

    fn small_solver(routing: Arc<dyn RoutingProvider>) -> Nsga2Solver {
        let mut s = Nsga2Solver::new(routing);
        s.pop_size = 30;
        s.generations = 20;
        s
    }

    #[tokio::test]
    async fn deterministic_same_seed_same_output() {
        let cands = vec![
            flex(site("A", 50.75), 8, 14, 0.9),
            flex(site("B", 50.8), 10, 18, 0.6),
        ];
        let s1 = small_solver(constant_routing(15));
        let s2 = small_solver(constant_routing(15));
        let p1 = s1.solve(base_input(cands.clone(), 3)).await.unwrap();
        let p2 = s2.solve(base_input(cands, 3)).await.unwrap();
        assert_eq!(p1.len(), p2.len());
        for (a, b) in p1.iter().zip(&p2) {
            assert_eq!(a.total_fun, b.total_fun);
            assert_eq!(a.total_drive, b.total_drive);
            assert_eq!(a.items.len(), b.items.len());
        }
    }

    #[tokio::test]
    async fn anti_regression_beats_or_matches_greedy() {
        let cands = vec![
            flex(site("A", 50.75), 8, 14, 0.9),
            flex(site("B", 50.8), 10, 18, 0.6),
        ];
        let routing = constant_routing(15);
        let greedy = GreedyDiversitySolver::new(routing.clone());
        let greedy_fun = greedy.solve(base_input(cands.clone(), 1)).await.unwrap()[0].total_fun;

        let best = small_solver(routing)
            .solve(base_input(cands, 1))
            .await
            .unwrap()[0]
            .total_fun;
        assert!(best >= greedy_fun - 1e-4, "nsga2 {best} < greedy {greedy_fun}");
    }

    #[tokio::test]
    async fn optimal_pick_on_overlap_fixture() {
        // Same fixture as greedy's `picks_higher_fun_when_overlapping`: only one fits, pick B (0.9).
        let cands = vec![
            flex(site("A", 50.75), 10, 14, 0.5),
            flex(site("B", 50.8), 10, 14, 0.9),
        ];
        let plans = small_solver(constant_routing(15)).solve(base_input(cands, 1)).await.unwrap();
        assert!(plans[0].total_fun >= 0.9 - 1e-4, "should reach B's fun, got {}", plans[0].total_fun);
    }

    #[tokio::test]
    async fn pareto_spread_returns_distinct_tradeoffs() {
        // A near + cheap-ish, B far → different fun/drive trade-offs on the front.
        let home_key = home().to_key();
        let routing = matrix_routing(move |from, to| {
            let far = from.name == "B" || to.name == "B";
            if from.to_key() == home_key || to.to_key() == home_key {
                if far { Duration::hours(2) } else { Duration::minutes(20) }
            } else {
                Duration::minutes(30)
            }
        });
        let cands = vec![
            flex(site("A", 50.75), 8, 18, 0.5),
            flex(site("B", 50.8), 8, 18, 0.9),
        ];
        let plans = small_solver(routing).solve(base_input(cands, 3)).await.unwrap();
        // At least two genuinely different trade-offs.
        let distinct: std::collections::HashSet<(i64, i64)> = plans
            .iter()
            .map(|p| ((p.total_fun * 100.0) as i64, p.total_drive.num_seconds()))
            .collect();
        assert!(distinct.len() >= 2, "expected spread, got {distinct:?}");
    }

    #[tokio::test]
    async fn greedy_seed_distinguishes_same_titled_windows() {
        // One site → several same-titled suggestions (per flyable range). The seed must map
        // each greedy placement back onto the candidate whose window contains it.
        let routing = constant_routing(0);
        let cands = vec![
            flex(site("A", 50.75), 8, 12, 0.9),
            flex(site("A", 50.75), 13, 18, 0.8),
        ];
        let input = base_input(cands, 1);
        let matrix = build_matrix(routing.as_ref(), &input).await.unwrap();
        let segments = partition_segments(&input.free_slots, &input.fixed, &input.origin);
        let overnight_pool = overnight_candidates(&input.origin);

        let genome = greedy_seed(&routing, &input, &matrix, &segments, &overnight_pool);
        let plan = decode(&genome, &input, &matrix);
        assert_eq!(plan.items.len(), 2, "both same-titled windows must survive the seed round-trip");
    }

    #[tokio::test]
    async fn undriveable_commitments_error_without_panic() {
        // Two located meetings 30 min apart in time but 2 h of driving between them.
        let routing = matrix_routing(|_, _| Duration::hours(2));
        let commit = |start: u32, end: u32, loc: Location| ScheduledActivity {
            kind: ActivityKind::Commitment,
            location: Some(loc),
            start: ts(start),
            end: ts(end),
            title: format!("meeting-{start}"),
            description: String::new(),
            fun: 0.0,
        };
        let mut input = base_input(vec![flex(site("A", 50.75), 8, 18, 0.5)], 1);
        input.fixed = vec![
            commit(10, 11, site("X", 50.9)),
            commit(11, 12, site("Y", 51.0)), // only 0 min gap, needs 2 h
        ];
        let err = small_solver(routing).solve(input).await;
        assert!(err.is_err());
    }
}
