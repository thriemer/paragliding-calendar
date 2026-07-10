use anyhow::Result;
use serde::Deserialize;

use crate::domain::{hiking::OutdoorTour, location::Location};

pub fn parse_tour(json: &str) -> Result<OutdoorTour> {
    let resp: OAResponse = serde_json::from_str(json)?;
    let tour = resp
        .answer
        .contents
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty contents array"))?;

    if tour.point.len() < 2 {
        anyhow::bail!("tour {} has no point coordinates", tour.id);
    }
    let (lon, lat) = (tour.point[0], tour.point[1]);
    let location = Location::new(lat, lon, tour.title.clone(), String::new());

    let description = tour
        .texts
        .as_ref()
        .and_then(|t| t.short.clone())
        .unwrap_or_default();

    let duration_minutes = tour
        .metrics
        .duration
        .as_ref()
        .and_then(|d| d.minimal)
        .unwrap_or(0.0) as u32;

    let length_meters = tour.metrics.length.unwrap_or(0.0) as u32;

    let (ascent, descent) = match &tour.metrics.elevation {
        Some(e) => (e.ascent.unwrap_or(0) as u32, e.descent.unwrap_or(0) as u32),
        None => (0, 0),
    };

    let is_loop = tour
        .properties
        .iter()
        .any(|p| p.name == "loopTour");

    let season_bitmask = season_to_bitmask(&tour.season);

    Ok(OutdoorTour {
        id: tour.id,
        title: tour.title,
        category: tour.category.title,
        location,
        description,
        duration_minutes,
        length_meters,
        ascent_meters: ascent,
        descent_meters: descent,
        difficulty: tour.rating_info.difficulty as u8,
        stamina: tour.rating_info.stamina as u8,
        landscape: tour.rating_info.landscape as u8,
        experience: tour.rating_info.experience as u8,
        is_loop,
        season_bitmask,
        raw_json: json.to_string(),
    })
}

fn season_to_bitmask(season: &OASeason) -> u16 {
    let months = [
        &season.jan, &season.feb, &season.mar, &season.apr,
        &season.may, &season.jun, &season.jul, &season.aug,
        &season.sep, &season.oct, &season.nov, &season.dec,
    ];
    let mut mask: u16 = 0;
    for (i, val) in months.iter().enumerate() {
        if val.as_deref() == Some("yes") {
            mask |= 1 << i;
        }
    }
    mask
}

#[derive(Deserialize)]
struct OAResponse {
    answer: OAAnswer,
}

#[derive(Deserialize)]
struct OAAnswer {
    contents: Vec<OATour>,
}

#[derive(Deserialize)]
struct OATour {
    id: String,
    title: String,
    category: OACategory,
    point: Vec<f64>,
    #[serde(default)]
    texts: Option<OATexts>,
    #[serde(default)]
    metrics: OAMetrics,
    #[serde(default)]
    properties: Vec<OAProperty>,
    #[serde(default)]
    season: OASeason,
    #[serde(rename = "ratingInfo", default)]
    rating_info: OARatingInfo,
}

#[derive(Deserialize)]
struct OACategory {
    title: String,
}

#[derive(Deserialize)]
struct OATexts {
    short: Option<String>,
}

#[derive(Deserialize, Default)]
struct OAMetrics {
    duration: Option<OADuration>,
    length: Option<f64>,
    elevation: Option<OAElevation>,
}

#[derive(Deserialize)]
struct OADuration {
    minimal: Option<f64>,
}

#[derive(Deserialize)]
struct OAElevation {
    ascent: Option<u32>,
    descent: Option<u32>,
}

#[derive(Deserialize, Default)]
struct OARatingInfo {
    #[serde(default)]
    stamina: u32,
    #[serde(default)]
    difficulty: u32,
    #[serde(default)]
    landscape: u32,
    #[serde(default)]
    experience: u32,
}

#[derive(Deserialize)]
struct OAProperty {
    name: String,
}

#[derive(Deserialize, Default)]
struct OASeason {
    #[serde(default)]
    jan: Option<String>,
    #[serde(default)]
    feb: Option<String>,
    #[serde(default)]
    mar: Option<String>,
    #[serde(default)]
    apr: Option<String>,
    #[serde(default)]
    may: Option<String>,
    #[serde(default)]
    jun: Option<String>,
    #[serde(default)]
    jul: Option<String>,
    #[serde(default)]
    aug: Option<String>,
    #[serde(default)]
    sep: Option<String>,
    #[serde(default)]
    oct: Option<String>,
    #[serde(default)]
    nov: Option<String>,
    #[serde(default)]
    dec: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_json(id: &str, category: &str, season_mar: &str) -> String {
        format!(
            r#"{{"header":{{"status":"ok"}},"answer":{{"type":"oois","contents":[{{
                "type":"tour","id":"{id}","title":"Test Tour",
                "category":{{"type":"category","id":"1","ooiType":"tour","title":"{category}"}},
                "point":[13.0,50.0,500.0],
                "texts":{{"short":"A short desc"}},
                "metrics":{{"duration":{{"minimal":120.0}},"length":8000,
                    "elevation":{{"ascent":300,"descent":280}}}},
                "ratingInfo":{{"stamina":3,"difficulty":2,"landscape":4,"experience":1}},
                "properties":[{{"id":"1","name":"loopTour","title":"Rundtour"}}],
                "season":{{"jan":"no","feb":"no","mar":"{season_mar}","apr":"yes","may":"yes",
                    "jun":"yes","jul":"yes","aug":"yes","sep":"yes","oct":"no","nov":"no","dec":"no"}},
                "geoJson":{{"type":"LineString","coordinates":[[13.0,50.0,500.0],[13.1,50.1,600.0]]}}
            }}]}}}}"#
        )
    }

    #[test]
    fn parses_full_tour() {
        let json = minimal_json("42", "Wanderung", "yes");
        let tour = parse_tour(&json).unwrap();
        assert_eq!(tour.id, "42");
        assert_eq!(tour.title, "Test Tour");
        assert_eq!(tour.category, "Wanderung");
        assert_eq!(tour.location.latitude, 50.0);
        assert_eq!(tour.location.longitude, 13.0);
        assert_eq!(tour.description, "A short desc");
        assert_eq!(tour.duration_minutes, 120);
        assert_eq!(tour.length_meters, 8000);
        assert_eq!(tour.ascent_meters, 300);
        assert_eq!(tour.descent_meters, 280);
        assert_eq!(tour.difficulty, 2);
        assert_eq!(tour.stamina, 3);
        assert_eq!(tour.landscape, 4);
        assert_eq!(tour.experience, 1);
        assert!(tour.is_loop);
        assert!(tour.in_season(3)); // mar = yes
        assert!(!tour.in_season(1)); // jan = no
    }

    #[test]
    fn handles_missing_optional_fields() {
        let json = r#"{"header":{"status":"ok"},"answer":{"type":"oois","contents":[{
            "type":"tour","id":"1","title":"Bare",
            "category":{"type":"category","id":"1","ooiType":"tour","title":"Radtour"},
            "point":[8.0,48.0,200.0],
            "metrics":{"length":5000},
            "ratingInfo":{},
            "season":{}
        }]}}"#;
        let tour = parse_tour(json).unwrap();
        assert_eq!(tour.description, "");
        assert_eq!(tour.duration_minutes, 0);
        assert_eq!(tour.ascent_meters, 0);
        assert_eq!(tour.descent_meters, 0);
        assert_eq!(tour.difficulty, 0);
        assert!(!tour.is_loop);
        assert_eq!(tour.season_bitmask, 0);
    }

    #[test]
    fn short_point_array_errors_instead_of_panicking() {
        let json = r#"{"header":{"status":"ok"},"answer":{"type":"oois","contents":[{
            "type":"tour","id":"9","title":"NoPoint",
            "category":{"type":"category","id":"1","ooiType":"tour","title":"Wanderung"},
            "point":[13.0],
            "metrics":{},"ratingInfo":{},"season":{}
        }]}}"#;
        assert!(parse_tour(json).is_err());
    }

    #[test]
    fn season_bitmask_round_trips() {
        let json = minimal_json("1", "Wanderung", "no");
        let tour = parse_tour(&json).unwrap();
        // apr-sep = bits 3..8
        assert_eq!(tour.season_bitmask, 0b0001_1111_1000);
        for m in 1..=12 {
            let expected = (4..=9).contains(&m);
            assert_eq!(tour.in_season(m), expected, "month {m}");
        }
    }
}
