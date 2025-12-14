use chrono::{DateTime, Utc, Timelike};
use crate::models::{ParaglidingSite, WeatherData, ParaglidingLaunch};
use crate::models::weather::WeatherForecast;
use crate::weather::get_sunrise_sunset;

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
    pub overall_score: u8,
    pub best_hours: Vec<DateTime<Utc>>,
    pub total_flyable_hours: usize,
}

#[derive(Debug, Clone)]
pub struct SiteEvaluationResult {
    pub hourly_scores: Vec<HourlyScore>,
    pub daily_summary: DailySummary,
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
        return (false, 0, format!("Wind direction {}° not suitable for launch (range: {:.0}°-{:.0}°)", wind_direction, start, stop));
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
    
    let reasoning = format!("Wind {}° within launch range {:.0}°-{:.0}° (score: {})", wind_direction, start, stop, score);
    
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
        return (false, 0, format!("Wind gusts too high: {:.1} km/h (max 40 km/h)", gust_speed_kmh));
    }
    
    let wind_speed_score = calculate_wind_speed_score(wind_speed_kmh);
    let reasoning = format!("Wind speed: {:.1} km/h (score: {}), gusts: {:.1} km/h", 
                          wind_speed_kmh, wind_speed_score, gust_speed_kmh);
    
    (true, wind_speed_score, reasoning)
}

pub fn evaluate_site(site: &ParaglidingSite, forecast: &WeatherForecast) -> SiteEvaluationResult {
    let mut hourly_scores = Vec::new();
    
    // Get sunrise/sunset for the first day of forecast
    let (daylight_start_hour, daylight_end_hour) = if let Some(first_weather) = forecast.forecast.first() {
        let date = first_weather.timestamp.date_naive();
        if let Ok((sunrise, sunset)) = get_sunrise_sunset(&forecast.location, date) {
            (sunrise.hour(), sunset.hour())
        } else {
            // Fallback: assume 6 AM to 8 PM if calculation fails
            (6, 20)
        }
    } else {
        return SiteEvaluationResult {
            hourly_scores: Vec::new(),
            daily_summary: DailySummary {
                overall_score: 0,
                best_hours: Vec::new(),
                total_flyable_hours: 0,
            },
        };
    };
    
    for weather_data in &forecast.forecast {
        // Skip nighttime hours
        let hour = weather_data.timestamp.hour();
        if hour < daylight_start_hour || hour > daylight_end_hour {
            continue;
        }
        let (is_safe, wind_speed_score, safety_reason) = is_safe_to_fly(weather_data);
        
        let (score, best_launch_index, reasoning) = if !is_safe {
            (0, None, safety_reason)
        } else {
            let mut best_direction_score = 0;
            let mut best_index = None;
            let mut best_reasoning = String::new();
            
            for (i, launch) in site.launches.iter().enumerate() {
                let (in_range, direction_score, launch_reason) = wind_in_launch_range(weather_data.wind_direction, launch);
                if in_range && direction_score > best_direction_score {
                    best_direction_score = direction_score;
                    best_index = Some(i);
                    best_reasoning = launch_reason;
                }
            }
            
            if best_direction_score == 0 {
                (0, None, format!("{}. No suitable launch for wind direction {}°", safety_reason, weather_data.wind_direction))
            } else {
                let final_score = (best_direction_score as u32 + wind_speed_score as u32) / 2;
                let combined_reasoning = format!("{}. {}. Final score: {} (avg of direction: {}, speed: {})", 
                                               safety_reason, best_reasoning, final_score, best_direction_score, wind_speed_score);
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
    
    let daily_summary = calculate_daily_summary(&hourly_scores);
    
    SiteEvaluationResult {
        hourly_scores,
        daily_summary,
    }
}

fn calculate_daily_summary(hourly_scores: &[HourlyScore]) -> DailySummary {
    let flyable_hours: Vec<_> = hourly_scores.iter()
        .filter(|h| h.is_flyable)
        .collect();
    
    let overall_score = if hourly_scores.is_empty() {
        0
    } else {
        hourly_scores.iter().map(|h| h.score as u32).sum::<u32>() / hourly_scores.len() as u32
    } as u8;
    
    let best_hours: Vec<DateTime<Utc>> = hourly_scores.iter()
        .filter(|h| h.score >= 80)
        .map(|h| h.timestamp)
        .collect();
    
    DailySummary {
        overall_score,
        best_hours,
        total_flyable_hours: flyable_hours.len(),
    }
}
