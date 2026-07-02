//! Shared placement primitives used by every `WeekSolver`: the drive-time matrix,
//! its up-front fetch, the itinerary drive total, and availability/commitment
//! partitioning into segments (§3.2, §7 of docs/genetic-planner-design.md).

use std::collections::{BTreeMap, HashMap};

use anyhow::Result;
use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};

use crate::domain::{
    activities::ScheduledActivity,
    location::Location,
    ports::{RoutingProvider, SolverInput},
    weather::overnight_deadline,
};

/// Pairwise drive times over the deduped set of every location a solver can query
/// (origin ∪ candidate locations ∪ located commitments), fetched once per solve so
/// placement is pure lookups.
pub struct DriveMatrix {
    index: HashMap<String, usize>,
    times: Vec<Vec<Duration>>,
}

impl DriveMatrix {
    pub fn get(&self, from: &Location, to: &Location) -> Duration {
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

/// Dedup origin + candidate locations + located commitments by `to_key()` and fetch the
/// full matrix once. Commitments must be included — otherwise `DriveMatrix::get` silently
/// returns zero drive to a meeting (§7).
pub async fn build_matrix(
    routing: &dyn RoutingProvider,
    input: &SolverInput,
) -> Result<DriveMatrix> {
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut locs: Vec<Location> = Vec::new();
    let commitment_locs = input.fixed.iter().filter_map(|f| f.location.as_ref());
    for loc in std::iter::once(&input.origin)
        .chain(input.candidates.iter().map(|c| &c.location))
        .chain(commitment_locs)
    {
        index.entry(loc.to_key()).or_insert_with(|| {
            locs.push(loc.clone());
            locs.len() - 1
        });
    }

    let times = if locs.len() > 1 {
        routing.travel_time_matrix(&locs).await?
    } else {
        vec![vec![Duration::zero(); locs.len()]; locs.len()]
    };
    Ok(DriveMatrix { index, times })
}

/// Total drive summed as one `home → items → home` round trip **per calendar day** — you sleep at
/// home each night, so days don't chain into each other. Items are grouped by the date of their
/// start; within a day they must already be time-ordered (callers sort the chain). Online
/// commitments (`None`) add no drive.
///
/// ponytail: overnight location is home today. When campable spots land, this must take the
/// per-night overnight locations and use them as each day's return/depart point instead of `home`.
pub fn compute_total_drive(
    matrix: &DriveMatrix,
    items: &[ScheduledActivity],
    home: &Location,
) -> Duration {
    let mut by_day: BTreeMap<NaiveDate, Vec<&Location>> = BTreeMap::new();
    for a in items {
        if let Some(loc) = &a.location {
            by_day.entry(a.start.date_naive()).or_default().push(loc);
        }
    }
    let mut total = Duration::zero();
    for (_day, locs) in by_day {
        let mut prev: &Location = home;
        for loc in locs {
            total += matrix.get(prev, loc);
            prev = loc;
        }
        total += matrix.get(prev, home);
    }
    total
}

/// A contiguous placeable block bounded by availability edges and/or fixed commitments (§3.2).
#[derive(Debug, Clone)]
pub struct Segment {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// Location at segment start. `None` = carried from the previous segment (start of horizon
    /// or after an availability gap → the decoder resolves the first carry to home; an online
    /// commitment leaves it carried).
    pub start_loc: Option<Location>,
    /// Location required at segment end. `None` = carried (end of horizon / gap → home; online
    /// commitment) or, when `night_end`, filled by the decoder from the chosen overnight spot.
    pub end_loc: Option<Location>,
    /// True when a night follows this segment: the decoder resolves `end_loc` (and the next
    /// segment's start) to the overnight spot chosen by the genome, and the segment's `end` is
    /// already capped at that day's sunset − 1h arrival deadline.
    pub night_end: bool,
}

/// Partition placeable time into segments: each free slot, cut at every fixed commitment that
/// overlaps it, then cut again at every day boundary. A located commitment (`Some`) pins the
/// boundary location; a day boundary is a `night_end` whose overnight location the decoder fills
/// from the genome. Each night-ending piece ends at that day's sunset − 1h (`home` sunset today).
pub fn partition_segments(
    free_slots: &[crate::domain::activities::TimeWindow],
    fixed: &[ScheduledActivity],
    home: &Location,
) -> Vec<Segment> {
    let mut segments = Vec::new();
    for fs in free_slots {
        // Non-strict: the planner subtracts events from the free slots, so a commitment never
        // overlaps a slot — it *touches* the slot edge. Touching must still pin the boundary
        // (drive-out reservation to the meeting, and departing from it afterwards).
        let mut overlapping: Vec<&ScheduledActivity> = fixed
            .iter()
            .filter(|c| c.start <= fs.end && c.end >= fs.start)
            .collect();
        overlapping.sort_by_key(|c| c.start);

        let mut cursor = fs.start;
        let mut start_loc: Option<Location> = None;
        for c in overlapping {
            if cursor < c.start {
                segments.extend(split_by_day(cursor, c.start, start_loc.clone(), c.location.clone(), home));
            }
            cursor = c.end.max(cursor);
            start_loc = c.location.clone();
        }
        if cursor < fs.end {
            segments.extend(split_by_day(cursor, fs.end, start_loc, None, home));
        }
    }
    segments
}

/// Split one commitment-bounded piece `[start, end]` into per-day sub-segments. Every day but the
/// last ends at sunset − 1h and is marked `night_end` (a home/camp return follows); the last day
/// keeps the piece's real `end_loc`. Degenerate (evening) days with no daylight left are skipped.
fn split_by_day(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    start_loc: Option<Location>,
    end_loc: Option<Location>,
    home: &Location,
) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut day_start = start;
    loop {
        if end <= day_start {
            break;
        }
        let date = day_start.date_naive();
        // `start_loc` pins only the first emitted sub-segment (the piece's entry location); later
        // days start from the carried overnight location.
        let seg_start_loc = if out.is_empty() { start_loc.clone() } else { None };

        if end.date_naive() == date {
            out.push(Segment { start: day_start, end, start_loc: seg_start_loc, end_loc: end_loc.clone(), night_end: false });
            break;
        }

        let deadline = overnight_deadline(home, date);
        if day_start < deadline {
            out.push(Segment { start: day_start, end: deadline, start_loc: seg_start_loc, end_loc: None, night_end: true });
        }
        let next = date.succ_opt().expect("date within chrono range");
        day_start = next.and_time(NaiveTime::MIN).and_utc();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::activities::{ActivityKind, TimeWindow};
    use chrono::TimeZone;

    fn ts(h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 13, h, 0, 0).unwrap()
    }
    fn loc(name: &str) -> Location {
        Location::new(50.7, 13.0, name.into(), "DE".into())
    }
    fn commitment(start: u32, end: u32, location: Option<Location>) -> ScheduledActivity {
        ScheduledActivity {
            kind: ActivityKind::Commitment,
            location,
            start: ts(start),
            end: ts(end),
            title: "meeting".into(),
            description: String::new(),
            fun: 0.0,
        }
    }

    #[test]
    fn no_commitments_yields_one_segment_per_slot() {
        let slots = vec![TimeWindow { start: ts(8), end: ts(18) }];
        let segs = partition_segments(&slots, &[], &loc("Home"));
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].start, ts(8));
        assert_eq!(segs[0].end, ts(18));
        assert!(segs[0].start_loc.is_none() && segs[0].end_loc.is_none());
        assert!(!segs[0].night_end);
    }

    #[test]
    fn multi_day_slot_splits_per_day_with_night_boundaries() {
        // A 3-day slot with no commitments → three day-pieces; the first two end at a night.
        let slots = vec![TimeWindow { start: ts(8), end: ts(18) + Duration::days(2) }];
        let segs = partition_segments(&slots, &[], &loc("Home"));
        assert_eq!(segs.len(), 3, "one placeable piece per day");
        assert!(segs[0].night_end && segs[1].night_end, "days before the last end at a night");
        assert!(!segs[2].night_end, "the last day is not a night boundary");
        // Each night piece ends at that day's sunset − 1h, before the calendar-day rollover.
        assert!(segs[0].end < ts(8) + Duration::days(1));
    }

    #[test]
    fn located_commitment_splits_slot_and_pins_boundary() {
        let slots = vec![TimeWindow { start: ts(8), end: ts(18) }];
        let segs = partition_segments(&slots, &[commitment(12, 13, Some(loc("Office")))], &loc("Home"));
        assert_eq!(segs.len(), 2);
        // before: 8→12, ends at the office
        assert_eq!((segs[0].start, segs[0].end), (ts(8), ts(12)));
        assert!(segs[0].start_loc.is_none());
        assert_eq!(segs[0].end_loc.as_ref().unwrap().name, "Office");
        // after: 13→18, starts at the office
        assert_eq!((segs[1].start, segs[1].end), (ts(13), ts(18)));
        assert_eq!(segs[1].start_loc.as_ref().unwrap().name, "Office");
        assert!(segs[1].end_loc.is_none());
    }

    #[test]
    fn adjacent_commitment_pins_boundary_like_the_planner_produces() {
        // The planner subtracts events from free slots, so the commitment sits exactly in the
        // gap: [8,12] + meeting [12,13] + [13,18]. Touching must pin like overlapping does.
        let slots = vec![
            TimeWindow { start: ts(8), end: ts(12) },
            TimeWindow { start: ts(13), end: ts(18) },
        ];
        let segs =
            partition_segments(&slots, &[commitment(12, 13, Some(loc("Office")))], &loc("Home"));
        assert_eq!(segs.len(), 2);
        assert_eq!((segs[0].start, segs[0].end), (ts(8), ts(12)));
        assert_eq!(segs[0].end_loc.as_ref().unwrap().name, "Office");
        assert_eq!((segs[1].start, segs[1].end), (ts(13), ts(18)));
        assert_eq!(segs[1].start_loc.as_ref().unwrap().name, "Office");
    }

    #[test]
    fn online_commitment_splits_time_but_carries_location() {
        let slots = vec![TimeWindow { start: ts(8), end: ts(18) }];
        let segs = partition_segments(&slots, &[commitment(12, 13, None)], &loc("Home"));
        assert_eq!(segs.len(), 2);
        assert_eq!((segs[0].start, segs[0].end), (ts(8), ts(12)));
        assert_eq!((segs[1].start, segs[1].end), (ts(13), ts(18)));
        // online → both split boundaries carried (None)
        assert!(segs[0].end_loc.is_none());
        assert!(segs[1].start_loc.is_none());
    }
}
