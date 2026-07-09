#![allow(dead_code)] // ponytail: routing stack not wired into AppState yet (CrowFlies stands in); kept per owner's call.

//! Shared helpers for provider matrix endpoints: per-pair cache key/TTL, cache-first
//! assembly, and incremental fetch planning so only the new pairs hit the provider.

use std::collections::HashMap;
use std::time::Duration as StdDuration;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Duration;
use rand::RngExt;

use crate::{
    adapters::cache::PersistentCache,
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
/// within the provider's per-request location cap.
pub async fn tile_matrix(
    provider: &(impl MatrixBlock + ?Sized),
    sources: &[Location],
    targets: &[Location],
    max_points: usize,
) -> Result<Vec<Vec<Option<u64>>>> {
    let mut out = vec![vec![None; targets.len()]; sources.len()];
    for (sb, s_chunk) in sources.chunks(max_points).enumerate() {
        for (tb, t_chunk) in targets.chunks(max_points).enumerate() {
            let block = provider.matrix_block(s_chunk, t_chunk).await?;
            let (s_off, t_off) = (sb * max_points, tb * max_points);
            for (r, row) in block.into_iter().enumerate() {
                for (c, cell) in row.into_iter().enumerate() {
                    out[s_off + r][t_off + c] = cell;
                }
            }
        }
    }
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
        .map(|row| row.iter().map(|c| Duration::seconds(c.unwrap_or(0) as i64)).collect())
        .collect()
}

/// Read whatever is already cached into a partial matrix (diagonal = 0, cached cells filled,
/// the rest `None`) and collect the off-diagonal pairs that are still missing.
pub async fn assemble_from_cache(
    cache: &PersistentCache,
    locations: &[Location],
) -> Result<(Vec<Vec<Option<u64>>>, Vec<(usize, usize)>)> {
    let n = locations.len();
    let mut secs = vec![vec![None; n]; n];
    let mut missing = Vec::new();
    for i in 0..n {
        for j in 0..n {
            if i == j {
                secs[i][j] = Some(0);
                continue;
            }
            match cache.get::<u64>(&pair_key(&locations[i], &locations[j])).await? {
                Some(s) => secs[i][j] = Some(s),
                None => missing.push((i, j)),
            }
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
            secs[i][j] = Some(
                provider
                    .get_travel_time(&locations[i], &locations[j])
                    .await?
                    .num_seconds()
                    .max(0) as u64,
            );
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
            cache.put(&pair_key(&locations[i], &locations[j]), s, week_ttl()).await?;
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
        let locs: Vec<Location> =
            (0..5).map(|i| Location::new(i as f64, 0.0, format!("l{i}"), "DE".into())).collect();
        let out = tile_matrix(&IndexEncodingProvider, &locs, &locs, 2).await.unwrap();
        assert_eq!(out.len(), 5);
        for i in 0..5 {
            assert_eq!(out[i].len(), 5);
            for j in 0..5 {
                assert_eq!(out[i][j], Some(i as u64 * 100 + j as u64), "cell [{i}][{j}]");
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
        let mut secs = vec![vec![Some(0), Some(10), None], vec![Some(20), Some(0), Some(30)], vec![None, None, Some(0)]];
        let missing = vec![(0, 2), (2, 0), (2, 1)];
        let cover = vec![2usize];
        let block_cover_all = vec![vec![Some(70), Some(80), Some(0)]]; // cover[0]=2 → j
        let block_all_cover = vec![vec![Some(60)], vec![Some(50)], vec![Some(0)]]; // i → cover[0]=2
        fill_from_blocks(&mut secs, &missing, &cover, &block_cover_all, &block_all_cover);
        assert_eq!(secs[0][2], Some(60)); // all→cover
        assert_eq!(secs[2][0], Some(70)); // cover→all
        assert_eq!(secs[2][1], Some(80)); // cover→all
    }
}
