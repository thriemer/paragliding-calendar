use crate::paragliding::{ParaglidingLaunch, ParaglidingSite};
use crate::weather::{self, WeatherData, WeatherForecast};
use chrono::{DateTime, Duration, NaiveDate, Timelike, Utc};
use std::collections::HashMap;
use sunrise::{Coordinates, SolarDay};

#[derive(Debug, Clone)]
pub struct HourlyScore {
    pub timestamp: DateTime<Utc>,
    pub score: u8,
    pub is_flyable: bool,
    pub best_launch_index: Option<usize>,
    pub reasoning: String,
}

#[derive(Debug, Clone)]
pub struct DailySummary {
    pub date: NaiveDate,
    pub hourly_scores: Vec<HourlyScore>,
    pub ranges: Vec<FlyableRange>,
    pub overall_score: u8,
    pub best_hours: Vec<DateTime<Utc>>,
    pub total_flyable_hours: usize,
}

#[derive(Debug, Clone)]
pub struct FlyableRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub avg_score: f32,
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

        // Sort by timestamp
        let mut sorted_scores = self.hourly_scores.clone();
        sorted_scores.sort_by_key(|h| h.timestamp);

        let mut ranges: Vec<FlyableRange> = Vec::new();
        let mut current_range: Option<Vec<&HourlyScore>> = None;

        for score in &sorted_scores {
            match &mut current_range {
                Some(range_scores) => {
                    let last_score = range_scores.last().unwrap();

                    // Check if consecutive
                    if score.timestamp == last_score.timestamp + Duration::hours(1) {
                        range_scores.push(score);
                    } else {
                        // Finalize current range
                        if !range_scores.is_empty() {
                            let start = range_scores.first().unwrap().timestamp;
                            let end = range_scores.last().unwrap().timestamp;
                            let avg = range_scores.iter().map(|s| s.score as f32).sum::<f32>()
                                / range_scores.len() as f32;

                            ranges.push(FlyableRange {
                                start,
                                end,
                                avg_score: avg,
                            });
                        }

                        // Start new range
                        current_range = Some(vec![score]);
                    }
                }
                None => {
                    // Start first range
                    current_range = Some(vec![score]);
                }
            }
        }

        // Handle the last range
        if let Some(range_scores) = current_range {
            if !range_scores.is_empty() {
                let start = range_scores.first().unwrap().timestamp;
                let end = range_scores.last().unwrap().timestamp;
                let avg = range_scores.iter().map(|s| s.score as f32).sum::<f32>()
                    / range_scores.len() as f32;

                ranges.push(FlyableRange {
                    start,
                    end,
                    avg_score: avg,
                });
            }
        }

        self.ranges = ranges;
    }
}

#[derive(Debug, Clone)]
pub struct SiteEvaluationResult {
    pub daily_summaries: Vec<DailySummary>,
}

fn wind_in_launch_range(wind_direction: u16, launch: &ParaglidingLaunch) -> (bool, u8, String) {
    let wind_deg = wind_direction as f64;
    let start = launch.direction_degrees_start;
    let stop = launch.direction_degrees_stop;

    let in_range = if start <= stop {
        wind_deg >= start && wind_deg <= stop
    } else {
        wind_deg >= start || wind_deg <= stop
    };

    if !in_range {
        return (
            false,
            0,
            format!(
                "Wind direction {}° not suitable for launch (range: {:.0}°-{:.0}°)",
                wind_direction, start, stop
            ),
        );
    }

    let center = if start <= stop {
        (start + stop) / 2.0
    } else {
        let mid = (start + stop + 360.0) / 2.0;
        if mid >= 360.0 { mid - 360.0 } else { mid }
    };

    let diff = (wind_deg - center).abs();
    let diff = if diff > 180.0 { 360.0 - diff } else { diff };

    let range_size = if start <= stop {
        stop - start
    } else {
        360.0 - start + stop
    };

    let max_diff = range_size / 2.0;
    let score_ratio = 1.0 - (diff / max_diff);
    let score = (score_ratio * 100.0) as u8;

    let reasoning = format!(
        "Wind {}° within launch range {:.0}°-{:.0}° (score: {})",
        wind_direction, start, stop, score
    );

    (true, score, reasoning)
}

fn calculate_wind_speed_score(wind_speed_kmh: f32) -> u8 {
    let ideal_speed = 10.0;
    let deviation = (wind_speed_kmh - ideal_speed).abs();
    let score = 100.0 - (deviation * 5.0);
    score.max(0.0) as u8
}

fn is_safe_to_fly(weather: &WeatherData) -> (bool, u8, String) {
    let wind_speed_kmh = weather.wind_speed_ms * 3.6;
    let gust_speed_kmh = weather.wind_gust_ms * 3.6;

    if gust_speed_kmh > 40.0 {
        return (
            false,
            0,
            format!(
                "Wind gusts too high: {:.1} km/h (max 40 km/h)",
                gust_speed_kmh
            ),
        );
    }

    let wind_speed_score = calculate_wind_speed_score(wind_speed_kmh);
    let reasoning = format!(
        "Wind speed: {:.1} km/h (score: {}), gusts: {:.1} km/h",
        wind_speed_kmh, wind_speed_score, gust_speed_kmh
    );

    (true, wind_speed_score, reasoning)
}

pub fn evaluate_site(site: &ParaglidingSite, forecast: &WeatherForecast) -> SiteEvaluationResult {
    let daily_forecasts = split_forecast_by_days(forecast.clone());
    let mut daily_summaries = Vec::new();

    for daily_forecast in daily_forecasts {
        if daily_forecast.forecast.is_empty() {
            continue;
        }

        let date = daily_forecast.forecast[0].timestamp.date_naive();
        let mut hourly_scores = Vec::new();

        for weather_data in &daily_forecast.forecast {
            let (is_safe, wind_speed_score, safety_reason) = is_safe_to_fly(weather_data);

            let (score, best_launch_index, reasoning) = if !is_safe {
                (0, None, safety_reason)
            } else {
                let mut best_direction_score = 0;
                let mut best_index = None;
                let mut best_reasoning = String::new();

                for (i, launch) in site.launches.iter().enumerate() {
                    let (in_range, direction_score, launch_reason) =
                        wind_in_launch_range(weather_data.wind_direction, launch);
                    if in_range && direction_score > best_direction_score {
                        best_direction_score = direction_score;
                        best_index = Some(i);
                        best_reasoning = launch_reason;
                    }
                }

                if best_direction_score == 0 {
                    (
                        0,
                        None,
                        format!(
                            "{}. No suitable launch for wind direction {}°",
                            safety_reason, weather_data.wind_direction
                        ),
                    )
                } else {
                    let final_score = (best_direction_score as u32 + wind_speed_score as u32) / 2;
                    let combined_reasoning = format!(
                        "{}. {}. Final score: {} (avg of direction: {}, speed: {})",
                        safety_reason,
                        best_reasoning,
                        final_score,
                        best_direction_score,
                        wind_speed_score
                    );
                    (final_score as u8, best_index, combined_reasoning)
                }
            };

            hourly_scores.push(HourlyScore {
                timestamp: weather_data.timestamp,
                score,
                is_flyable: score > 0,
                best_launch_index,
                reasoning,
            });
        }

        let daily_summary = calculate_daily_summary(date, hourly_scores);
        daily_summaries.push(daily_summary);
    }

    SiteEvaluationResult { daily_summaries }
}

fn split_forecast_by_days(forecast: WeatherForecast) -> Vec<WeatherForecast> {
    let mut daily_forecasts: HashMap<NaiveDate, Vec<WeatherData>> = HashMap::new();

    // Group hourly data by date
    for weather_data in forecast.forecast {
        let date = weather_data.timestamp.date_naive();
        daily_forecasts
            .entry(date)
            .or_insert_with(Vec::new)
            .push(weather_data);
    }

    // Calculate daylight hours once per day and filter
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

    let overall_score = if hourly_scores.is_empty() {
        0
    } else {
        hourly_scores.iter().map(|h| h.score as u32).sum::<u32>() / hourly_scores.len() as u32
    } as u8;

    let best_hours: Vec<DateTime<Utc>> = hourly_scores
        .iter()
        .filter(|h| h.score >= 80)
        .map(|h| h.timestamp)
        .collect();

    DailySummary {
        date,
        hourly_scores,
        overall_score,
        best_hours,
        total_flyable_hours,
        ranges: vec![],
    }
}
