#![allow(dead_code)]
// ponytail: routing stack not wired into AppState yet (CrowFlies stands in); kept per owner's call.

//! Shared helpers for provider matrix endpoints: per-pair cache key/TTL, cache-first
//! assembly, and incremental fetch planning so only the new pairs hit the provider.

use std::collections::HashMap;
use std::time::{Duration as StdDuration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use chrono::Duration;
use futures::stream::{FuturesUnordered, StreamExt};
use rand::RngExt;
use tokio::sync::Semaphore;
use tracing;

use crate::{
    adapters::persistence::cache::PersistentCache,
    domain::{location::Location, ports::RoutingProvider},
};

/// A provider's single `sources × targets` matrix request. Providers cap locations per request,
/// so `tile_matrix` splits large grids into blocks and calls this per block.
#[async_trait]
pub trait MatrixBlock {
    /// One request for a `sources × targets` block, indexed `[source][target]`;
    /// null/unroutable cells are `None`.
    async fn matrix_block(
        &self,
        sources: &[Location],
        targets: &[Location],
    ) -> Result<Vec<Vec<Option<u64>>>>;
}

/// Split a `sources × targets` matrix into `≤max_points`-per-side blocks, fetch each via the
/// provider's `matrix_block`, and stitch them back into the full grid — keeping every request
/// within the provider's per-request location cap. Up to `concurrency` blocks are fetched in
/// parallel.
pub async fn tile_matrix(
    provider: &(impl MatrixBlock + ?Sized),
    sources: &[Location],
    targets: &[Location],
    max_points: usize,
    concurrency: usize,
) -> Result<Vec<Vec<Option<u64>>>> {
    let mut out = vec![vec![None; targets.len()]; sources.len()];

    let s_chunks = sources.len().div_ceil(max_points);
    let t_chunks = targets.len().div_ceil(max_points);
    let total_blocks = s_chunks * t_chunks;

    if total_blocks == 0 {
        return Ok(out);
    }

    // Collect work items as owned chunks for the concurrent futures.
    struct Block {
        s_off: usize,
        t_off: usize,
        s_locs: Vec<Location>,
        t_locs: Vec<Location>,
    }

    let mut blocks: Vec<Block> = Vec::with_capacity(total_blocks);
    for (sb, s_chunk) in sources.chunks(max_points).enumerate() {
        for (tb, t_chunk) in targets.chunks(max_points).enumerate() {
            blocks.push(Block {
                s_off: sb * max_points,
                t_off: tb * max_points,
                s_locs: s_chunk.to_vec(),
                t_locs: t_chunk.to_vec(),
            });
        }
    }

    let sem = Semaphore::new(concurrency);
    let sem_ref = &sem;
    let mut stream: FuturesUnordered<_> = FuturesUnordered::new();

    for b in blocks {
        let s_off = b.s_off;
        let t_off = b.t_off;
        let s_locs = b.s_locs;
        let t_locs = b.t_locs;
        stream.push(async move {
            let _permit = sem_ref.acquire().await.expect("semaphore closed");
            let mut result = vec![vec![None; t_locs.len()]; s_locs.len()];

            match provider.matrix_block(&s_locs, &t_locs).await {
                Ok(block) => {
                    for (r, row) in block.into_iter().enumerate() {
                        for (c, cell) in row.into_iter().enumerate() {
                            result[r][c] = cell;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        s_off,
                        t_off,
                        s_len = s_locs.len(),
                        t_len = t_locs.len(),
                        error = %e,
                        "matrix block request failed; falling back to per-source requests"
                    );
                    for (sr, s_loc) in s_locs.iter().enumerate() {
                        let single = [s_loc.clone()];
                        match provider.matrix_block(&single, &t_locs).await {
                            Ok(row_block) => {
                                for (c, cell) in row_block[0].iter().enumerate() {
                                    result[sr][c] = *cell;
                                }
                            }
                            Err(inner) => {
                                tracing::warn!(
                                    location = %s_loc.name,
                                    lat = s_loc.latitude,
                                    lon = s_loc.longitude,
                                    error = %inner,
                                    "location cannot be mapped to road network; all pairs from this source marked unroutable"
                                );
                            }
                        }
                    }
                }
            }

            (s_off, t_off, result)
        });
    }

    let log_pct_step = 5u32;
    let mut next_log_pct = log_pct_step;
    let start = Instant::now();
    let mut blocks_done = 0u32;

    while let Some((s_off, t_off, block)) = stream.next().await {
        for (r, row) in block.into_iter().enumerate() {
            for (c, cell) in row.into_iter().enumerate() {
                out[s_off + r][t_off + c] = cell;
            }
        }

        blocks_done += 1;
        let pct = blocks_done * 100 / total_blocks as u32;
        if pct >= next_log_pct {
            let elapsed = start.elapsed();
            let avg = elapsed / blocks_done;
            let eta = avg * (total_blocks as u32 - blocks_done);
            let eta_secs = eta.as_secs();
            tracing::info!(
                progress = pct,
                blocks = blocks_done,
                total = total_blocks,
                elapsed_ms = elapsed.as_millis() as u64,
                eta_secs,
                "matrix progress: {pct}% ({blocks_done}/{total_blocks} blocks, \
                 {elapsed:.1?} elapsed, ~{eta_secs}s remaining)",
                elapsed = elapsed,
            );
            while next_log_pct <= pct {
                next_log_pct += log_pct_step;
            }
        }
    }

    let total = start.elapsed();
    tracing::info!(
        blocks = total_blocks,
        elapsed_ms = total.as_millis() as u64,
        "matrix complete: {total_blocks} blocks in {total:.1?}",
        total = total,
    );

    Ok(out)
}

/// Per-pair cache key — same scheme the single-route path uses.
pub fn pair_key(from: &Location, to: &Location) -> String {
    from.to_key() + "-" + &to.to_key()
}

/// 1-week TTL with ±10% jitter, matching the single-route cache.
pub fn week_ttl() -> StdDuration {
    let jitter: f32 = rand::rng().random_range(0.9..1.1);
    StdDuration::from_hours((24f32 * 7f32 * jitter) as u64)
}

pub fn finalize(seconds: &[Vec<Option<u64>>]) -> Vec<Vec<Duration>> {
    seconds
        .iter()
        .map(|row| {
            row.iter()
                .map(|c| Duration::seconds(c.unwrap_or(0) as i64))
                .collect()
        })
        .collect()
}

/// Read whatever is already cached into a partial matrix (diagonal = 0, cached cells filled,
/// the rest `None`) and collect the off-diagonal pairs that are still missing. Uses a single
/// batch query rather than N² individual round-trips.
pub async fn assemble_from_cache(
    cache: &PersistentCache,
    locations: &[Location],
) -> Result<(Vec<Vec<Option<u64>>>, Vec<(usize, usize)>)> {
    let n = locations.len();
    let mut secs = vec![vec![None; n]; n];
    let mut missing = Vec::new();

    // First pass: collect every pair key so we can fetch them all at once.
    // Store (i, j, key) tuples so we don't recompute keys in the second pass.
    struct Pair {
        i: usize,
        j: usize,
        key: String,
    }
    let mut pairs: Vec<Pair> = Vec::with_capacity(n * n);
    for i in 0..n {
        for j in 0..n {
            if i == j {
                secs[i][j] = Some(0);
                continue;
            }
            pairs.push(Pair {
                i,
                j,
                key: pair_key(&locations[i], &locations[j]),
            });
        }
    }

    // Single batch query: PostgreSQL returns every cached entry in one round-trip.
    let all_keys: Vec<String> = pairs.iter().map(|p| p.key.clone()).collect();
    let cached = cache.get_batch(&all_keys).await?;

    // Second pass: look up each pair in the in-memory map.
    for p in pairs {
        match cached
            .get(&p.key)
            .and_then(|json| serde_json::from_str::<u64>(json).ok())
        {
            Some(s) => secs[p.i][p.j] = Some(s),
            None => missing.push((p.i, p.j)),
        }
    }

    Ok((secs, missing))
}

/// What to fetch from the provider to fill the missing pairs.
pub enum FetchPlan {
    /// Nothing missing — the cache had every pair.
    None,
    /// Mostly cold: one full `locations × locations` request is cheapest.
    Full,
    /// A few new/stale locations: fetch `cover × all` and `all × cover` rectangular blocks.
    Incremental { cover: Vec<usize> },
}

pub fn plan_fetch(missing: &[(usize, usize)], n: usize) -> FetchPlan {
    if missing.is_empty() {
        return FetchPlan::None;
    }
    let cover = cover_locations(missing, n);
    // Two rectangular blocks cost ~2·|cover|·n cells; a full square costs n·n. Once the
    // cover reaches ~n/2, the full square is cheaper (and simpler), so prefer it.
    if cover.len() * 2 >= n {
        FetchPlan::Full
    } else {
        FetchPlan::Incremental { cover }
    }
}

/// Greedy vertex cover of the "missing pairs" graph: repeatedly take the location touching
/// the most still-uncovered missing pairs. Every missing pair then has an endpoint in the
/// cover, so `cover × all` ∪ `all × cover` covers them all. For a handful of new locations
/// this returns exactly those (they have the highest missing degree).
pub fn cover_locations(missing: &[(usize, usize)], n: usize) -> Vec<usize> {
    let mut remaining: Vec<(usize, usize)> = missing.to_vec();
    let mut cover = Vec::new();
    while !remaining.is_empty() {
        let mut deg = vec![0usize; n];
        for &(a, b) in &remaining {
            deg[a] += 1;
            deg[b] += 1;
        }
        let v = (0..n).max_by_key(|&i| deg[i]).unwrap();
        cover.push(v);
        remaining.retain(|&(a, b)| a != v && b != v);
    }
    cover
}

/// Fill missing cells from the two rectangular blocks.
/// `block_cover_all[p][j]` = drive from `cover[p]` to `j`; `block_all_cover[i][p]` = `i` to `cover[p]`.
pub fn fill_from_blocks(
    secs: &mut [Vec<Option<u64>>],
    missing: &[(usize, usize)],
    cover: &[usize],
    block_cover_all: &[Vec<Option<u64>>],
    block_all_cover: &[Vec<Option<u64>>],
) {
    let pos: HashMap<usize, usize> = cover.iter().enumerate().map(|(p, &v)| (v, p)).collect();
    for &(i, j) in missing {
        if let Some(&p) = pos.get(&i) {
            secs[i][j] = block_cover_all[p][j];
        } else if let Some(&p) = pos.get(&j) {
            secs[i][j] = block_all_cover[i][p];
        }
    }
}

/// Any pair the provider returned null for (genuinely unroutable) is filled with a single
/// routed call — keeps the returned matrix free of holes without sentinel values.
pub async fn fill_unroutable(
    provider: &impl RoutingProvider,
    locations: &[Location],
    secs: &mut [Vec<Option<u64>>],
    missing: &[(usize, usize)],
) -> Result<()> {
    for &(i, j) in missing {
        if secs[i][j].is_none() {
            match provider.get_travel_time(&locations[i], &locations[j]).await {
                Ok(d) => {
                    secs[i][j] = Some(d.num_seconds().max(0) as u64);
                }
                Err(e) => {
                    tracing::warn!(
                        from = %locations[i].name,
                        to = %locations[j].name,
                        error = %e,
                        "individual route lookup failed; marking pair as unroutable"
                    );
                }
            }
        }
    }
    Ok(())
}

/// Write only the given (previously-missing) pairs into the per-pair cache.
pub async fn cache_pairs(
    cache: &PersistentCache,
    locations: &[Location],
    pairs: &[(usize, usize)],
    seconds: &[Vec<Option<u64>>],
) -> Result<()> {
    for &(i, j) in pairs {
        if let Some(s) = seconds[i][j] {
            cache
                .put(&pair_key(&locations[i], &locations[j]), s, week_ttl())
                .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes each location's global index in its latitude, and returns a block whose cells are
    /// `source_index * 100 + target_index` — so a correctly-stitched full grid reads `i*100 + j`.
    struct IndexEncodingProvider;

    #[async_trait]
    impl MatrixBlock for IndexEncodingProvider {
        async fn matrix_block(
            &self,
            sources: &[Location],
            targets: &[Location],
        ) -> Result<Vec<Vec<Option<u64>>>> {
            Ok(sources
                .iter()
                .map(|s| {
                    targets
                        .iter()
                        .map(|t| Some(s.latitude as u64 * 100 + t.latitude as u64))
                        .collect()
                })
                .collect())
        }
    }

    #[tokio::test]
    async fn tile_matrix_stitches_blocks_by_offset() {
        // 5 locations, chunk size 2 → 3×3 blocks of uneven size (2,2,1); indices survive stitching.
        let locs: Vec<Location> = (0..5)
            .map(|i| Location::new(i as f64, 0.0, format!("l{i}"), "DE".into()))
            .collect();
        let out = tile_matrix(&IndexEncodingProvider, &locs, &locs, 2, 3)
            .await
            .unwrap();
        assert_eq!(out.len(), 5);
        for i in 0..5 {
            assert_eq!(out[i].len(), 5);
            for j in 0..5 {
                assert_eq!(
                    out[i][j],
                    Some(i as u64 * 100 + j as u64),
                    "cell [{i}][{j}]"
                );
            }
        }
    }

    #[test]
    fn cover_picks_the_single_new_location() {
        // 4 locations, index 3 is new: every (i,3) and (3,j) pair is missing.
        let n = 4;
        let mut missing = Vec::new();
        for k in 0..3 {
            missing.push((3, k));
            missing.push((k, 3));
        }
        let cover = cover_locations(&missing, n);
        assert_eq!(cover, vec![3]);
    }

    #[test]
    fn plan_full_when_mostly_cold() {
        // All off-diagonal pairs of a 4-node set missing → cover is large → Full.
        let n = 4;
        let mut missing = Vec::new();
        for i in 0..n {
            for j in 0..n {
                if i != j {
                    missing.push((i, j));
                }
            }
        }
        assert!(matches!(plan_fetch(&missing, n), FetchPlan::Full));
    }

    #[test]
    fn fill_uses_row_then_column_block() {
        // n=3, cover={2}. Missing (0,2) comes from all→cover; (2,1) from cover→all.
        let mut secs = vec![
            vec![Some(0), Some(10), None],
            vec![Some(20), Some(0), Some(30)],
            vec![None, None, Some(0)],
        ];
        let missing = vec![(0, 2), (2, 0), (2, 1)];
        let cover = vec![2usize];
        let block_cover_all = vec![vec![Some(70), Some(80), Some(0)]]; // cover[0]=2 → j
        let block_all_cover = vec![vec![Some(60)], vec![Some(50)], vec![Some(0)]]; // i → cover[0]=2
        fill_from_blocks(
            &mut secs,
            &missing,
            &cover,
            &block_cover_all,
            &block_all_cover,
        );
        assert_eq!(secs[0][2], Some(60)); // all→cover
        assert_eq!(secs[2][0], Some(70)); // cover→all
        assert_eq!(secs[2][1], Some(80)); // cover→all
    }
}
