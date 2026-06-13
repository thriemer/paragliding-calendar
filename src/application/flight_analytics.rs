use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use crate::domain::paragliding::flight::{
    AngularVelocity, BearingVelocity, Distance, ScalarVelocity, Track, TrackPoint,
};

#[derive(Serialize)]
pub struct TrackPointDto {
    pub latitude: f64,
    pub longitude: f64,
    pub height: f64,
    pub time: String,
    pub climb_rate: f64,
}

impl From<&TrackPoint> for TrackPointDto {
    fn from(point: &TrackPoint) -> Self {
        TrackPointDto {
            latitude: point.loc.latitude,
            longitude: point.loc.longitude,
            height: point.loc.height,
            time: point.time.to_rfc3339(),
            climb_rate: 0.0,
        }
    }
}

#[derive(Serialize)]
pub struct FlightAnalysis {
    pub path: Vec<TrackPointDto>,
    pub duration: String,
    pub distance: String,
    pub max_altitude: String,
    pub track_length: String,
    pub max_climb: String,
    pub max_sink: String,
    pub min_speed: String,
    pub max_speed: String,
    pub min_glide: f64,
    pub avg_glide: f64,
    pub total_elevation_gain: String,
}

pub fn analyse_flight(track: &Track) -> FlightAnalysis {
    let d = calculate_flight_duration(&track);
    let km = calculate_flight_distance(&track);
    let height = calculate_height_over_takeoff(&track);
    let tracklog_length = calculate_track_log_length(&track);
    let bearing_vel = calculate_bearing_velocity(&track);
    let (max_sink, max_climb) = calculate_min_max_climb(&bearing_vel, 60usize).unwrap();
    let (min_speed, max_speed) = calculate_min_max_speed(&bearing_vel, 60usize).unwrap();
    let glide = calculate_glide_ratio(&bearing_vel, 60usize);
    let (min_glide, sum_glide, count) = glide
        .iter()
        .fold(None, |a: Option<(f64, f64, u32)>, g| match a {
            Some((min, sum, cnt)) => {
                if g.0.is_finite() {
                    Some((min.min(g.0), sum + g.0, cnt + 1))
                } else {
                    Some((min, sum, cnt))
                }
            }
            None => {
                if g.0.is_finite() {
                    Some((g.0, g.0, 1))
                } else {
                    None
                }
            }
        })
        .unwrap();
    let total_height_gained = calculate_total_elevation_gained(&track);

    FlightAnalysis {
        path: {
            track
                .points
                .iter()
                .enumerate()
                .map(|(i, point)| {
                    let mut dto = TrackPointDto::from(point);
                    if i > 0 && i - 1 < bearing_vel.len() {
                        dto.climb_rate = bearing_vel[i - 1].0.vertical.get_ms();
                    }
                    dto
                })
                .collect()
        },
        duration: format!("{:?}", d.unwrap()),
        distance: format!("{}", km.unwrap()),
        max_altitude: format!("{}", height.unwrap()),
        track_length: format!("{}", tracklog_length.unwrap()),
        max_climb: format!("{}", max_climb),
        max_sink: format!("{}", max_sink),
        min_speed: format!("{}", min_speed),
        max_speed: format!("{}", max_speed),
        min_glide,
        avg_glide: sum_glide / count as f64,
        total_elevation_gain: format!("{}", total_height_gained),
    }
}

fn calculate_flight_duration(track: &Track) -> Option<Duration> {
    track
        .points
        .first()
        .zip(track.points.last())
        .map(|(f, l)| l.time - f.time)
}

fn calculate_flight_distance(track: &Track) -> Option<Distance> {
    track
        .points
        .first()
        .zip(track.points.last())
        .map(|(f, l)| f.loc.distance(&l.loc))
}

fn calculate_height_over_takeoff(track: &Track) -> Option<Distance> {
    let start_height = track.points.first();
    let max_height = track.points.iter().max_by_key(|p| p.loc.height as i64);
    start_height
        .zip(max_height)
        .map(|(s, m)| Distance::from_meters(m.loc.height - s.loc.height))
}

fn calculate_track_log_length(track: &Track) -> Option<Distance> {
    track
        .points
        .windows(2)
        .map(|t| t[0].loc.distance(&t[1].loc))
        .reduce(|acc, e| acc + e)
}

fn calculate_bearing_velocity(track: &Track) -> Vec<(BearingVelocity, DateTime<Utc>)> {
    track
        .points
        .windows(3)
        .map(|t| {
            let dt = t[2].time - t[0].time;
            let dx = t[2].loc - t[0].loc;
            (dx / dt, t[1].time)
        })
        .collect()
}

fn calculate_turn_rate(
    bv: &Vec<(BearingVelocity, DateTime<Utc>)>,
) -> Vec<(AngularVelocity, DateTime<Utc>)> {
    bv.windows(3)
        .map(|t| {
            let dt = t[2].1 - t[0].1;
            let da = t[2].0.bearing - t[0].0.bearing;
            (da / dt, t[1].1)
        })
        .collect()
}

fn calculate_min_max_climb(
    bv: &Vec<(BearingVelocity, DateTime<Utc>)>,
    samples: usize,
) -> Option<(ScalarVelocity, ScalarVelocity)> {
    bv.windows(samples)
        .map(|s| {
            s.iter()
                .map(|v| v.0.vertical)
                .reduce(|acc, e| acc + e)
                .unwrap()
                * (1.0 / samples as f64)
        })
        .fold(
            None,
            |a: Option<(ScalarVelocity, ScalarVelocity)>, x| match a {
                Some((min, max)) => Some((min.min(x), max.max(x))),
                None => Some((x, x)),
            },
        )
}

fn calculate_min_max_speed(
    bv: &Vec<(BearingVelocity, DateTime<Utc>)>,
    samples: usize,
) -> Option<(ScalarVelocity, ScalarVelocity)> {
    bv.windows(samples)
        .map(|s| {
            s.iter()
                .map(|v| v.0.horizontal)
                .reduce(|acc, e| acc + e)
                .unwrap()
                * (1.0 / samples as f64)
        })
        .fold(
            None,
            |a: Option<(ScalarVelocity, ScalarVelocity)>, x| match a {
                Some((min, max)) => Some((min.min(x), max.max(x))),
                None => Some((x, x)),
            },
        )
}

fn calculate_glide_ratio(
    bv: &Vec<(BearingVelocity, DateTime<Utc>)>,
    samples: usize,
) -> Vec<(f64, DateTime<Utc>)> {
    bv.windows(samples)
        .map(|s| {
            let avg_ground_speed = s
                .iter()
                .map(|v| v.0.horizontal)
                .reduce(|acc, e| acc + e)
                .unwrap()
                * (1.0 / samples as f64);
            let avg_sink = s
                .iter()
                .map(|v| v.0.vertical)
                .reduce(|acc, e| acc + e)
                .unwrap()
                * (1.0 / samples as f64);
            let time = s.get(s.len() / 2).unwrap().1;

            if avg_sink < ScalarVelocity::from_ms(0.0) {
                (avg_ground_speed.get_ms() / avg_sink.get_ms().abs(), time)
            } else {
                (f64::INFINITY, time)
            }
        })
        .collect()
}

fn calculate_total_elevation_gained(track: &Track) -> Distance {
    Distance::from_meters(
        track
            .points
            .windows(2)
            .map(|t| (t[1].loc.height - t[0].loc.height).max(0.0))
            .sum(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::paragliding::flight::Location as FlightLocation;
    use chrono::TimeZone;

    fn point(lat: f64, lon: f64, height: f64, secs: i64) -> TrackPoint {
        TrackPoint {
            loc: FlightLocation {
                latitude: lat,
                longitude: lon,
                height,
            },
            time: Utc.with_ymd_and_hms(2026, 6, 13, 10, 0, 0).unwrap()
                + Duration::seconds(secs),
        }
    }

    fn track(points: Vec<TrackPoint>) -> Track {
        Track {
            points,
            metadata: String::new(),
        }
    }

    #[test]
    fn flight_duration_is_last_minus_first() {
        let t = track(vec![
            point(50.0, 13.0, 1000.0, 0),
            point(50.0, 13.0, 1000.0, 600),
        ]);
        assert_eq!(calculate_flight_duration(&t), Some(Duration::seconds(600)));
    }

    #[test]
    fn flight_duration_is_none_for_empty_track() {
        let t = track(vec![]);
        assert!(calculate_flight_duration(&t).is_none());
    }

    #[test]
    fn height_over_takeoff_uses_max_minus_start() {
        let t = track(vec![
            point(50.0, 13.0, 1000.0, 0),
            point(50.0, 13.0, 1500.0, 60),
            point(50.0, 13.0, 1200.0, 120),
        ]);
        let h = calculate_height_over_takeoff(&t).unwrap();
        assert_eq!(format!("{h}"), "500m");
    }

    #[test]
    fn total_elevation_gained_ignores_descents() {
        let t = track(vec![
            point(50.0, 13.0, 1000.0, 0),
            point(50.0, 13.0, 1100.0, 60),
            point(50.0, 13.0, 1050.0, 120),
            point(50.0, 13.0, 1200.0, 180),
        ]);
        let gain = calculate_total_elevation_gained(&t);
        assert_eq!(format!("{gain}"), "250m");
    }

    #[test]
    fn track_log_length_sums_segment_distances() {
        let t = track(vec![
            point(50.0, 13.0, 0.0, 0),
            point(50.0, 13.0, 100.0, 60),
            point(50.0, 13.0, 200.0, 120),
        ]);
        let length = calculate_track_log_length(&t).unwrap();
        assert_eq!(format!("{length}"), "200m");
    }
}
