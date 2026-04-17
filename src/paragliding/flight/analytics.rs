use chrono::{DateTime, Duration, Utc};
use std::{
    fmt::{Display, Pointer},
    fs,
};

use crate::paragliding::flight::{
    AngularVelocity, BearingVelocity, Distance, ScalarVelocity, Track,
};

pub fn analyse_flight(track: &Track) -> String {
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

    format!(
        r#"Flight took: {} and was {} long. Height above start: {}
        Track log length: {}, Max Climb (60s): {}, Max Sink (60s): {}
        Min Speed: {}, Max Speed: {}
        Min Glide: {:.1}, Avg Glide: {:.1}
        Total elevation gained: {}"#,
        d.unwrap(),
        km.unwrap(),
        height.unwrap(),
        tracklog_length.unwrap(),
        max_climb,
        max_sink,
        min_speed,
        max_speed,
        min_glide,
        sum_glide / count as f64,
        total_height_gained
    )
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
