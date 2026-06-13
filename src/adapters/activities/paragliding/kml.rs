use chrono::{DateTime, NaiveDateTime, Utc};
use std::error::Error;
use quick_xml::Reader;
use quick_xml::events::Event;
use std::fs;

use crate::domain::paragliding::flight::{Location, Track, TrackPoint};

impl Track {
    pub fn from_kml_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        Self::from_kml(&content)
    }

    pub fn from_kml(xml: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut reader = Reader::from_str(xml);

        let mut buf = Vec::new();
        let mut in_placemark = false;
        let mut in_track_placemark = false;
        let mut in_metadata = false;
        let mut in_coords = false;
        let mut in_seconds = false;
        let mut in_altitude = false;
        let mut in_description = false;

        let mut time_of_first_point: Option<String> = None;
        let mut seconds: Vec<i64> = Vec::new();
        let mut altitudes: Vec<f64> = Vec::new();
        let mut coords_raw = String::new();
        let mut description = String::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    let name = e.name();
                    let tag_name = String::from_utf8_lossy(name.as_ref()).to_string();

                    if tag_name == "Placemark" {
                        // Stay neutral until the Metadata element confirms this is a track.
                        // Without this, non-track Placemarks (e.g. takeoff markers) would
                        // leak their <coordinates> into the track buffer.
                        in_track_placemark = false;
                        in_placemark = true;
                    } else if in_placemark && tag_name == "Metadata" {
                        in_metadata = true;
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"type" {
                                let attr_val = String::from_utf8_lossy(&attr.value).to_string();
                                in_track_placemark = attr_val == "track";
                                break;
                            }
                        }
                    } else if in_metadata && tag_name == "FsInfo" {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"time_of_first_point" {
                                time_of_first_point =
                                    Some(String::from_utf8_lossy(&attr.value).to_string());
                                break;
                            }
                        }
                    } else if in_metadata && tag_name == "SecondsFromTimeOfFirstPoint" {
                        in_seconds = true;
                    } else if in_metadata && tag_name == "PressureAltitude" {
                        in_altitude = true;
                    } else if in_track_placemark && tag_name == "coordinates" {
                        in_coords = true;
                    }
                    if !in_placemark && tag_name == "description" {
                        in_description = true;
                    }
                }
                Ok(Event::Text(e)) => {
                    let text = e.xml_content()?.to_string();

                    if in_coords {
                        coords_raw.push_str(&text);
                    } else if in_seconds {
                        for num in text.split_whitespace() {
                            if let Ok(v) = num.parse::<i64>() {
                                seconds.push(v);
                            }
                        }
                    } else if in_altitude {
                        for num in text.split_whitespace() {
                            if let Ok(v) = num.parse::<f64>() {
                                altitudes.push(v);
                            }
                        }
                    }
                }
                Ok(Event::CData(e)) => {
                    if in_description {
                        description = e.xml_content()?.to_string();
                    }
                }
                Ok(Event::End(e)) => {
                    let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    if tag_name == "Placemark" {
                        in_track_placemark = false;
                        in_placemark = false;
                        in_metadata = false;
                    } else if tag_name == "Metadata" {
                        in_metadata = false;
                    } else if tag_name == "SecondsFromTimeOfFirstPoint" {
                        in_seconds = false;
                    } else if tag_name == "PressureAltitude" {
                        in_altitude = false;
                    } else if tag_name == "coordinates" {
                        in_coords = false;
                    } else if tag_name == "description" {
                        in_description = false;
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(Box::new(e)),
                _ => {}
            }
            buf.clear();
        }

        if coords_raw.trim().is_empty() {
            return Err("No coordinates found in track placemark".into());
        }

        let base_time = time_of_first_point.ok_or("Missing time_of_first_point")?;
        let base_datetime = parse_datetime(&base_time)?;

        let coord_strings: Vec<&str> = coords_raw
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .collect();

        let mut points = Vec::new();

        for (i, coord_str) in coord_strings.iter().enumerate() {
            let parts: Vec<&str> = coord_str.split(',').collect();
            if parts.len() != 3 {
                continue;
            }

            let lon: f64 = parts[0].parse().map_err(|_| "Invalid longitude")?;
            let lat: f64 = parts[1].parse().map_err(|_| "Invalid latitude")?;
            let height: f64 = parts[2].parse().map_err(|_| "Invalid height")?;

            // Skip points without an explicit timestamp rather than guessing the
            // offset from the index — the cadence is not constant across emitters.
            let Some(&offset_secs) = seconds.get(i) else {
                tracing::warn!(
                    point_index = i,
                    seconds_len = seconds.len(),
                    "KML coords have more points than SecondsFromTimeOfFirstPoint entries; truncating"
                );
                break;
            };

            let time = base_datetime + chrono::Duration::seconds(offset_secs);

            points.push(TrackPoint {
                loc: Location {
                    latitude: lat,
                    longitude: lon,
                    height,
                },
                time,
            });
        }

        Ok(Track {
            points,
            metadata: description,
        })
    }
}

fn parse_datetime(s: &str) -> Result<DateTime<Utc>, Box<dyn Error>> {
    // Accept both "2026-06-13T10:00:00" and "2026-06-13T10:00:00Z" / "+02:00".
    // We assume the timestamp is UTC if no offset is given (matches what the FS
    // KML emitters publish).
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    let trimmed = s.trim_end_matches('Z');
    let naive = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S")?;
    Ok(DateTime::from_naive_utc_and_offset(naive, Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const SAMPLE_KML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <Placemark>
      <Metadata type="track">
        <FsInfo time_of_first_point="2026-06-13T10:00:00"></FsInfo>
        <SecondsFromTimeOfFirstPoint>0 5 10</SecondsFromTimeOfFirstPoint>
        <PressureAltitude>1000.0 1010.0 1020.0</PressureAltitude>
      </Metadata>
      <LineString>
        <coordinates>
          13.0,50.0,1000.0 13.001,50.001,1010.0 13.002,50.002,1020.0
        </coordinates>
      </LineString>
    </Placemark>
  </Document>
</kml>"#;

    #[test]
    fn parses_three_trackpoints_from_minimal_kml() {
        let track = Track::from_kml(SAMPLE_KML).unwrap();
        assert_eq!(track.points.len(), 3);
    }

    #[test]
    fn trackpoint_coordinates_are_lon_lat_height_in_kml_order() {
        let track = Track::from_kml(SAMPLE_KML).unwrap();
        let p = &track.points[0];
        assert_eq!(p.loc.longitude, 13.0);
        assert_eq!(p.loc.latitude, 50.0);
        assert_eq!(p.loc.height, 1000.0);
    }

    #[test]
    fn timestamps_use_first_point_plus_seconds_offset() {
        let track = Track::from_kml(SAMPLE_KML).unwrap();
        let base = Utc.with_ymd_and_hms(2026, 6, 13, 10, 0, 0).unwrap();
        assert_eq!(track.points[0].time, base);
        assert_eq!(track.points[1].time, base + chrono::Duration::seconds(5));
        assert_eq!(track.points[2].time, base + chrono::Duration::seconds(10));
    }

    #[test]
    fn ignores_coordinates_from_non_track_placemark() {
        let kml = r#"<?xml version="1.0"?>
<kml><Document>
<Placemark>
  <Metadata type="marker"></Metadata>
  <Point><coordinates>99.0,99.0,99.0</coordinates></Point>
</Placemark>
<Placemark>
  <Metadata type="track">
    <FsInfo time_of_first_point="2026-06-13T10:00:00"></FsInfo>
    <SecondsFromTimeOfFirstPoint>0 5</SecondsFromTimeOfFirstPoint>
  </Metadata>
  <LineString><coordinates>13.0,50.0,1000.0 13.001,50.001,1010.0</coordinates></LineString>
</Placemark>
</Document></kml>"#;
        let track = Track::from_kml(kml).unwrap();
        assert_eq!(track.points.len(), 2);
        assert_eq!(track.points[0].loc.longitude, 13.0);
    }

    #[test]
    fn accepts_iso_timestamp_with_z_suffix() {
        let kml = SAMPLE_KML.replace(
            "time_of_first_point=\"2026-06-13T10:00:00\"",
            "time_of_first_point=\"2026-06-13T10:00:00Z\"",
        );
        let track = Track::from_kml(&kml).unwrap();
        let base = Utc.with_ymd_and_hms(2026, 6, 13, 10, 0, 0).unwrap();
        assert_eq!(track.points[0].time, base);
    }

    #[test]
    fn truncates_when_seconds_shorter_than_coords() {
        let kml = r#"<?xml version="1.0"?>
<kml><Document><Placemark>
<Metadata type="track">
  <FsInfo time_of_first_point="2026-06-13T10:00:00"></FsInfo>
  <SecondsFromTimeOfFirstPoint>0 5</SecondsFromTimeOfFirstPoint>
</Metadata>
<LineString><coordinates>13.0,50.0,1000.0 13.001,50.001,1010.0 13.002,50.002,1020.0</coordinates></LineString>
</Placemark></Document></kml>"#;
        let track = Track::from_kml(kml).unwrap();
        assert_eq!(
            track.points.len(),
            2,
            "third coordinate has no matching seconds entry, must be skipped",
        );
    }

    #[test]
    fn missing_time_of_first_point_returns_error() {
        let kml = r#"<?xml version="1.0"?>
<kml><Document><Placemark>
<Metadata type="track">
<SecondsFromTimeOfFirstPoint>0</SecondsFromTimeOfFirstPoint>
</Metadata>
<LineString><coordinates>13.0,50.0,1000.0</coordinates></LineString>
</Placemark></Document></kml>"#;
        assert!(Track::from_kml(kml).is_err());
    }
}
