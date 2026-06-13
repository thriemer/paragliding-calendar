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
    pub fn is_longer_than(&self, d: Duration) -> bool {
        (self.end - self.start) > d
    }
}

impl DailySummary {
    pub fn calculate_flyable_time_ranges(&mut self) {
        self.ranges.clear();

        if self.hourly_scores.is_empty() {
            return;
        }

        let mut sorted_scores = self.hourly_scores.clone();
        sorted_scores.sort_by_key(|h| h.timestamp);

        let mut ranges: Vec<FlyableRange> = Vec::new();
        let mut current_range: Option<Vec<&HourlyScore>> = None;

        for score in &sorted_scores {
            match &mut current_range {
                Some(range_scores) => {
                    let last_score = range_scores.last().unwrap();

                    if score.timestamp == last_score.timestamp + Duration::hours(1) {
                        range_scores.push(score);
                    } else {
                        if !range_scores.is_empty() {
                            let start = range_scores.first().unwrap().timestamp;
                            let end = range_scores.last().unwrap().timestamp;

                            ranges.push(FlyableRange { start, end });
                        }

                        current_range = Some(vec![score]);
                    }
                }
                None => {
                    current_range = Some(vec![score]);
                }
            }
        }

        if let Some(range_scores) = current_range
            && !range_scores.is_empty()
        {
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

        let daily_summary = calculate_daily_summary(date, hourly_scores);
        daily_summaries.push(daily_summary);
    }

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
