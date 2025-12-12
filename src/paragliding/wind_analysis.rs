//! Wind Analysis Engine for Paragliding Site Evaluation
//!
//! This module provides comprehensive wind analysis capabilities for paragliding sites,
//! evaluating wind conditions against launch orientations to determine flyability.

use crate::models::WeatherData;
use crate::paragliding::sites::{LaunchDirectionRange, ParaglidingSite};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Wind direction analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindDirectionAnalysis {
    /// Wind direction in degrees from north (0-360)
    pub wind_direction_deg: u16,
    /// Wind direction as cardinal direction (N, NE, etc.)
    pub wind_direction_cardinal: String,
    /// Angular difference from optimal launch direction(s)
    pub angular_differences: Vec<f64>,
    /// Best matching launch direction
    pub best_launch_direction: Option<LaunchDirectionRange>,
    /// Wind direction compatibility rating
    pub direction_compatibility: WindDirectionCompatibility,
}

/// Wind speed analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindSpeedAnalysis {
    /// Wind speed in km/h (converted from m/s)
    pub wind_speed_kmh: f32,
    /// Wind speed in m/s (original)
    pub wind_speed_ms: f32,
    /// Wind gust speed in km/h
    pub wind_gust_kmh: f32,
    /// Wind speed category
    pub speed_category: WindSpeedCategory,
    /// Suitability for different pilot skill levels
    pub pilot_suitability: PilotSuitability,
}

/// Complete flyability analysis combining all factors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlyabilityAnalysis {
    /// Site being analyzed
    pub site_id: String,
    /// Wind direction analysis
    pub wind_direction: WindDirectionAnalysis,
    /// Wind speed analysis
    pub wind_speed: WindSpeedAnalysis,
    /// Safety margin assessment
    pub safety_margins: SafetyMargins,
    /// Final flyability score (0-10)
    pub flyability_score: f32,
    /// Human-readable explanation
    pub explanation: String,
    /// Detailed reasoning for the score
    pub reasoning: Vec<String>,
}

/// Wind direction compatibility levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WindDirectionCompatibility {
    /// Perfect alignment (±20°)
    Perfect,
    /// Favorable conditions (±45°)
    Favorable,
    /// Marginal conditions (±90°)
    Marginal,
    /// Unfavorable but potentially flyable (>90°)
    Unfavorable,
    /// Dangerous conditions (strong tailwind)
    Dangerous,
}

/// Wind speed categories for paragliding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WindSpeedCategory {
    /// 0-10 km/h - Light winds
    Light,
    /// 10-15 km/h - Moderate winds
    Moderate,
    /// 15-20 km/h - Strong winds
    Strong,
    /// >20 km/h - Dangerous winds
    Dangerous,
}

/// Pilot suitability based on skill level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PilotSuitability {
    /// Suitable for beginner pilots
    pub beginner: bool,
    /// Suitable for intermediate pilots
    pub intermediate: bool,
    /// Suitable for advanced pilots
    pub advanced: bool,
}

/// Safety margin calculations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyMargins {
    /// Uncertainty buffer applied (degrees)
    pub direction_uncertainty: f64,
    /// Speed safety factor applied
    pub speed_safety_factor: f32,
    /// Forecast confidence adjustment
    pub forecast_confidence: f32,
    /// Time-based degradation factor
    pub time_degradation: f32,
}

impl WindDirectionAnalysis {
    /// Analyze wind direction against site launch directions
    #[must_use] 
    pub fn analyze(weather: &WeatherData, site: &ParaglidingSite) -> Self {
        let wind_direction_deg = weather.wind_direction;
        let wind_direction_cardinal = crate::models::WeatherData::wind_direction_to_cardinal(wind_direction_deg).to_string();
        
        let mut angular_differences = Vec::new();
        let mut best_launch_direction = None;
        let mut min_difference = f64::INFINITY;

        // Calculate angular differences for each launch direction
        for launch_dir in &site.launch_directions {
            // Check both start and stop angles of the range
            let launch_degrees = [launch_dir.direction_degrees_start, launch_dir.direction_degrees_stop];
            
            for &launch_deg in &launch_degrees {
                let diff = calculate_angular_difference(f64::from(wind_direction_deg), launch_deg);
                angular_differences.push(diff);
                
                if diff < min_difference {
                    min_difference = diff;
                    best_launch_direction = Some(launch_dir.clone());
                }
            }
            
            // Also check if wind direction is within the range
            let wind_deg = f64::from(wind_direction_deg);
            if is_angle_in_range(wind_deg, launch_dir.direction_degrees_start, launch_dir.direction_degrees_stop) {
                angular_differences.push(0.0); // Perfect match - within range
                min_difference = 0.0;
                best_launch_direction = Some(launch_dir.clone());
            }
        }

        let direction_compatibility = determine_direction_compatibility(min_difference);

        Self {
            wind_direction_deg,
            wind_direction_cardinal,
            angular_differences,
            best_launch_direction,
            direction_compatibility,
        }
    }
}

impl WindSpeedAnalysis {
    /// Analyze wind speed for paragliding suitability
    #[must_use] 
    pub fn analyze(weather: &WeatherData) -> Self {
        let wind_speed_ms = weather.wind_speed;
        let wind_speed_kmh = wind_speed_ms * 3.6; // Convert m/s to km/h
        let wind_gust_kmh = weather.wind_gust * 3.6;
        
        // Check if gusts make it dangerous even if normal speed is ok
        let mut speed_category = determine_speed_category(wind_speed_kmh);
        if wind_gust_kmh > 40.0 {
            speed_category = WindSpeedCategory::Dangerous;
        }
        
        let pilot_suitability = determine_pilot_suitability(wind_speed_kmh, wind_gust_kmh);

        Self {
            wind_speed_kmh,
            wind_speed_ms,
            wind_gust_kmh,
            speed_category,
            pilot_suitability,
        }
    }
}

impl SafetyMargins {
    /// Calculate safety margins based on forecast and conditions
    #[must_use] 
    pub fn calculate(hours_ahead: f32) -> Self {
        // Uncertainty increases over time
        let direction_uncertainty = (f64::from(hours_ahead) * 2.0).min(15.0); // Max 15° uncertainty
        let speed_safety_factor = 0.8; // 20% safety margin on wind speed
        let forecast_confidence = 1.0 - (hours_ahead / 72.0).min(0.3); // Confidence decreases over 72h
        let time_degradation = 1.0 - (hours_ahead / 168.0).min(0.2); // Degrades over week

        Self {
            direction_uncertainty,
            speed_safety_factor,
            forecast_confidence,
            time_degradation,
        }
    }
}

impl FlyabilityAnalysis {
    /// Perform complete flyability analysis
    #[must_use] 
    pub fn analyze(weather: &WeatherData, site: &ParaglidingSite, hours_ahead: f32) -> Self {
        let wind_direction = WindDirectionAnalysis::analyze(weather, site);
        let wind_speed = WindSpeedAnalysis::analyze(weather);
        let safety_margins = SafetyMargins::calculate(hours_ahead);

        let (flyability_score, explanation, reasoning) = 
            calculate_flyability_score(&wind_direction, &wind_speed, &safety_margins);

        Self {
            site_id: site.id.clone(),
            wind_direction,
            wind_speed,
            safety_margins,
            flyability_score,
            explanation,
            reasoning,
        }
    }

    /// Check if conditions are flyable (score >= 5.0)
    #[must_use] 
    pub fn is_flyable(&self) -> bool {
        self.flyability_score >= 5.0
    }

    /// Get color-coded score representation
    #[must_use] 
    pub fn score_color(&self) -> &'static str {
        match self.flyability_score.clamp(0.0, 10.0).round() as u8 {
            9..=10 => "🟢", // Green - Excellent
            7..=8 => "🟡",  // Yellow - Good  
            5..=6 => "🟠",  // Orange - Marginal
            3..=4 => "🔴",  // Red - Poor
            _ => "⚫",       // Black - Dangerous
        }
    }
}

/// Calculate angular difference between two directions (0-180°)
fn calculate_angular_difference(wind_deg: f64, launch_deg: f64) -> f64 {
    let diff = (wind_deg - launch_deg).abs();
    diff.min(360.0 - diff)
}

/// Check if an angle is within a directional range, handling 360-degree wraparound
fn is_angle_in_range(angle: f64, start: f64, stop: f64) -> bool {
    let normalize = |a: f64| a.rem_euclid(360.0);
    let angle = normalize(angle);
    let start = normalize(start);
    let stop = normalize(stop);
    
    if start <= stop {
        angle >= start && angle <= stop
    } else {
        // Range wraps around 360/0 degrees
        angle >= start || angle <= stop
    }
}

/// Determine wind direction compatibility based on angular difference
fn determine_direction_compatibility(min_difference: f64) -> WindDirectionCompatibility {
    match min_difference {
        d if d <= 20.0 => WindDirectionCompatibility::Perfect,
        d if d <= 45.0 => WindDirectionCompatibility::Favorable,
        d if d <= 90.0 => WindDirectionCompatibility::Marginal,
        d if d <= 150.0 => WindDirectionCompatibility::Unfavorable,
        _ => WindDirectionCompatibility::Dangerous,
    }
}

/// Determine wind speed category
fn determine_speed_category(wind_speed_kmh: f32) -> WindSpeedCategory {
    match wind_speed_kmh {
        s if s <= 10.0 => WindSpeedCategory::Light,
        s if s <= 15.0 => WindSpeedCategory::Moderate,
        s if s <= 20.0 => WindSpeedCategory::Strong,
        _ => WindSpeedCategory::Dangerous,
    }
}

/// Determine pilot suitability based on wind conditions
fn determine_pilot_suitability(wind_speed_kmh: f32, wind_gust_kmh: f32) -> PilotSuitability {
    let beginner = wind_speed_kmh <= 10.0 && wind_gust_kmh <= 15.0;
    let intermediate = wind_speed_kmh <= 15.0 && wind_gust_kmh <= 25.0;
    let advanced = wind_speed_kmh <= 30.0 && wind_gust_kmh <= 40.0;

    PilotSuitability {
        beginner,
        intermediate,
        advanced,
    }
}

/// Calculate overall flyability score combining all factors
fn calculate_flyability_score(
    direction: &WindDirectionAnalysis,
    speed: &WindSpeedAnalysis,
    safety: &SafetyMargins,
) -> (f32, String, Vec<String>) {
    let mut reasoning = Vec::new();

    // Direction scoring
    let direction_score = match direction.direction_compatibility {
        WindDirectionCompatibility::Perfect => {
            reasoning.push("Perfect wind direction alignment".to_string());
            10.0
        }
        WindDirectionCompatibility::Favorable => {
            reasoning.push("Favorable wind direction".to_string());
            8.0
        }
        WindDirectionCompatibility::Marginal => {
            reasoning.push("Marginal wind direction - crosswind conditions".to_string());
            6.0
        }
        WindDirectionCompatibility::Unfavorable => {
            reasoning.push("Unfavorable wind direction".to_string());
            3.0
        }
        WindDirectionCompatibility::Dangerous => {
            reasoning.push("Dangerous wind direction - strong tailwind".to_string());
            0.0
        }
    };

    // Speed scoring
    let speed_score = match speed.speed_category {
        WindSpeedCategory::Light => {
            reasoning.push("Light winds - good for all skill levels".to_string());
            9.0
        }
        WindSpeedCategory::Moderate => {
            reasoning.push("Moderate winds - suitable for most pilots".to_string());
            8.0
        }
        WindSpeedCategory::Strong => {
            reasoning.push("Strong winds - experienced pilots only".to_string());
            5.0
        }
        WindSpeedCategory::Dangerous => {
            reasoning.push("Dangerous wind speeds".to_string());
            0.0
        }
    };

    // Apply safety margins
    let safety_factor = safety.forecast_confidence * safety.time_degradation;
    if safety_factor < 0.8 {
        reasoning.push("Reduced confidence due to forecast uncertainty".to_string());
    }

    // Combine scores (weighted average) - but if either direction or speed is dangerous, cap the score
    let score = if matches!(direction.direction_compatibility, WindDirectionCompatibility::Dangerous) ||
       matches!(speed.speed_category, WindSpeedCategory::Dangerous) {
        0.0
    } else {
        (direction_score * 0.6 + speed_score * 0.4) * safety_factor
    };

    let explanation = match score.clamp(0.0, 10.0).round() as u8 {
        9..=10 => "Excellent flying conditions".to_string(),
        7..=8 => "Good flying conditions".to_string(),
        5..=6 => "Marginal conditions - proceed with caution".to_string(),
        3..=4 => "Poor conditions - not recommended".to_string(),
        _ => "Dangerous conditions - do not fly".to_string(),
    };

    (score, explanation, reasoning)
}

// Display implementations
impl fmt::Display for WindDirectionCompatibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WindDirectionCompatibility::Perfect => write!(f, "Perfect"),
            WindDirectionCompatibility::Favorable => write!(f, "Favorable"),
            WindDirectionCompatibility::Marginal => write!(f, "Marginal"),
            WindDirectionCompatibility::Unfavorable => write!(f, "Unfavorable"),
            WindDirectionCompatibility::Dangerous => write!(f, "Dangerous"),
        }
    }
}

impl fmt::Display for WindSpeedCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WindSpeedCategory::Light => write!(f, "Light"),
            WindSpeedCategory::Moderate => write!(f, "Moderate"),
            WindSpeedCategory::Strong => write!(f, "Strong"),
            WindSpeedCategory::Dangerous => write!(f, "Dangerous"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::WeatherData;
    use crate::paragliding::sites::{Coordinates, DataSource, LaunchDirectionRange, ParaglidingSite, SiteCharacteristics, SiteType};
    use chrono::Utc;

    fn create_test_weather(wind_direction: u16, wind_speed: f32) -> WeatherData {
        WeatherData {
            timestamp: Utc::now(),
            temperature: 15.0,
            wind_speed,
            wind_direction,
            wind_gust: wind_speed * 1.2,
            precipitation: 0.0,
            cloud_cover: 20,
            pressure: 1013.0,
            visibility: 10.0,
            description: "Clear".to_string(),
            icon: None,
        }
    }

    fn create_test_site() -> ParaglidingSite {
        ParaglidingSite {
            id: "test_site".to_string(),
            name: "Test Site".to_string(),
            coordinates: Coordinates {
                latitude: 46.0,
                longitude: 8.0,
            },
            elevation: Some(1500.0),
            launch_directions: vec![
                LaunchDirectionRange {
                    direction_degrees_start: 337.5, // North range (337.5-22.5)
                    direction_degrees_stop: 22.5,
                },
                LaunchDirectionRange {
                    direction_degrees_start: 157.5, // South range (157.5-202.5)
                    direction_degrees_stop: 202.5,
                },
            ],
            site_type: SiteType::Hang,
            country: Some("CH".to_string()),
            data_source: DataSource::DHV,
            characteristics: SiteCharacteristics {
                height_difference_max: Some(800.0),
                site_url: None,
                access_by_car: Some(true),
                access_by_foot: Some(true),
                access_by_public_transport: Some(false),
                hanggliding: Some(true),
                paragliding: Some(true),
            },
        }
    }

    #[test]
    fn test_angular_difference() {
        assert_eq!(calculate_angular_difference(0.0, 0.0), 0.0);
        assert_eq!(calculate_angular_difference(0.0, 90.0), 90.0);
        assert_eq!(calculate_angular_difference(0.0, 180.0), 180.0);
        assert_eq!(calculate_angular_difference(0.0, 270.0), 90.0);
        assert_eq!(calculate_angular_difference(0.0, 350.0), 10.0);
        assert_eq!(calculate_angular_difference(10.0, 350.0), 20.0);
    }

    #[test]
    fn test_wind_direction_analysis() {
        let weather = create_test_weather(0, 10.0); // North wind
        let site = create_test_site();
        
        let analysis = WindDirectionAnalysis::analyze(&weather, &site);
        
        assert_eq!(analysis.wind_direction_deg, 0);
        assert_eq!(analysis.wind_direction_cardinal, "N");
        assert!(matches!(analysis.direction_compatibility, WindDirectionCompatibility::Perfect));
        assert!(analysis.best_launch_direction.is_some());
    }

    #[test]
    fn test_wind_speed_analysis() {
        let weather = create_test_weather(0, 4.0); // 4 m/s = 14.4 km/h (moderate)
        
        let analysis = WindSpeedAnalysis::analyze(&weather);
        
        assert_eq!(analysis.wind_speed_ms, 4.0);
        assert_eq!(analysis.wind_speed_kmh, 14.4);
        assert!(matches!(analysis.speed_category, WindSpeedCategory::Moderate));
        assert!(analysis.pilot_suitability.intermediate);
        assert!(!analysis.pilot_suitability.beginner);
    }

    #[test]
    fn test_flyability_analysis_good_conditions() {
        let weather = create_test_weather(0, 3.0); // Perfect north wind, light speed (10.8 km/h)
        let site = create_test_site();
        
        let analysis = FlyabilityAnalysis::analyze(&weather, &site, 1.0);
        
        assert!(analysis.is_flyable());
        assert!(analysis.flyability_score >= 7.0);
        assert!(analysis.explanation.contains("Good") || analysis.explanation.contains("Excellent"));
    }

    #[test]
    fn test_flyability_analysis_dangerous_conditions() {
        let weather = create_test_weather(0, 15.0); // 15 m/s = 54 km/h (dangerous)
        let site = create_test_site();
        
        let analysis = FlyabilityAnalysis::analyze(&weather, &site, 1.0);
        
        assert!(!analysis.is_flyable());
        assert!(analysis.flyability_score <= 3.0);
    }

    #[test]
    fn test_direction_compatibility_levels() {
        assert!(matches!(
            determine_direction_compatibility(10.0),
            WindDirectionCompatibility::Perfect
        ));
        assert!(matches!(
            determine_direction_compatibility(30.0),
            WindDirectionCompatibility::Favorable
        ));
        assert!(matches!(
            determine_direction_compatibility(70.0),
            WindDirectionCompatibility::Marginal
        ));
        assert!(matches!(
            determine_direction_compatibility(120.0),
            WindDirectionCompatibility::Unfavorable
        ));
        assert!(matches!(
            determine_direction_compatibility(170.0),
            WindDirectionCompatibility::Dangerous
        ));
    }
}