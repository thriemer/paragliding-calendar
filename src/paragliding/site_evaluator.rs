use std::collections::HashMap;

use chrono::{DateTime, Duration, NaiveDate, Timelike, Utc};
use serde::Serialize;
use serde_json::json;
use sunrise::{Coordinates, SolarDay};
use zen_engine::{DecisionEngine, model::DecisionContent};

use crate::{
    paragliding::{ParaglidingLaunch, ParaglidingSite},
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

                            ranges.push(FlyableRange { start, end });
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

                ranges.push(FlyableRange { start, end });
            }
        }

        self.ranges = ranges;
    }
}

#[derive(Debug, Clone)]
pub struct SiteEvaluationResult {
    pub daily_summaries: Vec<DailySummary>,
}

#[derive(Debug, Serialize)]
struct LaunchWeather {
    weather: WeatherData,
    launch: ParaglidingLaunch,
}

async fn is_flyable_decision_evaluation(weather: &WeatherData, site: &ParaglidingLaunch) -> bool {
    let lw = LaunchWeather {
        weather: weather.clone(),
        launch: site.clone(),
    };

    let decision_content: DecisionContent =
        serde_json::from_str(include_str!("flyable_decision_graph.json")).unwrap();
    let engine = DecisionEngine::default();
    let decision = engine.create_decision(decision_content.into());

    let result = decision
        .evaluate(serde_json::to_value(&lw).unwrap().into())
        .await
        .unwrap();

    let flyable = result.result.dot("flyable").unwrap().as_bool().unwrap();
    flyable
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
            let mut any_flyable = false;
            for launch in site.launches.iter() {
                let flyable = is_flyable_decision_evaluation(&weather_data, &launch).await;
                any_flyable = any_flyable | flyable;
                if any_flyable {
                    break;
                }
            }

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

    DailySummary {
        date,
        hourly_scores,
        total_flyable_hours,
        ranges: vec![],
    }
}
