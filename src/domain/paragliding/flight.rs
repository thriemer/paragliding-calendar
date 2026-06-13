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

#[derive(Debug, Clone, Copy)]
pub struct Angle(f64);

#[derive(Debug, Clone, Copy)]
pub struct AngleDelta(f64);

#[derive(Debug, Clone, Copy)]
pub struct AngularVelocity(f64);

impl Div<Duration> for AngleDelta {
    type Output = AngularVelocity;

    fn div(self, rhs: Duration) -> Self::Output {
        AngularVelocity(self.0 / rhs.as_seconds_f64())
    }
}

impl Angle {
    pub fn to_cartesian(&self) -> (f64, f64) {
        let (s, c) = (self.0 * std::f64::consts::PI / 180.0).sin_cos();
        (c, s) // cosine, sine because x is forward
    }
}

impl Sub for Angle {
    type Output = AngleDelta;

    fn sub(self, rhs: Self) -> Self::Output {
        let mut diff = (self.0 - rhs.0) % 360.0;
        if diff > 180.0 {
            diff -= 360.0;
        } else if diff < -180.0 {
            diff += 360.0;
        }
        AngleDelta(diff)
    }
}

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

pub struct EuclideanVelocity {
    pub vx: ScalarVelocity, // x is forward
    pub vy: ScalarVelocity, // y is up
    pub vz: ScalarVelocity,
}

impl From<BearingVelocity> for EuclideanVelocity {
    fn from(value: BearingVelocity) -> Self {
        let (x, z) = value.bearing.to_cartesian();

        EuclideanVelocity {
            vx: value.horizontal * x,
            vy: value.vertical,
            vz: value.horizontal * z,
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

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct ScalarVelocity(f64);

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

impl Into<geo::Point> for &Location {
    fn into(self) -> geo::Point {
        geo::Point::new(self.longitude, self.latitude)
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
    pub metadata: String,
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
    fn angle_subtraction_normalises_within_180_range() {
        let a = Angle(10.0);
        let b = Angle(350.0);
        let delta = a - b;
        assert!(delta.0.abs() <= 180.0);
    }

    #[test]
    fn angle_subtraction_is_clockwise_from_rhs_to_self() {
        // Going from 350° to 10° the short way is +20° (clockwise).
        let delta = Angle(10.0) - Angle(350.0);
        assert!((delta.0 - 20.0).abs() < 1e-9, "expected +20°, got {}", delta.0);

        // And the reverse should be -20°.
        let delta = Angle(350.0) - Angle(10.0);
        assert!((delta.0 + 20.0).abs() < 1e-9, "expected -20°, got {}", delta.0);
    }

    #[test]
    fn to_cartesian_returns_forward_axis_for_zero_degrees() {
        // Bearing 0° (north) should map to x=1, z=0 (purely forward).
        let (x, z) = Angle(0.0).to_cartesian();
        assert!((x - 1.0).abs() < 1e-9, "x={x}");
        assert!(z.abs() < 1e-9, "z={z}");
    }

    #[test]
    fn to_cartesian_returns_lateral_axis_for_ninety_degrees() {
        // Bearing 90° (east) should map to x=0, z=1.
        let (x, z) = Angle(90.0).to_cartesian();
        assert!(x.abs() < 1e-9, "x={x}");
        assert!((z - 1.0).abs() < 1e-9, "z={z}");
    }

    #[test]
    fn to_cartesian_is_periodic_in_360_degrees() {
        let (x0, z0) = Angle(45.0).to_cartesian();
        let (x1, z1) = Angle(405.0).to_cartesian();
        assert!((x0 - x1).abs() < 1e-9);
        assert!((z0 - z1).abs() < 1e-9);
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
