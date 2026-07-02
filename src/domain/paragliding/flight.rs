use std::{
    cmp::Ordering,
    fmt::Display,
    ops::{Add, Div, Mul, Sub},
};

use chrono::{DateTime, Duration, Utc};
use geo::{Bearing as _, Distance as GeoDistance};

pub struct Distance(f64); //distance in meters

impl Distance {
    pub fn from_km(km: f64) -> Self {
        Distance(km * 1000.0)
    }
    pub fn from_meters(m: f64) -> Self {
        Distance(m)
    }
}

impl Add for Distance {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Distance(self.0 + other.0)
    }
}

impl Display for Distance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0 > 1000.0 {
            write!(f, "{:.2}km", self.0 / 1000.0)
        } else {
            write!(f, "{:.0}m", self.0)
        }
    }
}

// ponytail: the wrapped value is only carried, not read, since turn-rate analysis was cut;
// the next consumer (turn detection, wind drift) reads it again.
#[derive(Debug, Clone, Copy)]
pub struct Angle(#[allow(dead_code)] f64);

pub struct LocationDelta {
    pub distance: Distance,
    pub bearing: Angle,
    pub height_diff: Distance,
}

impl Div<Duration> for LocationDelta {
    type Output = BearingVelocity;

    fn div(self, rhs: Duration) -> Self::Output {
        BearingVelocity {
            horizontal: self.distance / rhs,
            bearing: self.bearing,
            vertical: self.height_diff / rhs,
        }
    }
}

pub struct BearingVelocity {
    pub horizontal: ScalarVelocity,
    pub bearing: Angle,
    pub vertical: ScalarVelocity,
}

#[derive(Debug, Clone, Copy)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScalarVelocity(f64);

impl PartialOrd for ScalarVelocity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl ScalarVelocity {
    pub fn min(&self, other: &ScalarVelocity) -> ScalarVelocity {
        if self.0 < other.0 { *self } else { *other }
    }
    pub fn max(&self, other: &ScalarVelocity) -> ScalarVelocity {
        if self.0 > other.0 { *self } else { *other }
    }
    pub fn from_ms(ms: f64) -> Self {
        ScalarVelocity(ms)
    }

    pub fn get_ms(&self) -> f64 {
        self.0
    }
}

impl Mul<f64> for ScalarVelocity {
    type Output = ScalarVelocity;

    fn mul(self, rhs: f64) -> Self::Output {
        ScalarVelocity(self.0 * rhs)
    }
}

impl Eq for ScalarVelocity {}

impl Display for ScalarVelocity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0 < 4.0 {
            write!(f, "{:.1} m/s", self.0)
        } else {
            write!(f, "{:.1} km/h", self.0 * 3.6)
        }
    }
}

impl Add for ScalarVelocity {
    type Output = ScalarVelocity;

    fn add(self, rhs: Self) -> Self::Output {
        ScalarVelocity(self.0 + rhs.0)
    }
}

impl Ord for ScalarVelocity {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl Div<Duration> for Distance {
    type Output = ScalarVelocity;

    fn div(self, rhs: Duration) -> Self::Output {
        ScalarVelocity(self.0 / rhs.as_seconds_f64())
    }
}

impl Sub for Location {
    type Output = LocationDelta;

    fn sub(self, rhs: Self) -> Self::Output {
        LocationDelta {
            distance: self.distance(&rhs),
            bearing: self.bearing(&rhs),
            height_diff: Distance::from_meters(self.height - rhs.height),
        }
    }
}

impl Location {
    pub fn distance(&self, other: &Location) -> Distance {
        let hd = geo::Haversine.distance(self.into(), other.into());
        let vd = self.height - other.height;
        Distance::from_meters((hd * hd + vd * vd).sqrt())
    }

    pub fn bearing(&self, other: &Location) -> Angle {
        Angle(geo::Geodesic.bearing(self.into(), other.into()))
    }
}

impl From<&Location> for geo::Point {
    fn from(loc: &Location) -> geo::Point {
        geo::Point::new(loc.longitude, loc.latitude)
    }
}

#[derive(Debug, Clone)]
pub struct TrackPoint {
    pub loc: Location,
    pub time: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Track {
    pub points: Vec<TrackPoint>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_from_km_and_from_meters_are_equivalent() {
        let a = Distance::from_km(1.0);
        let b = Distance::from_meters(1000.0);
        assert_eq!(a.0, b.0);
    }

    #[test]
    fn distance_display_switches_units_at_one_kilometer() {
        assert_eq!(format!("{}", Distance::from_meters(800.0)), "800m");
        assert_eq!(format!("{}", Distance::from_km(1.5)), "1.50km");
    }

    #[test]
    fn distance_addition_sums_meters() {
        let s = Distance::from_meters(100.0) + Distance::from_meters(250.0);
        assert_eq!(s.0, 350.0);
    }

    #[test]
    fn scalar_velocity_min_max_pick_correct_value() {
        let slow = ScalarVelocity::from_ms(2.0);
        let fast = ScalarVelocity::from_ms(8.0);
        assert_eq!(ScalarVelocity::min(&slow, &fast).get_ms(), 2.0);
        assert_eq!(ScalarVelocity::max(&slow, &fast).get_ms(), 8.0);
    }

    #[test]
    fn scalar_velocity_display_switches_units_at_4_ms() {
        assert_eq!(format!("{}", ScalarVelocity::from_ms(3.5)), "3.5 m/s");
        assert_eq!(format!("{}", ScalarVelocity::from_ms(5.0)), "18.0 km/h");
    }

    #[test]
    fn distance_per_duration_gives_velocity_in_ms() {
        let v = Distance::from_meters(100.0) / Duration::seconds(10);
        assert_eq!(v.get_ms(), 10.0);
    }

    #[test]
    fn location_distance_to_self_is_zero() {
        let p = Location {
            latitude: 50.0,
            longitude: 13.0,
            height: 1000.0,
        };
        assert_eq!(p.distance(&p).0, 0.0);
    }

    #[test]
    fn location_distance_includes_vertical_component() {
        let a = Location {
            latitude: 50.0,
            longitude: 13.0,
            height: 0.0,
        };
        let b = Location {
            latitude: 50.0,
            longitude: 13.0,
            height: 100.0,
        };
        let d = a.distance(&b).0;
        assert!((d - 100.0).abs() < 0.001, "expected ~100m, got {d}");
    }
}
