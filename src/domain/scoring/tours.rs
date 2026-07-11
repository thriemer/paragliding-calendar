//! Weather-suitability scoring for outdoor tours. Pure policy — no I/O, no ports.

use crate::domain::{activities::ActivityKind, weather::WeatherData};

/// Per-hour weather sensitivity params, tuned per activity.
struct Sensitivity {
    /// Precipitation (mm/h) at which fun hits zero.
    rain_kill: f32,
    /// Wind (m/s) tolerated with no penalty, and where fun hits zero.
    wind_ok: f32,
    wind_kill: f32,
    /// Comfort temperature and the half-width (°C) of the tolerable band.
    ideal_temp: f32,
    temp_span: f32,
}

// ponytail: hand-tuned starting values, one knob-set per kind. Adjust from real forecasts/feedback
// rather than adding config until there's a reason to.
fn sensitivity(kind: ActivityKind) -> Sensitivity {
    match kind {
        // Wet rock / exposed ridges — rain and wind matter most.
        ActivityKind::MountainClimbing => Sensitivity {
            rain_kill: 2.0,
            wind_ok: 8.0,
            wind_kill: 15.0,
            ideal_temp: 15.0,
            temp_span: 18.0,
        },
        // Wind on open water is the killer.
        ActivityKind::Kayaking => Sensitivity {
            rain_kill: 6.0,
            wind_ok: 5.0,
            wind_kill: 12.0,
            ideal_temp: 20.0,
            temp_span: 18.0,
        },
        // Dislikes rain; heat-sensitive (narrow, cool comfort band).
        ActivityKind::Running => Sensitivity {
            rain_kill: 6.0,
            wind_ok: 10.0,
            wind_kill: 20.0,
            ideal_temp: 12.0,
            temp_span: 14.0,
        },
        ActivityKind::Biking => Sensitivity {
            rain_kill: 4.0,
            wind_ok: 8.0,
            wind_kill: 18.0,
            ideal_temp: 18.0,
            temp_span: 18.0,
        },
        // Hiking (and the default) — most weather-tolerant.
        _ => Sensitivity {
            rain_kill: 8.0,
            wind_ok: 12.0,
            wind_kill: 25.0,
            ideal_temp: 15.0,
            temp_span: 20.0,
        },
    }
}

/// A 0..1 fun multiplier for the hour plus a human-readable reason (rides into the calendar body).
/// Rain and wind gate hard (multiply toward zero); temperature softens.
pub fn weather_suitability(kind: ActivityKind, wd: &WeatherData) -> (f32, String) {
    let s = sensitivity(kind);

    let rain = (1.0 - wd.precipitation / s.rain_kill).clamp(0.0, 1.0);
    let wind = if wd.wind_speed_ms <= s.wind_ok {
        1.0
    } else {
        (1.0 - (wd.wind_speed_ms - s.wind_ok) / (s.wind_kill - s.wind_ok)).clamp(0.0, 1.0)
    };
    let temp = (1.0 - (wd.temperature - s.ideal_temp).abs() / s.temp_span).clamp(0.0, 1.0);

    let factor = rain * wind * (0.5 + 0.5 * temp);
    let reason = format!(
        "{:.0} °C, {:.1} mm rain, {:.0} m/s wind",
        wd.temperature, wd.precipitation, wd.wind_speed_ms
    );
    (factor, reason)
}
