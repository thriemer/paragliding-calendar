use std::collections::HashMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};

use crate::domain::{
    paragliding::{ParaglidingLaunch, ParaglidingSite, SiteType},
    weather::{self, WeatherData, WeatherForecast},
};

#[derive(Debug, Clone)]
pub struct HourlyScore {
    pub timestamp: DateTime<Utc>,
    pub is_flyable: bool,
}

#[derive(Debug, Clone)]
pub struct DailySummary {
    pub date: NaiveDate,
    pub hourly_scores: Vec<HourlyScore>,
    pub ranges: Vec<FlyableRange>,
    pub total_flyable_hours: usize,
}

#[derive(Debug, Clone)]
pub struct FlyableRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl FlyableRange {
    pub fn is_at_least(&self, d: Duration) -> bool {
        (self.end - self.start) >= d
    }
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

fn is_flyable(weather: &WeatherData, launch: &ParaglidingLaunch) -> bool {
    if !matches!(launch.site_type, SiteType::Hang) {
        return false;
    }
    if weather.precipitation != 0.0 {
        return false;
    }
    if weather.wind_speed_ms >= MAX_WIND_MS {
        return false;
    }
    if weather.wind_gust_ms >= MAX_GUST_MS {
        return false;
    }
    wind_direction_in_sector(
        weather.wind_direction as f64,
        launch.direction_degrees_start,
        launch.direction_degrees_stop,
    )
}

fn wind_direction_in_sector(wind_dir: f64, start: f64, stop: f64) -> bool {
    // start == stop is the conventional way to say "launchable from any direction"
    // (e.g. a flat-top site). Without this branch the strict-< sector check would
    // reject every wind, since `start < wind && wind < start` is never true.
    if start == stop {
        return true;
    }
    if start < stop {
        start < wind_dir && wind_dir < stop
    } else {
        start < wind_dir || wind_dir < stop
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
            let any_flyable = site
                .launches
                .iter()
                .any(|launch| is_flyable(weather_data, launch));

            hourly_scores.push(HourlyScore {
                timestamp: weather_data.timestamp,
                is_flyable: any_flyable,
            });
        }

        let mut daily_summary = calculate_daily_summary(date, hourly_scores);
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

fn calculate_daily_summary(date: NaiveDate, hourly_scores: Vec<HourlyScore>) -> DailySummary {
    let total_flyable_hours = hourly_scores.iter().filter(|h| h.is_flyable).count();

    DailySummary {
        date,
        hourly_scores,
        total_flyable_hours,
        ranges: vec![],
    }
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
            visibility: 10.0,
            description: String::new(),
        }
    }

    fn ts(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 13, hour, 0, 0).unwrap()
    }

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

    #[rstest]
    #[case(Duration::hours(1), Duration::hours(2), false)]
    #[case(Duration::hours(2), Duration::hours(2), true)]
    #[case(Duration::hours(3), Duration::hours(2), true)]
    fn flyable_range_is_at_least_is_inclusive(
        #[case] range_len: Duration,
        #[case] threshold: Duration,
        #[case] expected: bool,
    ) {
        let r = FlyableRange {
            start: ts(10),
            end: ts(10) + range_len,
        };
        assert_eq!(r.is_at_least(threshold), expected);
    }

    #[test]
    fn is_flyable_accepts_wind_speed_just_below_limit() {
        let l = launch(0.0, 360.0, SiteType::Hang);
        let mut w = weather(ts(12));
        w.wind_direction = 180;
        w.wind_speed_ms = MAX_WIND_MS - 0.01;
        w.wind_gust_ms = MAX_GUST_MS - 0.01;
        assert!(is_flyable(&w, &l));
    }

    #[test]
    fn is_flyable_rejects_wind_speed_just_at_limit() {
        let l = launch(0.0, 360.0, SiteType::Hang);
        let mut w = weather(ts(12));
        w.wind_speed_ms = MAX_WIND_MS;
        assert!(!is_flyable(&w, &l));
    }

    #[test]
    fn is_flyable_rejects_wind_gust_just_at_limit() {
        let l = launch(0.0, 360.0, SiteType::Hang);
        let mut w = weather(ts(12));
        w.wind_gust_ms = MAX_GUST_MS;
        assert!(!is_flyable(&w, &l));
    }

    #[test]
    fn max_wind_ms_pins_kmh_to_ms_conversion() {
        assert!((MAX_WIND_MS - 25.0 / 3.6).abs() < 1e-6);
        assert!((MAX_GUST_MS - 40.0 / 3.6).abs() < 1e-6);
    }

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
        assert_eq!(
            day_summary.hourly_scores[0].timestamp.hour(),
            12,
        );
    }

    #[test]
    fn is_flyable_winch_site_never_flyable() {
        let l = launch(0.0, 360.0, SiteType::Winch);
        let w = weather(ts(12));
        assert!(!is_flyable(&w, &l));
    }

    #[test]
    fn is_flyable_rejects_precipitation() {
        let l = launch(0.0, 360.0, SiteType::Hang);
        let mut w = weather(ts(12));
        w.precipitation = 0.1;
        assert!(!is_flyable(&w, &l));
    }

    #[test]
    fn is_flyable_rejects_wind_at_limit() {
        let l = launch(0.0, 360.0, SiteType::Hang);
        let mut w = weather(ts(12));
        w.wind_speed_ms = MAX_WIND_MS;
        assert!(!is_flyable(&w, &l));
    }

    #[test]
    fn is_flyable_rejects_gust_at_limit() {
        let l = launch(0.0, 360.0, SiteType::Hang);
        let mut w = weather(ts(12));
        w.wind_gust_ms = MAX_GUST_MS;
        assert!(!is_flyable(&w, &l));
    }

    #[test]
    fn is_flyable_rejects_wind_outside_sector() {
        let l = launch(90.0, 180.0, SiteType::Hang);
        let mut w = weather(ts(12));
        w.wind_direction = 45;
        assert!(!is_flyable(&w, &l));
    }

    #[test]
    fn is_flyable_accepts_when_all_conditions_met() {
        let l = launch(90.0, 180.0, SiteType::Hang);
        let mut w = weather(ts(12));
        w.wind_direction = 135;
        w.wind_speed_ms = 3.0;
        w.wind_gust_ms = 5.0;
        w.precipitation = 0.0;
        assert!(is_flyable(&w, &l));
    }

    fn hourly(hour: u32, is_flyable: bool) -> HourlyScore {
        HourlyScore {
            timestamp: ts(hour),
            is_flyable,
        }
    }

    fn summary(scores: Vec<HourlyScore>) -> DailySummary {
        DailySummary {
            date: ts(0).date_naive(),
            hourly_scores: scores,
            ranges: vec![],
            total_flyable_hours: 0,
        }
    }

    #[test]
    fn all_unflyable_produces_no_ranges() {
        let mut s = summary((6..20).map(|h| hourly(h, false)).collect());
        s.calculate_flyable_time_ranges();
        assert!(s.ranges.is_empty());
    }

    #[test]
    fn single_flyable_hour_produces_one_range() {
        let mut s = summary(vec![hourly(10, true)]);
        s.calculate_flyable_time_ranges();
        assert_eq!(s.ranges.len(), 1);
        assert_eq!(s.ranges[0].start, ts(10));
        assert_eq!(s.ranges[0].end, ts(10));
    }

    #[test]
    fn consecutive_flyable_hours_collapse_into_one_range() {
        let mut s = summary(vec![hourly(10, true), hourly(11, true), hourly(12, true)]);
        s.calculate_flyable_time_ranges();
        assert_eq!(s.ranges.len(), 1);
        assert_eq!(s.ranges[0].start, ts(10));
        assert_eq!(s.ranges[0].end, ts(12));
    }

    #[test]
    fn unflyable_hour_between_flyable_runs_splits_them() {
        let mut s = summary(vec![
            hourly(10, true),
            hourly(11, true),
            hourly(12, false),
            hourly(13, true),
            hourly(14, true),
        ]);
        s.calculate_flyable_time_ranges();
        assert_eq!(s.ranges.len(), 2);
        assert_eq!((s.ranges[0].start, s.ranges[0].end), (ts(10), ts(11)));
        assert_eq!((s.ranges[1].start, s.ranges[1].end), (ts(13), ts(14)));
    }

    #[test]
    fn non_consecutive_flyable_timestamps_produce_separate_ranges() {
        let mut s = summary(vec![hourly(10, true), hourly(13, true)]);
        s.calculate_flyable_time_ranges();
        assert_eq!(s.ranges.len(), 2);
    }

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
