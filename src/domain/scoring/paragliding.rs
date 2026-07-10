use std::collections::HashMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};

use crate::domain::{
    paragliding::{ParaglidingLaunch, ParaglidingSite, SiteType},
    weather::{self, WeatherData, WeatherForecast},
};

#[derive(Debug, Clone)]
pub struct HourlyScore {
    pub timestamp: DateTime<Utc>,
    pub score: f32,
    pub is_flyable: bool,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct DailySummary {
    pub date: NaiveDate,
    pub hourly_scores: Vec<HourlyScore>,
    pub ranges: Vec<FlyableRange>,
}

#[derive(Debug, Clone)]
pub struct FlyableRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl DailySummary {
    pub fn calculate_flyable_time_ranges(&mut self) {
        self.ranges.clear();

        let mut flyable: Vec<&HourlyScore> =
            self.hourly_scores.iter().filter(|h| h.is_flyable).collect();
        flyable.sort_by_key(|h| h.timestamp);

        let mut ranges: Vec<FlyableRange> = Vec::new();
        let mut current_range: Option<Vec<&HourlyScore>> = None;

        for score in flyable {
            match &mut current_range {
                Some(range_scores) => {
                    let last_score = range_scores.last().unwrap();

                    if score.timestamp == last_score.timestamp + Duration::hours(1) {
                        range_scores.push(score);
                    } else {
                        let start = range_scores.first().unwrap().timestamp;
                        let end = range_scores.last().unwrap().timestamp;
                        ranges.push(FlyableRange { start, end });

                        current_range = Some(vec![score]);
                    }
                }
                None => {
                    current_range = Some(vec![score]);
                }
            }
        }

        if let Some(range_scores) = current_range {
            let start = range_scores.first().unwrap().timestamp;
            let end = range_scores.last().unwrap().timestamp;
            ranges.push(FlyableRange { start, end });
        }

        self.ranges = ranges;
    }
}

#[derive(Debug, Clone)]
pub struct SiteEvaluationResult {
    pub daily_summaries: Vec<DailySummary>,
}

const MAX_WIND_MS: f32 = 25.0 / 3.6;
const MAX_GUST_MS: f32 = 40.0 / 3.6;
const THERMAL_MAX_WIND_MS: f32 = 6.0;

fn ridge_wind_score(wind_ms: f32) -> f32 {
    let v = wind_ms.clamp(0.0, MAX_WIND_MS);
    4.0 * (v / MAX_WIND_MS) * (1.0 - v / MAX_WIND_MS)
}

fn thermal_wind_score(wind_ms: f32) -> f32 {
    let ratio = 1.0 - (wind_ms / THERMAL_MAX_WIND_MS).min(1.0);
    ratio * ratio
}

fn cloud_score(cover: u8) -> f32 {
    let diff = cover as f32 - 25.0;
    let val = 1.0 - (diff / 45.0).powi(2);
    val.max(0.0)
}

fn wind_direction_in_sector(wind_dir: f64, start: f64, stop: f64) -> bool {
    if start == stop {
        return true;
    }
    if start < stop {
        start < wind_dir && wind_dir < stop
    } else {
        start < wind_dir || wind_dir < stop
    }
}

fn wind_direction_score(wind_dir: u16, start: f64, stop: f64) -> f32 {
    if start == stop {
        return 1.0;
    }
    let wind_dir = wind_dir as f64 % 360.0;
    let sector_size = if start < stop {
        stop - start
    } else {
        stop + 360.0 - start
    };
    let half = sector_size / 2.0;
    let center = (start + half) % 360.0;
    let delta = (wind_dir - center).abs();
    let angular_dist = delta.min(360.0 - delta);
    if angular_dist > half {
        0.0
    } else {
        (1.0 - angular_dist / half) as f32
    }
}

fn score_hour(weather: &WeatherData, launch: &ParaglidingLaunch) -> (f32, String) {
    if !matches!(launch.site_type, SiteType::Hang) {
        return (
            0.0,
            "Winch launch site is not suitable for free flying".into(),
        );
    }
    if weather.precipitation != 0.0 {
        return (
            0.0,
            format!(
                "Precipitation: {:.1} mm — cannot fly in rain",
                weather.precipitation
            ),
        );
    }
    if weather.wind_speed_ms >= MAX_WIND_MS {
        return (
            0.0,
            format!(
                "Wind too strong: {:.1} m/s exceeds limit of {:.1} m/s",
                weather.wind_speed_ms, MAX_WIND_MS
            ),
        );
    }
    if weather.wind_gust_ms >= MAX_GUST_MS {
        return (
            0.0,
            format!(
                "Gusts too strong: {:.1} m/s exceeds limit of {:.1} m/s",
                weather.wind_gust_ms, MAX_GUST_MS
            ),
        );
    }
    if !wind_direction_in_sector(
        weather.wind_direction as f64,
        launch.direction_degrees_start,
        launch.direction_degrees_stop,
    ) {
        return (
            0.0,
            format!(
                "Wind direction {}° outside launch sector {}°–{}°",
                weather.wind_direction,
                launch.direction_degrees_start,
                launch.direction_degrees_stop,
            ),
        );
    }

    let dir_score = wind_direction_score(
        weather.wind_direction,
        launch.direction_degrees_start,
        launch.direction_degrees_stop,
    );

    let ridge = ridge_wind_score(weather.wind_speed_ms) * dir_score;
    let thermal = thermal_wind_score(weather.wind_speed_ms)
        * (0.3 + 0.7 * dir_score)
        * cloud_score(weather.cloud_cover);

    if ridge >= thermal {
        (
            ridge,
            format!(
                "Ridge soaring: wind {:.1} m/s from {}° (direction score {:.0}%)",
                weather.wind_speed_ms,
                weather.wind_direction,
                dir_score * 100.0,
            ),
        )
    } else {
        (
            thermal,
            format!(
                "Thermal flying: wind {:.1} m/s, cloud cover {}%",
                weather.wind_speed_ms, weather.cloud_cover,
            ),
        )
    }
}

pub async fn evaluate_site(
    site: &ParaglidingSite,
    forecast: &WeatherForecast,
) -> SiteEvaluationResult {
    let daily_forecasts = split_forecast_by_days(forecast.clone());
    let mut daily_summaries = Vec::new();

    for daily_forecast in daily_forecasts {
        if daily_forecast.forecast.is_empty() {
            continue;
        }

        let date = daily_forecast.forecast[0].timestamp.date_naive();
        let mut hourly_scores = Vec::new();

        for weather_data in &daily_forecast.forecast {
            let (score, reason) = site
                .launches
                .iter()
                .map(|launch| score_hour(weather_data, launch))
                .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or((0.0, "No suitable launch found".into()));

            hourly_scores.push(HourlyScore {
                timestamp: weather_data.timestamp,
                score,
                is_flyable: score > 0.0,
                reason,
            });
        }

        let mut daily_summary = DailySummary {
            date,
            hourly_scores,
            ranges: vec![],
        };
        daily_summary.calculate_flyable_time_ranges();
        daily_summaries.push(daily_summary);
    }

    daily_summaries.sort_by_key(|d| d.date);
    SiteEvaluationResult { daily_summaries }
}

fn split_forecast_by_days(forecast: WeatherForecast) -> Vec<WeatherForecast> {
    let mut daily_forecasts: HashMap<NaiveDate, Vec<WeatherData>> = HashMap::new();

    for weather_data in forecast.forecast {
        let date = weather_data.timestamp.date_naive();
        daily_forecasts.entry(date).or_default().push(weather_data);
    }

    daily_forecasts
        .into_iter()
        .filter_map(|(date, daily_data)| {
            let (sunrise, sunset) = weather::get_sunrise_sunset(&forecast.location, date).unwrap();

            let filtered_data: Vec<WeatherData> = daily_data
                .into_iter()
                .filter(|data| data.timestamp >= sunrise && data.timestamp <= sunset)
                .collect();

            if filtered_data.is_empty() {
                None
            } else {
                Some(WeatherForecast {
                    location: forecast.location.clone(),
                    forecast: filtered_data,
                })
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        location::Location,
        paragliding::{ParaglidingLaunch, ParaglidingSite, SiteType},
    };
    use chrono::{TimeZone, Timelike};
    use rstest::rstest;

    fn assert_close(a: f32, b: f32, eps: f32) {
        assert!((a - b).abs() < eps, "{a} != {b} (eps={eps})");
    }

    fn loc(lat: f64, lon: f64) -> Location {
        Location::new(lat, lon, "Test".into(), "Test".into())
    }

    fn launch(start: f64, stop: f64, site_type: SiteType) -> ParaglidingLaunch {
        ParaglidingLaunch {
            site_type,
            location: loc(50.0, 13.0),
            direction_degrees_start: start,
            direction_degrees_stop: stop,
            elevation: 500.0,
        }
    }

    fn site(launches: Vec<ParaglidingLaunch>) -> ParaglidingSite {
        ParaglidingSite {
            name: "Test Site".into(),
            launches,
            landings: vec![],
            country: None,
            data_source: "test".into(),
            parking_location: None,
            mute_alerts: None,
            rating: None,
            preferred_weather_model: None,
        }
    }

    fn weather(ts: DateTime<Utc>) -> WeatherData {
        WeatherData {
            timestamp: ts,
            temperature: 20.0,
            wind_speed_ms: 3.0,
            wind_direction: 135,
            wind_gust_ms: 5.0,
            precipitation: 0.0,
            cloud_cover: 0,
            pressure: 1013.0,
            visibility: Some(10.0),
            description: String::new(),
        }
    }

    fn ts(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 13, hour, 0, 0).unwrap()
    }

    // --- wind_direction_in_sector tests (unchanged) ---

    #[rstest]
    #[case(90.0, 180.0, 135.0, true)]
    #[case(90.0, 180.0, 89.0, false)]
    #[case(90.0, 180.0, 181.0, false)]
    #[case(90.0, 180.0, 90.0, false)]
    #[case(90.0, 180.0, 180.0, false)]
    #[case(330.0, 30.0, 350.0, true)]
    #[case(330.0, 30.0, 10.0, true)]
    #[case(330.0, 30.0, 0.0, true)]
    #[case(330.0, 30.0, 100.0, false)]
    #[case(330.0, 30.0, 330.0, false)]
    #[case(330.0, 30.0, 30.0, false)]
    #[case(180.0, 180.0, 180.0, true)]
    #[case(180.0, 180.0, 45.0, true)]
    #[case(0.0, 0.0, 0.0, true)]
    #[case(0.0, 0.0, 180.0, true)]
    fn wind_direction_in_sector_cases(
        #[case] start: f64,
        #[case] stop: f64,
        #[case] wind: f64,
        #[case] expected: bool,
    ) {
        assert_eq!(wind_direction_in_sector(wind, start, stop), expected);
    }

    // --- ridge_wind_score tests ---

    #[test]
    fn ridge_wind_zero_returns_zero() {
        assert_eq!(ridge_wind_score(0.0), 0.0);
    }

    #[test]
    fn ridge_wind_at_max_returns_zero() {
        assert_eq!(ridge_wind_score(MAX_WIND_MS), 0.0);
    }

    #[test]
    fn ridge_wind_above_max_returns_zero() {
        assert_eq!(ridge_wind_score(MAX_WIND_MS * 2.0), 0.0);
    }

    #[test]
    fn ridge_wind_peaks_at_half_max() {
        let peak = ridge_wind_score(MAX_WIND_MS / 2.0);
        assert_close(peak, 1.0, 1e-6);
    }

    #[test]
    fn ridge_wind_is_symmetric_around_peak() {
        let low = ridge_wind_score(MAX_WIND_MS * 0.25);
        let high = ridge_wind_score(MAX_WIND_MS * 0.75);
        assert_close(low, high, 1e-6);
    }

    // --- thermal_wind_score tests ---

    #[test]
    fn thermal_wind_zero_returns_one() {
        assert_eq!(thermal_wind_score(0.0), 1.0);
    }

    #[test]
    fn thermal_wind_at_max_returns_zero() {
        assert_eq!(thermal_wind_score(THERMAL_MAX_WIND_MS), 0.0);
    }

    #[test]
    fn thermal_wind_above_max_returns_zero() {
        assert_eq!(thermal_wind_score(THERMAL_MAX_WIND_MS * 2.0), 0.0);
    }

    #[test]
    fn thermal_wind_decays_quadratically() {
        let half = thermal_wind_score(THERMAL_MAX_WIND_MS / 2.0);
        assert_close(half, 0.25, 1e-6);
    }

    // --- cloud_score tests ---

    #[test]
    fn cloud_score_at_optimal_is_one() {
        assert_close(cloud_score(25), 1.0, 1e-6);
    }

    #[test]
    fn cloud_score_clear_skies_is_moderate() {
        let s = cloud_score(0);
        assert!(s > 0.6 && s < 0.8);
    }

    #[test]
    fn cloud_score_overcast_is_zero() {
        assert_eq!(cloud_score(80), 0.0);
        assert_eq!(cloud_score(100), 0.0);
    }

    #[test]
    fn cloud_score_symmetric_around_optimal() {
        let a = cloud_score(10);
        let b = cloud_score(40);
        assert_close(a, b, 1e-6);
    }

    // --- wind_direction_score tests ---

    #[rstest]
    #[case(0.0, 360.0, 180, 1.0)]
    #[case(90.0, 180.0, 135, 1.0)]
    #[case(90.0, 180.0, 90, 0.0)]
    #[case(90.0, 180.0, 180, 0.0)]
    #[case(330.0, 30.0, 0, 1.0)]
    #[case(330.0, 30.0, 350, 0.6666667)]
    #[case(330.0, 30.0, 15, 0.5)]
    #[case(90.0, 180.0, 112, 0.4888889)]
    fn wind_direction_score_cases(
        #[case] start: f64,
        #[case] stop: f64,
        #[case] wind: u16,
        #[case] expected: f32,
    ) {
        let score = wind_direction_score(wind, start, stop);
        assert_close(score, expected, 1e-4);
    }

    // --- score_hour tests ---

    #[test]
    fn score_hour_winch_returns_zero() {
        let l = launch(0.0, 360.0, SiteType::Winch);
        let w = weather(ts(12));
        let (score, reason) = score_hour(&w, &l);
        assert_eq!(score, 0.0);
        assert!(reason.contains("Winch"));
    }

    #[test]
    fn score_hour_precipitation_returns_zero() {
        let l = launch(0.0, 360.0, SiteType::Hang);
        let mut w = weather(ts(12));
        w.precipitation = 2.5;
        let (score, reason) = score_hour(&w, &l);
        assert_eq!(score, 0.0);
        assert!(reason.contains("Precipitation"));
    }

    #[test]
    fn score_hour_wind_at_limit_returns_zero() {
        let l = launch(0.0, 360.0, SiteType::Hang);
        let mut w = weather(ts(12));
        w.wind_speed_ms = MAX_WIND_MS;
        let (score, reason) = score_hour(&w, &l);
        assert_eq!(score, 0.0);
        assert!(reason.contains("too strong"));
    }

    #[test]
    fn score_hour_gust_at_limit_returns_zero() {
        let l = launch(0.0, 360.0, SiteType::Hang);
        let mut w = weather(ts(12));
        w.wind_gust_ms = MAX_GUST_MS;
        let (score, reason) = score_hour(&w, &l);
        assert_eq!(score, 0.0);
        assert!(reason.contains("Gusts"));
    }

    #[test]
    fn score_hour_outside_sector_returns_zero() {
        let l = launch(90.0, 180.0, SiteType::Hang);
        let mut w = weather(ts(12));
        w.wind_direction = 45;
        let (score, reason) = score_hour(&w, &l);
        assert_eq!(score, 0.0);
        assert!(reason.contains("outside launch sector"));
    }

    #[test]
    fn score_hour_good_conditions_returns_positive() {
        let l = launch(90.0, 180.0, SiteType::Hang);
        let mut w = weather(ts(12));
        w.wind_direction = 135;
        w.wind_speed_ms = 5.0;
        w.wind_gust_ms = 7.0;
        let (score, reason) = score_hour(&w, &l);
        assert!(score > 0.0);
        assert!(reason.contains("Ridge") || reason.contains("Thermal"));
    }

    #[test]
    fn score_hour_thermal_wins_for_light_wind() {
        let l = launch(90.0, 180.0, SiteType::Hang);
        let mut w = weather(ts(12));
        w.wind_direction = 135;
        w.wind_speed_ms = 1.0;
        w.cloud_cover = 20;
        let (score, reason) = score_hour(&w, &l);
        assert!(score > 0.0);
        assert!(
            reason.contains("Thermal"),
            "Expected thermal, got: {reason}"
        );
    }

    #[test]
    fn score_hour_ridge_wins_for_strong_wind() {
        let l = launch(90.0, 180.0, SiteType::Hang);
        let mut w = weather(ts(12));
        w.wind_direction = 135;
        w.wind_speed_ms = 6.0;
        w.cloud_cover = 80;
        let (score, reason) = score_hour(&w, &l);
        assert!(score > 0.0);
        assert!(reason.contains("Ridge"), "Expected ridge, got: {reason}");
    }

    #[test]
    fn score_hour_is_flyable_when_score_positive() {
        let l = launch(0.0, 360.0, SiteType::Hang);
        let w = weather(ts(12));
        let (score, _) = score_hour(&w, &l);
        assert!(score > 0.0);
    }

    // --- max_wind_ms conversion ---

    #[test]
    fn max_wind_ms_pins_kmh_to_ms_conversion() {
        assert!((MAX_WIND_MS - 25.0 / 3.6).abs() < 1e-6);
        assert!((MAX_GUST_MS - 40.0 / 3.6).abs() < 1e-6);
    }

    // --- sunrise/sunset filtering ---

    #[tokio::test]
    async fn split_forecast_by_days_filters_out_data_outside_sunrise_sunset() {
        let l = launch(0.0, 360.0, SiteType::Hang);
        let s = site(vec![l]);
        let day = ts(0);

        let forecast = WeatherForecast {
            location: loc(50.0, 13.0),
            forecast: vec![
                weather(day + chrono::Duration::hours(1)),
                weather(day + chrono::Duration::hours(12)),
                weather(day + chrono::Duration::hours(23)),
            ],
        };

        let result = evaluate_site(&s, &forecast).await;
        assert_eq!(result.daily_summaries.len(), 1);
        let day_summary = &result.daily_summaries[0];
        assert_eq!(
            day_summary.hourly_scores.len(),
            1,
            "only the 12:00 entry sits inside June sunrise/sunset; \
             1:00 is before sunrise, 23:00 is after sunset",
        );
        assert_eq!(day_summary.hourly_scores[0].timestamp.hour(), 12);
    }

    // --- FlyableRange grouping tests ---

    fn hourly(hour: u32, score: f32) -> HourlyScore {
        HourlyScore {
            timestamp: ts(hour),
            score,
            is_flyable: score > 0.0,
            reason: String::new(),
        }
    }

    fn summary(scores: Vec<HourlyScore>) -> DailySummary {
        DailySummary {
            date: ts(0).date_naive(),
            hourly_scores: scores,
            ranges: vec![],
        }
    }

    #[test]
    fn all_unflyable_produces_no_ranges() {
        let mut s = summary((6..20).map(|h| hourly(h, 0.0)).collect());
        s.calculate_flyable_time_ranges();
        assert!(s.ranges.is_empty());
    }

    #[test]
    fn single_flyable_hour_produces_one_range() {
        let mut s = summary(vec![hourly(10, 0.8)]);
        s.calculate_flyable_time_ranges();
        assert_eq!(s.ranges.len(), 1);
        assert_eq!(s.ranges[0].start, ts(10));
        assert_eq!(s.ranges[0].end, ts(10));
    }

    #[test]
    fn consecutive_flyable_hours_collapse_into_one_range() {
        let mut s = summary(vec![hourly(10, 0.5), hourly(11, 0.6), hourly(12, 0.7)]);
        s.calculate_flyable_time_ranges();
        assert_eq!(s.ranges.len(), 1);
        assert_eq!(s.ranges[0].start, ts(10));
        assert_eq!(s.ranges[0].end, ts(12));
    }

    #[test]
    fn unflyable_hour_between_flyable_runs_splits_them() {
        let mut s = summary(vec![
            hourly(10, 0.5),
            hourly(11, 0.6),
            hourly(12, 0.0),
            hourly(13, 0.7),
            hourly(14, 0.8),
        ]);
        s.calculate_flyable_time_ranges();
        assert_eq!(s.ranges.len(), 2);
        assert_eq!((s.ranges[0].start, s.ranges[0].end), (ts(10), ts(11)));
        assert_eq!((s.ranges[1].start, s.ranges[1].end), (ts(13), ts(14)));
    }

    #[test]
    fn non_consecutive_flyable_timestamps_produce_separate_ranges() {
        let mut s = summary(vec![hourly(10, 0.5), hourly(13, 0.6)]);
        s.calculate_flyable_time_ranges();
        assert_eq!(s.ranges.len(), 2);
    }

    // --- Integration test ---

    #[tokio::test]
    async fn evaluate_site_emits_single_range_for_contiguous_flyable_window() {
        let l = launch(90.0, 180.0, SiteType::Hang);
        let s = site(vec![l.clone()]);

        let forecast = WeatherForecast {
            location: loc(50.0, 13.0),
            forecast: (4..22)
                .map(|h| {
                    let mut w = weather(ts(h));
                    w.wind_direction = if (10..=14).contains(&h) { 135 } else { 45 };
                    w
                })
                .collect(),
        };

        let result = evaluate_site(&s, &forecast).await;
        assert_eq!(result.daily_summaries.len(), 1);
        let day = &result.daily_summaries[0];
        assert_eq!(day.ranges.len(), 1);
        assert_eq!(day.ranges[0].start, ts(10));
        assert_eq!(day.ranges[0].end, ts(14));
    }
}
