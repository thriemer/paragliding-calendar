//! Feature-extraction contract for the preference-learning pipeline.
//!
//! Each activity type exposes a free-text `description` (fed to the sentence
//! embedder) and a vector of raw `(name, value)` features (log-transformed where
//! declared, but NOT z-score normalized — normalization is corpus-global and
//! happens in [`crate::domain::features`]).
//!
//! `ActivityKind` is deliberately NOT on this trait: the batch pipeline knows a
//! candidate's kind statically at collection time (a `Tour` maps via
//! [`kind_from_category`], a `Happening` is always `Event`, a `ParaglidingSite`
//! always `Paragliding`), so a runtime discriminator method would be redundant —
//! and tours whose category doesn't map are filtered out at collection rather
//! than forced through a fallible `kind()`.

use crate::domain::{
    happening::Happening,
    paragliding::{ParaglidingLaunch, ParaglidingSite, SiteType},
    tour::Tour,
};

pub trait ActivityEmbedding {
    /// Stable identity used as the `activity_id` key (`tour.id` / `happening.id`
    /// / `site.name`).
    fn activity_id(&self) -> String;
    /// Free text for the sentence embedder. Owned because most types synthesize
    /// it rather than storing one.
    fn description(&self) -> String;
    /// Raw (transformed, un-normalized) features. May be empty.
    fn features(&self) -> Vec<(String, f64)>;
}

/// `ln` guarded against non-positive inputs: `ln(0)`/`ln(<0)` would be
/// `-inf`/`NaN` and poison PCA and z-score statistics. Anything below 1 maps to
/// 0, which is the sensible floor for the counts/lengths we log-transform.
fn safe_ln(x: f64) -> f64 {
    x.max(1.0).ln()
}

impl ActivityEmbedding for Tour {
    fn activity_id(&self) -> String {
        self.id.clone()
    }

    fn description(&self) -> String {
        self.description.clone()
    }

    fn features(&self) -> Vec<(String, f64)> {
        vec![
            ("landscape".into(), self.landscape as f64),
            ("experience".into(), self.experience as f64),
            ("difficulty".into(), self.difficulty as f64),
            ("stamina".into(), self.stamina as f64),
            ("duration_log".into(), safe_ln(self.duration_minutes as f64)),
            ("ascent_log".into(), safe_ln(self.ascent_meters as f64)),
            ("length_log".into(), safe_ln(self.length_meters as f64)),
        ]
    }
}

impl ActivityEmbedding for Happening {
    fn activity_id(&self) -> String {
        self.id.clone()
    }

    fn description(&self) -> String {
        // Prefer the long description, then short, then the title so the
        // embedding is never degenerate on an empty string.
        self.description_long
            .clone()
            .or_else(|| self.description_short.clone())
            .unwrap_or_else(|| self.title.clone())
    }

    fn features(&self) -> Vec<(String, f64)> {
        // No struct features — quality comes entirely from the description.
        vec![]
    }
}

impl ActivityEmbedding for ParaglidingSite {
    fn activity_id(&self) -> String {
        self.name.clone()
    }

    fn description(&self) -> String {
        synthesize_site_description(self)
    }

    fn features(&self) -> Vec<(String, f64)> {
        // Optional features are omitted when absent; the corpus normalizer
        // imputes them to a normalized 0 (the per-feature mean).
        let mut f = vec![("launch_count".into(), self.launches.len() as f64)];

        // Height difference = max launch elevation minus min landing elevation.
        // If no landings are recorded, estimate min landing as max / 2 — a
        // reasonable heuristic for sites with launch data but no mapped landing.
        if let Some(max_elev) = self
            .launches
            .iter()
            .map(|l| l.elevation)
            .fold(None, |acc: Option<f64>, e| {
                Some(acc.map_or(e, |a| a.max(e)))
            })
        {
            let min_landing = self
                .landings
                .iter()
                .map(|l| l.elevation)
                .fold(None, |acc: Option<f64>, e| {
                    Some(acc.map_or(e, |a| a.min(e)))
                });
            let pseudo = max_elev / 2.0;
            let height_diff = max_elev - min_landing.unwrap_or(pseudo);
            f.push(("height_difference_log".into(), safe_ln(height_diff)));
        }

        // Fraction of the compass circle covered by the union of all launch
        // direction arcs.
        if !self.launches.is_empty() {
            f.push(("direction_coverage".into(), direction_coverage(&self.launches)));
        }

        // Binary: does this site have any winch launches?
        f.push((
            "has_winch".into(),
            if self
                .launches
                .iter()
                .any(|l| matches!(l.site_type, SiteType::Winch))
            {
                1.0
            } else {
                0.0
            },
        ));

        f
    }
}

/// Paragliding sites carry no free text, so build a deterministic German
/// description from structured fields. Deterministic on purpose: re-embedding
/// the same site must reproduce the same string.
fn synthesize_site_description(site: &ParaglidingSite) -> String {
    let mut parts: Vec<String> = Vec::new();

    if site.launches.is_empty() {
        parts.push("Gleitschirm-Fluggebiet ohne erfasste Startplätze.".into());
    } else {
        let n = site.launches.len();
        let has_hang = site
            .launches
            .iter()
            .any(|l| matches!(l.site_type, SiteType::Hang));
        let has_winch = site
            .launches
            .iter()
            .any(|l| matches!(l.site_type, SiteType::Winch));
        let type_str = match (has_hang, has_winch) {
            (true, true) => "Hangstart und Windenschlepp",
            (false, true) => "Windenschlepp",
            _ => "Hangstart",
        };
        parts.push(format!(
            "Gleitschirm-Fluggebiet mit {n} Startplätzen ({type_str})."
        ));

        let (min_e, max_e) = site
            .launches
            .iter()
            .map(|l| l.elevation)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), e| {
                (lo.min(e), hi.max(e))
            });
        if (max_e - min_e).abs() < 1.0 {
            parts.push(format!("Starthöhe {} m.", min_e.round() as i64));
        } else {
            parts.push(format!(
                "Starthöhe {}–{} m.",
                min_e.round() as i64,
                max_e.round() as i64
            ));
        }

        let mut dirs: Vec<&'static str> = Vec::new();
        for l in &site.launches {
            for d in direction_to_compass_de(l.direction_degrees_start, l.direction_degrees_stop) {
                if !dirs.contains(&d) {
                    dirs.push(d);
                }
            }
        }
        if !dirs.is_empty() {
            parts.push(format!("Startrichtungen: {}.", dirs.join(", ")));
        }
    }

    if let Some(country) = &site.country {
        parts.push(format!("Land: {country}."));
    }

    parts.join(" ")
}

/// Fraction of the 360° compass circle covered by the union of all launch
/// direction arcs. Each arc is clockwise from `direction_degrees_start` to
/// `direction_degrees_stop`. Arcs that wrap through 0° (e.g. 350°→20°) are
/// split, then overlapping intervals are merged before summing.
fn direction_coverage(launches: &[ParaglidingLaunch]) -> f64 {
    let norm = |d: f64| ((d % 360.0) + 360.0) % 360.0;

    let mut intervals: Vec<(f64, f64)> = Vec::new();
    for l in launches {
        let start = norm(l.direction_degrees_start);
        let stop = norm(l.direction_degrees_stop);
        if (start - stop).abs() < 1e-9 {
            continue; // zero-width arc contributes nothing
        }
        if start < stop {
            intervals.push((start, stop));
        } else {
            // Wrap-around: split into [start, 360) and [0, stop)
            intervals.push((start, 360.0));
            intervals.push((0.0, stop));
        }
    }

    if intervals.is_empty() {
        return 0.0;
    }

    intervals.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (s, e) in intervals {
        if let Some(last) = merged.last_mut() {
            if s <= last.1 + 1e-9 {
                last.1 = last.1.max(e);
                continue;
            }
        }
        merged.push((s, e));
    }

    let covered: f64 = merged.iter().map(|(s, e)| e - s).sum();
    (covered / 360.0).min(1.0)
}

/// The eight German compass sectors covered by the clockwise arc `start..stop`
/// (degrees). Sector centers: Nord=0, Nordost=45, … Nordwest=315. Handles
/// wrap-around (e.g. 350→020 spans Nord). Returned in fixed compass order.
fn direction_to_compass_de(start_deg: f64, stop_deg: f64) -> Vec<&'static str> {
    const SECTORS: [(f64, &str); 8] = [
        (0.0, "Nord"),
        (45.0, "Nordost"),
        (90.0, "Ost"),
        (135.0, "Südost"),
        (180.0, "Süd"),
        (225.0, "Südwest"),
        (270.0, "West"),
        (315.0, "Nordwest"),
    ];
    let norm = |d: f64| ((d % 360.0) + 360.0) % 360.0;
    let width = norm(stop_deg - start_deg);
    SECTORS
        .iter()
        .filter(|(center, _)| norm(center - start_deg) <= width + 1e-9)
        .map(|(_, name)| *name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{location::Location, paragliding::ParaglidingLaunch};

    fn loc() -> Location {
        Location::new(47.0, 10.0, "L".into(), "DE".into())
    }

    fn launch(site_type: SiteType, elevation: f64, start: f64, stop: f64) -> ParaglidingLaunch {
        ParaglidingLaunch {
            site_type,
            location: loc(),
            direction_degrees_start: start,
            direction_degrees_stop: stop,
            elevation,
        }
    }

    fn tour() -> Tour {
        Tour {
            id: "tour_1".into(),
            title: "T".into(),
            category: "Wanderung".into(),
            location: loc(),
            description: "Eine schöne Tour".into(),
            duration_minutes: 0,
            length_meters: 0,
            ascent_meters: 0,
            descent_meters: 0,
            difficulty: 2,
            stamina: 3,
            landscape: 5,
            experience: 4,
            is_loop: false,
            season_bitmask: 0,
            source_url: String::new(),
            image_urls: vec![],
            raw_json: String::new(),
        }
    }

    fn site() -> ParaglidingSite {
        ParaglidingSite {
            name: "Testberg".into(),
            launches: vec![
                launch(SiteType::Hang, 1200.0, 135.0, 225.0),
                launch(SiteType::Hang, 900.0, 180.0, 200.0),
            ],
            landings: vec![],
            country: Some("Deutschland".into()),
            data_source: "dhv".into(),
            parking_location: None,
            mute_alerts: None,
            rating: Some(4),
            preferred_weather_model: None,
        }
    }

    #[test]
    fn tour_features_are_named_and_finite_even_for_zero_inputs() {
        let f = tour().features();
        // Zero duration/ascent/length must not produce -inf/NaN.
        for (name, v) in &f {
            assert!(v.is_finite(), "{name} = {v} not finite");
        }
        let map: std::collections::HashMap<_, _> = f.into_iter().collect();
        assert_eq!(map["landscape"], 5.0);
        assert_eq!(map["experience"], 4.0);
        assert_eq!(map["duration_log"], 0.0); // ln(max(0,1)) = 0
        assert_eq!(map["ascent_log"], 0.0);
        assert_eq!(map["length_log"], 0.0);
    }

    #[test]
    fn happening_description_falls_back_long_short_title() {
        let mut h = Happening {
            id: "e1".into(),
            title: "Titel".into(),
            location: None,
            category_id: None,
            category_title: None,
            category_keys: vec![],
            description_short: Some("kurz".into()),
            description_long: Some("lang".into()),
            homepage: None,
            address: None,
            organizer: None,
            schedule_rules: None,
            dates: vec![],
            source_url: String::new(),
            image_urls: vec![],
            data: serde_json::Value::Null,
        };
        assert_eq!(h.description(), "lang");
        h.description_long = None;
        assert_eq!(h.description(), "kurz");
        h.description_short = None;
        assert_eq!(h.description(), "Titel");
        assert!(h.features().is_empty());
    }

    #[test]
    fn site_description_is_deterministic_german() {
        let d = site().description();
        assert_eq!(
            d,
            "Gleitschirm-Fluggebiet mit 2 Startplätzen (Hangstart). Starthöhe 900–1200 m. \
             Startrichtungen: Südost, Süd, Südwest. Land: Deutschland."
        );
    }

    #[test]
    fn site_description_omits_missing_country() {
        let mut s = site();
        s.country = None;
        let d = s.description();
        assert!(!d.contains("Land"));
    }

    #[test]
    fn site_features_include_all_paragliding_features() {
        let names: Vec<String> = site().features().into_iter().map(|(n, _)| n).collect();
        assert!(names.contains(&"launch_count".to_string()));
        assert!(names.contains(&"height_difference_log".to_string()));
        assert!(names.contains(&"direction_coverage".to_string()));
        assert!(names.contains(&"has_winch".to_string()));
    }

    #[test]
    fn compass_handles_range_and_wraparound() {
        assert_eq!(direction_to_compass_de(180.0, 180.0), vec!["Süd"]);
        assert_eq!(
            direction_to_compass_de(135.0, 225.0),
            vec!["Südost", "Süd", "Südwest"]
        );
        // Wrap-around across north.
        assert_eq!(direction_to_compass_de(350.0, 20.0), vec!["Nord"]);
    }
}
