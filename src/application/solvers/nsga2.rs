//! NSGA-II `WeekSolver` (§6 of docs/genetic-planner-design.md). A multi-objective genetic
//! algorithm producing a Pareto front of trip plans trading `total_fun` (↑) against
//! `total_drive` (↓), scheduling optional activities around fixed calendar commitments. Built
//! on the Phase 4 genome/decoder/repair (`genome.rs`) and the shared `placement.rs` matrix.

use std::cmp::Ordering;
use std::sync::Arc;

use anyhow::{bail, Result};
use async_trait::async_trait;
use rand::{rngs::StdRng, RngExt, SeedableRng};
use rayon::prelude::*;

use tracing::{debug, info, instrument, Span};

use crate::application::solvers::genome::{
    decode, dedup_single_use, night_count, overnight_candidates, repair, Gene, GeneAction, Genome,
};
use crate::application::solvers::placement::{crow_flies_drive, partition_segments, Segment};
use crate::domain::{
    activities::{ActivitySuggestion, OvernightSpot, Plan, TimeWindow},
    ports::{SolverInput, WeekSolver},
};

pub struct Nsga2Solver {
    pub pop_size: usize,
    pub generations: usize,
    /// Per-operator probability applied per segment during mutation.
    pub mutation_rate: f32,
    /// Fixed RNG seed → reproducible output.
    pub seed: u64,
}

impl Nsga2Solver {
    pub fn new() -> Self {
        Self {
            pop_size: 10_000,
            generations: 1000,
            mutation_rate: 0.3,
            seed: 42,
        }
    }
}

impl Default for Nsga2Solver {
    fn default() -> Self {
        Self::new()
    }
}

/// A genome and its decoded plan (objectives live on the plan).
#[derive(Clone)]
struct Individual {
    genome: Genome,
    plan: Plan,
}

impl Individual {
    fn new(genome: Genome, input: &SolverInput) -> Self {
        let plan = decode(&genome, input);
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
        validate_fixed(&input)?;

        // The GA is CPU-bound (rayon inside, sequential selection between generations). Run it on
        // the blocking pool so it doesn't park a tokio worker for the whole solve. The span is
        // propagated so field recording and per-generation logs stay attributed to this solve.
        let (pop_size, generations, mutation_rate, seed) =
            (self.pop_size, self.generations, self.mutation_rate, self.seed);
        let span = Span::current();
        tokio::task::spawn_blocking(move || {
            let _guard = span.enter();
            run_ga(input, pop_size, generations, mutation_rate, seed)
        })
        .await
        .map_err(|e| anyhow::anyhow!("solver task panicked: {e}"))
    }
}

/// The NSGA-II loop itself: pure CPU, no async. Split out of `solve` so it can run under
/// `spawn_blocking`. Infallible — feasibility is checked in `solve` before we get here.
fn run_ga(
    input: SolverInput,
    pop_size: usize,
    generations: usize,
    mutation_rate: f32,
    seed: u64,
) -> Vec<Plan> {
    let segments = partition_segments(&input.free_slots, &input.fixed, &input.origin);
    Span::current().record("segments", segments.len());
    let pool: Vec<Arc<ActivitySuggestion>> =
        input.candidates.iter().cloned().map(Arc::new).collect();
    // Per-segment candidate pools: an activity is only ever inserted into / moved to a segment
    // its window overlaps, so operators don't seed wrong-day genes that repair would delete
    // (shrinking good genes to zero on the way). See `segment_pools`.
    let seg_pools = segment_pools(&pool, &segments);
    // Overnight-spot pool: home only today (see `overnight_candidates`). One choice per night.
    let overnight_pool = overnight_candidates(&input.origin);
    let mut rng = StdRng::seed_from_u64(seed);

    // Initial population: random genomes.
    let mut pop: Vec<Individual> = Vec::with_capacity(pop_size);
    while pop.len() < pop_size {
        let mut g = random_genome(&seg_pools, &segments, &overnight_pool, &mut rng);
        dedup_single_use(&mut g);
        pop.push(Individual::new(g, &input));
    }

    for generation in 0..generations {
        let (rank, crowd) = rank_and_crowding(&pop);
        // Parent selection is sequential (rng not Send); crossover/eval are parallel.
        let pairs: Vec<(usize, usize)> = (0..pop_size)
            .map(|_| (tournament(&pop, &rank, &crowd, &mut rng), tournament(&pop, &rank, &crowd, &mut rng)))
            .collect();
        let offspring: Vec<Individual> = pairs
            .into_par_iter()
            .enumerate()
            .map(|(i, (p1, p2))| {
                let mut wrng = StdRng::seed_from_u64(seed ^ (generation as u64 * pop_size as u64 + i as u64));
                let mut child = crossover(&pop[p1].genome, &pop[p2].genome, &mut wrng);
                mutate(&mut child, &seg_pools, &segments, &overnight_pool, mutation_rate, &mut wrng);
                dedup_single_use(&mut child);
                repair(&mut child, &input, &mut wrng);
                Individual::new(child, &input)
            })
            .collect();
        // (μ+λ) elitist: fill the next generation from parents ∪ offspring by rank, then
        // crowding distance on the boundary front.
        pop.extend(offspring);
        pop = select_next(pop, pop_size);

        let best = pop.iter().max_by(|a, b| a.plan.total_fun.partial_cmp(&b.plan.total_fun).unwrap_or(Ordering::Equal));
        if let Some(b) = best {
            info!(generation, best_fun = b.plan.total_fun, best_drive_min = b.plan.total_drive.num_minutes(), "generation");
        }
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
    plans
}

/// Err (no panic) if two consecutive located commitments can't be driven between in their gap.
fn validate_fixed(input: &SolverInput) -> Result<()> {
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
        let drive = crow_flies_drive(a.location.as_ref().unwrap(), b.location.as_ref().unwrap());
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

/// True if an activity with time window `w` can occur inside segment `seg` (windows overlap). This
/// is the only "fits" test the operators apply — drive-time and packing feasibility stay repair's
/// job, so the search still explores tight/infeasible packings within a day.
fn window_overlaps(w: &TimeWindow, seg: &Segment) -> bool {
    w.start < seg.end && w.end > seg.start
}

/// For each segment, the candidates whose window overlaps it — the only activities an operator may
/// place there. Preserves `pool` order (deterministic). Cheap: candidates × segments.
fn segment_pools(
    pool: &[Arc<ActivitySuggestion>],
    segments: &[Segment],
) -> Vec<Vec<Arc<ActivitySuggestion>>> {
    segments
        .iter()
        .map(|seg| {
            pool.iter()
                .filter(|a| window_overlaps(&a.timing.window(), seg))
                .cloned()
                .collect()
        })
        .collect()
}

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
    seg_pools: &[Vec<Arc<ActivitySuggestion>>],
    segments: &[Segment],
    overnight_pool: &[Arc<OvernightSpot>],
    rng: &mut StdRng,
) -> Genome {
    let segment_genes = seg_pools
        .iter()
        .map(|pool| {
            let n = rng.random_range(0..=3);
            (0..n).map(|_| random_gene(pool, rng)).collect()
        })
        .collect();
    let overnight = (0..night_count(segments))
        .map(|_| overnight_pool[rng.random_range(0..overnight_pool.len())].clone())
        .collect();
    Genome { segments: segment_genes, overnight }
}

// ---- domination, sorting, crowding -------------------------------------------------------

/// `a` dominates `b`: no worse on both objectives and strictly better on one. Fun compared via
/// `total_cmp` (may be NaN); drive as integer seconds. The canonical definition `non_dominated_sort`
/// must agree with — now only the oracle test calls it directly (the fast sort reduces domination
/// to the drive axis), so it's test-only.
#[cfg(test)]
fn dominates(a: &Plan, b: &Plan) -> bool {
    let fun = a.total_fun.total_cmp(&b.total_fun);
    let (da, db) = (a.total_drive.num_seconds(), b.total_drive.num_seconds());
    let no_worse = fun != Ordering::Less && da <= db;
    let strictly_better = fun == Ordering::Greater || da < db;
    no_worse && strictly_better
}

/// Fenwick tree for prefix-max over point updates (1-indexed internally). Used by the two-objective
/// non-dominated sort to query, in O(log N), the deepest front reachable from a dominator.
struct FenwickMax {
    tree: Vec<i64>,
}

impl FenwickMax {
    fn new(size: usize) -> Self {
        Self { tree: vec![0; size + 1] }
    }
    /// Raise the value stored at 0-indexed `pos` to at least `val`.
    fn update(&mut self, pos: usize, val: i64) {
        let mut i = pos + 1;
        while i < self.tree.len() {
            self.tree[i] = self.tree[i].max(val);
            i += i & i.wrapping_neg();
        }
    }
    /// Max stored value over the prefix `[0, pos]` (inclusive). 0 if nothing was stored there.
    fn prefix_max(&self, pos: usize) -> i64 {
        let mut i = pos + 1;
        let mut res = 0;
        while i > 0 {
            res = res.max(self.tree[i]);
            i -= i & i.wrapping_neg();
        }
        res
    }
}

/// O(N log N) non-dominated sort, valid because there are exactly two objectives (`total_fun` ↑,
/// `total_drive` ↓). Produces the same front partition as the classic O(N²) Deb sort (verified by
/// the `fast_sort_matches_reference_random_and_ties` oracle test) with no O(N²) adjacency
/// structure — the Deb sort's multi-GB memory blowup at large `pop_size` is gone.
///
/// Order by fun desc (ties: drive asc), so every dominator of a point precedes it. Sweep fun-group
/// by fun-group over a Fenwick prefix-max keyed by drive: a point's front is one past the deepest
/// dominator. The group split is load-bearing — across groups (strictly higher fun) equal drive
/// still dominates (weak, `<=`), but within a group (equal fun) domination needs strictly smaller
/// drive (`<`). Committing each group to the tree only *after* computing its fronts keeps those two
/// rules apart; intra-group strict domination is handled by a running best-of-smaller-drive.
///
/// Fronts come out in rank order; within-front order is unspecified (callers re-sort by crowding).
fn non_dominated_sort(pop: &[Individual]) -> Vec<Vec<usize>> {
    let n = pop.len();
    if n == 0 {
        return Vec::new();
    }
    let drive = |i: usize| pop[i].plan.total_drive.num_seconds();
    let fun_eq = |a: usize, b: usize| {
        pop[a].plan.total_fun.total_cmp(&pop[b].plan.total_fun) == Ordering::Equal
    };

    // Compress drive values → dense ranks for the Fenwick tree.
    let mut uniq: Vec<i64> = (0..n).map(drive).collect();
    uniq.sort_unstable();
    uniq.dedup();
    let rank = |d: i64| uniq.partition_point(|&x| x < d);

    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        pop[b].plan.total_fun.total_cmp(&pop[a].plan.total_fun).then(drive(a).cmp(&drive(b)))
    });

    let mut tree = FenwickMax::new(uniq.len());
    let mut front_of = vec![0usize; n]; // front index per individual
    let mut max_front = 0usize;

    let mut g = 0;
    while g < n {
        // Group [g, h) = one block of equal fun (already contiguous in `order`).
        let mut h = g;
        while h < n && fun_eq(order[h], order[g]) {
            h += 1;
        }

        // Compute fronts for the group without letting its own members leak through the tree.
        // `best_less` = max (front+1) among already-processed group members with strictly smaller
        // drive; a whole equal-drive sub-run shares one front and none of them counts for the others.
        let mut best_less: i64 = 0;
        let mut a = g;
        while a < h {
            let d = drive(order[a]);
            let mut b = a;
            while b < h && drive(order[b]) == d {
                b += 1;
            }
            // Deepest dominator: higher-fun points with drive ≤ d (tree) or smaller-drive same-fun
            // members (`best_less`). Stored/compared as front+1, so 0 means "no dominator".
            let f = tree.prefix_max(rank(d)).max(best_less) as usize;
            for &i in &order[a..b] {
                front_of[i] = f;
            }
            best_less = best_less.max(f as i64 + 1);
            max_front = max_front.max(f);
            a = b;
        }
        // Commit the group so lower-fun groups can dominate through it (weak drive rule).
        for &i in &order[g..h] {
            tree.update(rank(drive(i)), front_of[i] as i64 + 1);
        }
        g = h;
    }

    let mut fronts: Vec<Vec<usize>> = vec![Vec::new(); max_front + 1];
    for i in 0..n {
        fronts[front_of[i]].push(i);
    }
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

/// The recommended plan (max fun on front 0) first, then the rest of the front by crowding
/// distance (spread), padded from subsequent fronts, deduped by trade-off.
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
    seg_pools: &[Vec<Arc<ActivitySuggestion>>],
    segments: &[Segment],
    overnight_pool: &[Arc<OvernightSpot>],
    rate: f32,
    rng: &mut StdRng,
) {
    let hit = |rng: &mut StdRng| rng.random_range(0.0..1.0) < rate;

    for (i, seg) in genome.segments.iter_mut().enumerate() {
        for gene in seg.iter_mut() {
            if hit(rng) {
                gene.duration = (gene.duration + rng.random_range(-0.2..0.2)).clamp(0.0, 1.0);
            }
        }
        if hit(rng) {
            // Insert only a candidate that can occur in this segment (or a Wait).
            let g = random_gene(&seg_pools[i], rng);
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

    // Move a gene to another segment it can actually occur in — a `Do` gene only to segments its
    // window overlaps (else the move just seeds a wrong-day gene for repair to delete); a `Wait`
    // gene is day-agnostic, so any segment.
    if genome.segments.len() >= 2 && hit(rng) {
        let from = rng.random_range(0..genome.segments.len());
        if !genome.segments[from].is_empty() {
            let at = rng.random_range(0..genome.segments[from].len());
            let gene_window = match &genome.segments[from][at].action {
                GeneAction::Do(act) => Some(act.timing.window()),
                GeneAction::Wait => None,
            };
            let valid: Vec<usize> = (0..genome.segments.len())
                .filter(|&j| gene_window.is_none_or(|w| window_overlaps(&w, &segments[j])))
                .collect();
            if !valid.is_empty() {
                let gene = genome.segments[from].remove(at);
                let to = valid[rng.random_range(0..valid.len())];
                let pos = rng.random_range(0..=genome.segments[to].len());
                genome.segments[to].insert(pos, gene);
            }
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
    use chrono::{DateTime, Duration, TimeZone, Utc};
    use crate::domain::{
        activities::{ActivityKind, Score, ScheduledActivity, TimeWindow, Timing},
        location::Location,
    };

    fn home() -> Location {
        Location::new(50.7, 13.0, "Home".into(), "DE".into())
    }
    fn site(name: &str, lat: f64) -> Location {
        Location::new(lat, 13.0, name.into(), "DE".into())
    }
    fn ts(h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 13, h, 0, 0).unwrap()
    }

    fn flex(loc: Location, start_h: u32, end_h: u32, fun: f32) -> ActivitySuggestion {
        let hours = (end_h - start_h).max(1) as f32;
        ActivitySuggestion {
            id: format!("flex-{}", loc.name),
            kind: ActivityKind::Paragliding,
            location: loc.clone(),
            timing: Timing::Flexible {
                window: TimeWindow { start: ts(start_h), end: ts(end_h) },
                min_duration: Duration::hours(2),
            },
            title: format!("flex-{}", loc.name),
            description: String::new(),
            score: Some(Score { window_start: ts(start_h), hourly: vec![fun / hours; hours as usize], reasons: vec![] }),
            allow_multiple: false,
        }
    }

    fn fixed_cand(loc: Location, start_h: u32, end_h: u32, fun: f32) -> ActivitySuggestion {
        let hours = (end_h - start_h).max(1) as usize;
        ActivitySuggestion {
            id: format!("fixed-{}", loc.name),
            kind: ActivityKind::Event,
            location: loc.clone(),
            timing: Timing::Fixed { start: ts(start_h), end: ts(end_h) },
            title: format!("fixed-{}", loc.name),
            description: String::new(),
            score: Some(Score { window_start: ts(start_h), hourly: vec![fun / hours as f32; hours], reasons: vec![] }),
            allow_multiple: false,
        }
    }

    /// Reference O(N²) Deb non-dominated sort — the oracle the fast sort must match.
    fn non_dominated_sort_ref(pop: &[Individual]) -> Vec<Vec<usize>> {
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
            for &p in fronts[i].clone().iter() {
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
        fronts.pop();
        fronts
    }

    /// Per-individual rank (front index) from a fronts partition, for order-independent comparison.
    fn ranks(fronts: &[Vec<usize>], n: usize) -> Vec<usize> {
        let mut r = vec![usize::MAX; n];
        for (f, front) in fronts.iter().enumerate() {
            for &i in front {
                r[i] = f;
            }
        }
        r
    }

    /// A minimal individual carrying only the objectives the sort reads.
    fn ind(fun: f32, drive_min: i64) -> Individual {
        let plan = Plan { items: vec![], total_fun: fun, total_drive: Duration::minutes(drive_min) };
        Individual { genome: Genome { segments: vec![], overnight: vec![] }, plan }
    }

    #[test]
    fn fast_sort_matches_reference_random_and_ties() {
        let mut rng = StdRng::seed_from_u64(1);
        // Several random populations, plus a heavy-tie population (small value ranges → many equal
        // fun, equal drive, and points equal on both — the case patience-sort ties can break).
        for round in 0..40 {
            let n = 1 + (round % 60);
            let (fun_range, drive_range) = if round % 3 == 0 { (3i32, 3i64) } else { (50, 50) };
            let pop: Vec<Individual> = (0..n)
                .map(|_| {
                    ind(
                        rng.random_range(0..fun_range) as f32,
                        rng.random_range(0..drive_range),
                    )
                })
                .collect();
            let fast = ranks(&non_dominated_sort(&pop), n);
            let refr = ranks(&non_dominated_sort_ref(&pop), n);
            assert_eq!(fast, refr, "front partition mismatch (round {round}, n={n})");
        }
    }

    /// A two-day free slot (→ per-day segments) and one flexible candidate per day.
    fn two_day_setup() -> (Vec<Segment>, Vec<Arc<ActivitySuggestion>>) {
        let slot = TimeWindow { start: ts(8), end: ts(18) + Duration::days(1) };
        let segments = partition_segments(&[slot], &[], &home());
        assert!(segments.len() >= 2, "expected a per-day split, got {}", segments.len());
        let day0 = Arc::new(flex(site("A", 50.75), 9, 15, 0.5)); // window on day 0
        let day1 = Arc::new(ActivitySuggestion {
            timing: Timing::Flexible {
                window: TimeWindow { start: ts(9) + Duration::days(1), end: ts(15) + Duration::days(1) },
                min_duration: Duration::hours(2),
            },
            ..flex(site("B", 50.8), 9, 15, 0.5)
        });
        (segments, vec![day0, day1])
    }

    #[test]
    fn segment_pools_membership_matches_window_overlap() {
        let (segments, pool) = two_day_setup();
        let pools = segment_pools(&pool, &segments);
        // A candidate is in a segment's pool iff (and only iff) their windows overlap.
        for (seg, p) in segments.iter().zip(&pools) {
            for cand in &pool {
                let present = p.iter().any(|x| Arc::ptr_eq(x, cand));
                assert_eq!(present, window_overlaps(&cand.timing.window(), seg));
            }
        }
    }

    #[test]
    fn operators_only_place_fitting_candidates() {
        // Init + a heavy mutation pass must never leave a Do gene in a segment its window can't
        // occur in — the wrong-day pathology this change removes.
        let (segments, pool) = two_day_setup();
        let seg_pools = segment_pools(&pool, &segments);
        let overnight = overnight_candidates(&home());
        let mut rng = StdRng::seed_from_u64(3);
        let mut g = random_genome(&seg_pools, &segments, &overnight, &mut rng);
        for _ in 0..200 {
            mutate(&mut g, &seg_pools, &segments, &overnight, 0.9, &mut rng);
        }
        for (seg, genes) in segments.iter().zip(&g.segments) {
            for gene in genes {
                if let GeneAction::Do(act) = &gene.action {
                    assert!(
                        window_overlaps(&act.timing.window(), seg),
                        "operator placed a non-fitting candidate in a segment"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn fixed_candidate_is_placed_at_its_exact_times() {
        // Fixed timing (events) must land at exactly [start, end], not shifted like Flexible.
        let cands = vec![fixed_cand(site("E", 50.75), 10, 13, 3.0)];
        let plans = small_solver().solve(base_input(cands, 1)).await.unwrap();
        let placed = plans[0]
            .items
            .iter()
            .find(|i| i.kind == ActivityKind::Event)
            .expect("fixed event should be scheduled");
        assert_eq!(placed.start, ts(10));
        assert_eq!(placed.end, ts(13));
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

    fn small_solver() -> Nsga2Solver {
        let mut s = Nsga2Solver::new();
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
        let s1 = small_solver();
        let s2 = small_solver();
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
    async fn optimal_pick_on_overlap_fixture() {
        // Overlapping windows: only one fits, pick the higher-fun one.
        let cands = vec![
            flex(site("A", 50.75), 10, 14, 0.5),
            flex(site("B", 50.8), 10, 14, 0.9),
        ];
        let plans = small_solver().solve(base_input(cands, 1)).await.unwrap();
        assert!(plans[0].total_fun >= 0.9 - 1e-4, "should reach B's fun, got {}", plans[0].total_fun);
    }

    #[tokio::test]
    async fn pareto_spread_returns_distinct_tradeoffs() {
        // A near, B far (crow-flies from home coords) → different fun/drive trade-offs on the front.
        let cands = vec![
            flex(site("A", 50.75), 8, 18, 0.5), // ~5 km north of home
            flex(site("B", 51.6), 8, 18, 0.9),  // ~100 km north → real drive cost
        ];
        let plans = small_solver().solve(base_input(cands, 3)).await.unwrap();
        // At least two genuinely different trade-offs.
        let distinct: std::collections::HashSet<(i64, i64)> = plans
            .iter()
            .map(|p| ((p.total_fun * 100.0) as i64, p.total_drive.num_seconds()))
            .collect();
        assert!(distinct.len() >= 2, "expected spread, got {distinct:?}");
    }

    #[tokio::test]
    async fn undriveable_commitments_error_without_panic() {
        // Two located meetings with a 0-min gap but real driving between them → infeasible.
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
        let err = small_solver().solve(input).await;
        assert!(err.is_err());
    }
}
