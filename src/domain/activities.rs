use chrono::{DateTime, Duration, Utc};

use crate::domain::location::Location;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    Paragliding,
    Hiking,
    Biking,
    Running,
    MountainClimbing,
    Kayaking,
    /// A generic dated event (festival, theatre, kids' activity, …) — sourced from outdoor-active
    /// but not itself an outdoor/weather-bound activity.
    Event,
    /// A fixed calendar commitment (meeting, appointment) the planner schedules around.
    Commitment,
}

/// Maps the outdoor-active German category title to an `ActivityKind`. Unmapped / out-of-scope
/// categories (winter sports, motorised, equestrian, skating) return `None` and are skipped.
/// Substring match on the lowercased title so minor title variants still land.
pub fn kind_from_category(category: &str) -> Option<ActivityKind> {
    let c = category.to_lowercase();
    let has = |needle: &str| c.contains(needle);

    // Order matters: check biking/running before generic "wander" fallthrough isn't needed since
    // categories are disjoint, but keep the more specific keywords first regardless.
    if has("mountainbike") || has("radtour") || has("radweg") || has("rennrad") || has("gravel") {
        Some(ActivityKind::Biking)
    } else if has("trailrunning") || has("jogging") {
        Some(ActivityKind::Running)
    } else if has("bergtour") || has("klettersteig") || has("hochtour") || has("alpinklettern") {
        Some(ActivityKind::MountainClimbing)
    } else if has("kanu") || has("kajak") || has("paddel") {
        Some(ActivityKind::Kayaking)
    } else if has("wander")        // Wanderung, Winterwandern, Fernwanderweg
        || has("themenweg")
        || has("pilgerweg")
        || has("stadtrundgang")
        || has("schneeschuh")
        || has("nordic walking")
    {
        Some(ActivityKind::Hiking)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TimeWindow {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl TimeWindow {
    pub fn duration(&self) -> Duration {
        self.end - self.start
    }
}

#[derive(Debug, Clone)]
pub enum Timing {
    // ponytail: no source emits Fixed yet (all suggestions are weather windows); the solvers
    // already place it — the seam for fixed-time activities like competitions or bookings.
    #[allow(dead_code)]
    Fixed {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    },
    Flexible {
        window: TimeWindow,
        min_duration: Duration,
    },
    /// An activity that takes exactly `duration` — it is placed at the earliest feasible
    /// start within `window` (after Wait genes and drive-in), not stretched or shrunk.
    ExactDuration {
        window: TimeWindow,
        duration: Duration,
    },
}

impl Timing {
    /// The time window the activity can occupy — used to check whether a candidate can occur in a
    /// given placement segment before the solver ever tries to schedule it.
    pub fn window(&self) -> TimeWindow {
        match self {
            Timing::Fixed { start, end } => TimeWindow { start: *start, end: *end },
            Timing::Flexible { window, .. } | Timing::ExactDuration { window, .. } => *window,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Score {
    /// Clock time of the first hourly bucket (`hourly[0]` covers
    /// `[window_start, window_start + 1h)`).
    pub window_start: DateTime<Utc>,
    /// Score per clock hour, index 0 = the window's first hour.
    pub hourly: Vec<f32>,
    pub reasons: Vec<String>,
}

impl Score {
    #[allow(dead_code)]
    /// Total score = Σ hourly. Used for ranking.
    pub fn total(&self) -> f32 {
        self.hourly.iter().sum()
    }

    /// Fun earned by occupying `[start, end]`, summing the covered hourly
    /// buckets and pro-rating partial hours. Bucket `i` covers
    /// `[window_start + i h, window_start + (i+1) h)`, keyed off the window
    /// origin the `Score` carries — callers pass only the placed span.
    pub fn fun_between(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> f32 {
        let mut fun = 0.0;
        for (i, &s) in self.hourly.iter().enumerate() {
            let bucket_start = self.window_start + Duration::hours(i as i64);
            let bucket_end = bucket_start + Duration::hours(1);
            let lo = start.max(bucket_start);
            let hi = end.min(bucket_end);
            let overlap = (hi - lo).num_seconds().max(0) as f32 / 3600.0;
            fun += s * overlap;
        }
        fun
    }
}

#[derive(Debug, Clone)]
pub struct ActivitySuggestion {
    /// Stable identity of the underlying activity (`tour.id` / `event.id` / site name). One activity
    /// fans out into several per-day/per-window suggestions sharing this `id`; single-use dedup
    /// (`allow_multiple = false`) collapses them to one. Not consulted when `allow_multiple = true`.
    pub id: String,
    pub kind: ActivityKind,
    pub location: Location,
    pub timing: Timing,
    pub title: String,
    pub description: String,
    pub score: Option<Score>,
    /// If `false` (default), this activity may appear at most once per plan.
    /// Set to `true` for activities where revisiting the same candidate is legitimate
    /// (e.g. paragliding — spots are scarce, conditions windows distinct).
    pub allow_multiple: bool,
}

/// Where a day ends: the location you sleep at. Today only `Home`; a future version adds campable
/// spots (with facilities, cost, notes) as `Camp`, which is why the solver references these by
/// `Arc<OvernightSpot>` rather than a bare `Location` or an id — the struct is meant to grow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OvernightKind {
    Home,
    // ponytail: constructed once `overnight_candidates` sources campable spots; the whole
    // overnight-in-genome machinery is wired for it, only the data source is deferred.
    #[allow(dead_code)]
    Camp,
}

#[derive(Debug, Clone)]
pub struct OvernightSpot {
    pub location: Location,
    pub kind: OvernightKind,
}

impl OvernightSpot {
    pub fn home(location: Location) -> Self {
        Self { location, kind: OvernightKind::Home }
    }
}

#[derive(Debug, Clone)]
pub struct PlanningContext {
    pub home: Location,
    pub horizon: TimeWindow,
    pub conflict_calendars: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ScheduledActivity {
    pub kind: ActivityKind,
    /// `None` for online commitments (stationary, no drive); `Some` otherwise.
    pub location: Option<Location>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub title: String,
    pub description: String,
    pub fun: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub items: Vec<ScheduledActivity>,
    pub total_fun: f32,
    pub total_drive: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn time_window_duration_is_end_minus_start() {
        let start = Utc.with_ymd_and_hms(2026, 6, 13, 10, 0, 0).unwrap();
        let end = start + Duration::hours(3);
        let w = TimeWindow { start, end };
        assert_eq!(w.duration(), Duration::hours(3));
    }

    #[test]
    fn time_window_zero_duration_when_start_equals_end() {
        let t = Utc.with_ymd_and_hms(2026, 6, 13, 10, 0, 0).unwrap();
        let w = TimeWindow { start: t, end: t };
        assert_eq!(w.duration(), Duration::zero());
    }

    fn score(window_start: DateTime<Utc>, hourly: Vec<f32>) -> Score {
        Score {
            window_start,
            hourly,
            reasons: vec![],
        }
    }

    #[test]
    fn total_is_sum_of_hourly() {
        let ws = Utc.with_ymd_and_hms(2026, 6, 13, 10, 0, 0).unwrap();
        assert_eq!(score(ws, vec![1.0, 2.0, 3.0]).total(), 6.0);
    }

    #[test]
    fn fun_between_prorates_partial_hour() {
        let ws = Utc.with_ymd_and_hms(2026, 6, 13, 10, 0, 0).unwrap();
        let s = score(ws, vec![2.0, 2.0]);
        // Occupy the first 30 minutes of bucket 0 → half of 2.0.
        let fun = s.fun_between(ws, ws + Duration::minutes(30));
        assert!((fun - 1.0).abs() < 1e-6, "got {fun}");
    }

    #[test]
    fn splitting_a_span_is_fun_neutral() {
        // Locks in the invariant that lets the renderer coalesce fragments freely: two
        // back-to-back placements earn exactly what one covering the same span does — so the GA
        // gains nothing from splitting. If scoring ever rewards splitting, this fails loudly.
        let ws = Utc.with_ymd_and_hms(2026, 6, 13, 7, 0, 0).unwrap();
        let s = score(ws, vec![1.0, 2.0, 3.0, 4.0]); // hours 7,8,9,10
        let whole = s.fun_between(ws, ws + Duration::hours(4));
        let split = s.fun_between(ws, ws + Duration::hours(2))
            + s.fun_between(ws + Duration::hours(2), ws + Duration::hours(4));
        assert!((whole - split).abs() < 1e-6, "whole {whole} != split {split}");
    }

    #[test]
    fn category_mapping_covers_the_five_kinds() {
        assert_eq!(kind_from_category("Wanderung"), Some(ActivityKind::Hiking));
        assert_eq!(kind_from_category("Winterwandern"), Some(ActivityKind::Hiking));
        assert_eq!(kind_from_category("Mountainbike"), Some(ActivityKind::Biking));
        assert_eq!(kind_from_category("Radtour"), Some(ActivityKind::Biking));
        assert_eq!(kind_from_category("Trailrunning"), Some(ActivityKind::Running));
        assert_eq!(kind_from_category("Bergtour"), Some(ActivityKind::MountainClimbing));
        assert_eq!(kind_from_category("Kanu"), Some(ActivityKind::Kayaking));
        // Out of scope → skipped.
        assert_eq!(kind_from_category("Skitour"), None);
        assert_eq!(kind_from_category("Motorrad"), None);
        assert_eq!(kind_from_category("Reiten"), None);
    }

    #[test]
    fn fun_between_rewards_the_good_hours() {
        let ws = Utc.with_ymd_and_hms(2026, 6, 13, 10, 0, 0).unwrap();
        let s = score(ws, vec![0.0, 10.0]); // bad hour then good hour
        let bad = s.fun_between(ws, ws + Duration::hours(1));
        let good = s.fun_between(ws + Duration::hours(1), ws + Duration::hours(2));
        assert!(good > bad, "good {good} should beat bad {bad}");
        assert_eq!(bad, 0.0);
        assert_eq!(good, 10.0);
    }
}
