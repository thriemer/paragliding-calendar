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
        let (s, c) = (0.5 * self.0 / std::f64::consts::PI).sin_cos();
        (c, s) // cosine, sine because x is forward
    }
}

impl Sub for Angle {
    type Output = AngleDelta;

    fn sub(self, rhs: Self) -> Self::Output {
        let mut diff = (rhs.0 - self.0) % 360.0;
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
