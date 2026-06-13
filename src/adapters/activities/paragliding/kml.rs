use chrono::{DateTime, NaiveDateTime, Utc};
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
                        in_track_placemark = true;
                        in_placemark = true;
                    } else if in_track_placemark && tag_name == "Metadata" {
                        in_metadata = true;
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"type" {
                                let attr_val = String::from_utf8_lossy(&attr.value).to_string();
                                if attr_val == "track" {
                                    in_track_placemark = true;
                                } else {
                                    in_track_placemark = false;
                                }
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

            let offset_secs = if i < seconds.len() {
                seconds[i]
            } else {
                i as i64
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

fn parse_datetime(s: &str) -> Result<DateTime<Utc>, Box<dyn std::error::Error>> {
    let naive = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")?;
    Ok(DateTime::from_naive_utc_and_offset(naive, Utc))
}
