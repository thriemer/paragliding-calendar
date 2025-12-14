//! Paragliding Flyability Forecast Module
//!
//! This module provides daily flyability recommendations by combining site data,
//! weather forecasts, and wind analysis to generate comprehensive paragliding forecasts.

use crate::models::{Location, WeatherData, WeatherForecast};
use crate::location_resolver::LocationResolver;
use crate::paragliding::site_loader::SiteLoader;
use crate::paragliding::sites::ParaglidingSite;
use crate::paragliding::wind_analysis::{FlyabilityAnalysis, HourlyFlyabilityAnalysis, WindDirectionCompatibility, WindSpeedCategory};
use crate::{Cache, LocationInput, WeatherApiClient};
use crate::config::TravelAiConfig;
use anyhow::Result;
use chrono::{DateTime, NaiveDate, Timelike, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// Daily flyability forecast for a specific location
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyFlyabilityForecast {
    /// Date for this forecast
    pub date: NaiveDate,
    /// Day name (e.g., "Saturday", "Tomorrow")
    pub day_name: String,
    /// Weather summary for the day
    pub weather_summary: DailyWeatherSummary,
    /// Ranked list of flyable sites
    pub flyable_sites: Vec<SiteFlyabilityRating>,
    /// Overall day rating
    pub day_rating: DayRating,
    /// Forecast confidence (0.0-1.0)
    pub confidence: f32,
    /// Human-readable explanation
    pub explanation: String,
}

/// Weather summary for a day
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyWeatherSummary {
    /// Description (e.g., "Sunny", "Partly cloudy")
    pub description: String,
    /// Temperature range in Celsius
    pub temperature_range: TemperatureRange,
    /// Wind summary
    pub wind_summary: WindSummary,
    /// Precipitation probability (0-100%)
    pub precipitation_probability: u8,
    /// Cloud cover percentage (0-100%)
    pub cloud_cover: u8,
}

/// Temperature range for a day
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemperatureRange {
    pub min: f32,
    pub max: f32,
}

/// Wind summary for a day
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindSummary {
    /// Primary direction (e.g., "WSW")
    pub direction: String,
    /// Speed range in km/h
    pub speed_range: SpeedRange,
    /// Dominant direction in degrees
    pub direction_degrees: u16,
}

/// Speed range for wind
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedRange {
    pub min: f32,
    pub max: f32,
}

/// Flyability rating for a specific site
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteFlyabilityRating {
    /// Site information
    pub site: ParaglidingSite,
    /// Flyability score (0-10) - best score for the day
    pub score: f32,
    /// Distance from search center in km
    pub distance_km: f64,
    /// Wind analysis for this site (best hour analysis for backward compatibility)
    pub wind_analysis: FlyabilityAnalysis,
    /// Hourly analysis for the full day
    pub hourly_analysis: HourlyFlyabilityAnalysis,
    /// Site-specific reasoning
    pub reasoning: String,
}

/// Overall rating for a day
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DayRating {
    /// Excellent flying conditions (score >= 8)
    Excellent,
    /// Good flying conditions (score >= 6)
    Good,
    /// Marginal flying conditions (score >= 4)
    Marginal,
    /// Poor flying conditions (score >= 2)
    Poor,
    /// Not flyable (score < 2)
    NotFlyable,
}

/// Multi-day paragliding forecast
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParaglidingForecast {
    /// Search location
    pub location: Location,
    /// Search radius in km
    pub radius_km: f64,
    /// Daily forecasts
    pub daily_forecasts: Vec<DailyFlyabilityForecast>,
    /// When this forecast was generated
    pub generated_at: DateTime<Utc>,
    /// Available sites in the area
    pub sites_in_area: Vec<ParaglidingSite>,
}

/// Paragliding forecast service
pub struct ParaglidingForecastService;

impl ParaglidingForecastService {
    /// Generate multi-day paragliding forecast
    pub async fn generate_forecast(
        api_client: &WeatherApiClient,
        cache: &Cache,
        location_input: LocationInput,
        radius_km: f64,
        days: usize,
        config: Option<&TravelAiConfig>,
    ) -> Result<ParaglidingForecast> {
        info!(
            "Generating {}-day paragliding forecast for radius {}km",
            days, radius_km
        );

        // Resolve location
        let location = LocationResolver::resolve_location(api_client, location_input).await?;
        debug!(
            "Resolved location: {} at ({}, {})",
            location.name, location.latitude, location.longitude
        );

        // Load sites in area
        let sites = SiteLoader::load_sites_in_area(&location, radius_km, config).await?;
        info!("Found {} sites within {}km radius", sites.len(), radius_km);

        if sites.is_empty() {
            warn!("No paragliding sites found in search area");
        }

        // Generate daily forecasts (weather will be fetched per-site)
        let daily_forecasts = Self::generate_daily_forecasts(api_client, cache, &sites, &location, days).await?;

        Ok(ParaglidingForecast {
            location,
            radius_km,
            daily_forecasts,
            generated_at: Utc::now(),
            sites_in_area: sites,
        })
    }


    /// Generate daily forecasts from weather data and sites
    async fn generate_daily_forecasts(
        api_client: &WeatherApiClient,
        cache: &Cache,
        sites: &[ParaglidingSite],
        center_location: &Location,
        days: usize,
    ) -> Result<Vec<DailyFlyabilityForecast>> {
        let mut daily_forecasts = Vec::new();

        for day in 0..days {
            let date = Utc::now().date_naive() + chrono::Duration::days(i64::try_from(day).unwrap_or(0));
            
            let daily_forecast = Self::generate_daily_forecast(
                api_client, 
                cache, 
                date, 
                day, 
                sites, 
                center_location
            ).await?;
            daily_forecasts.push(daily_forecast);
        }

        Ok(daily_forecasts)
    }

    /// Get weather forecast for location using the main weather service
    async fn get_weather_forecast(
        api_client: &WeatherApiClient,
        cache: &Cache,
        location: &Location,
    ) -> Result<WeatherForecast> {
        let location_input = LocationInput::Coordinates(location.latitude, location.longitude);
        let forecast = crate::weather::get_weather_forecast(api_client, cache, location_input).await?;
        Ok(forecast)
    }

    /// Get weather forecast for a specific site
    async fn get_site_weather_forecast(
        api_client: &WeatherApiClient,
        cache: &Cache,
        site: &ParaglidingSite,
    ) -> Result<WeatherForecast> {
        let location_input = LocationInput::Coordinates(
            site.coordinates.latitude, 
            site.coordinates.longitude
        );
        let forecast = crate::weather::get_weather_forecast(api_client, cache, location_input).await?;
        Ok(forecast)
    }

    /// Generate forecast for a single day
    async fn generate_daily_forecast(
        api_client: &WeatherApiClient,
        cache: &Cache,
        date: NaiveDate,
        day_offset: usize,
        sites: &[ParaglidingSite],
        center_location: &Location,
    ) -> Result<DailyFlyabilityForecast> {
        let day_name = Self::format_day_name(day_offset, date);
        
        // Get center location weather for the daily summary
        let center_forecast = Self::get_weather_forecast(api_client, cache, center_location).await?;
        let center_day_weather = center_forecast.daily_forecast(day_offset);
        let weather_summary = Self::create_weather_summary(&center_day_weather);

        // Calculate flyability for each site using site-specific weather
        let mut site_ratings = Vec::new();
        for site in sites {
            match Self::get_site_weather_forecast(api_client, cache, site).await {
                Ok(site_forecast) => {
                    let site_day_weather = site_forecast.daily_forecast(day_offset);
                    if !site_day_weather.is_empty() {
                        // Filter to daylight hours only
                        let daylight_weather = site_forecast.filter_daylight_hours(
                            date,
                            site.coordinates.latitude,
                            site.coordinates.longitude
                        );
                        
                        if !daylight_weather.is_empty() {
                            // Perform hourly analysis for the full daylight period
                            let hourly_analysis = HourlyFlyabilityAnalysis::analyze_hourly(
                                &daylight_weather,
                                site,
                                day_offset
                            );

                            // Only include sites that have flyable conditions (at least 25% favorable hours)
                            if hourly_analysis.is_flyable_day() {
                                // Get best hour analysis for backward compatibility
                                let best_hour = hourly_analysis.hourly_scores
                                    .iter()
                                    .max_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal));
                                
                                if let Some(best_hour_score) = best_hour {
                                    let rating = SiteFlyabilityRating {
                                        site: site.clone(),
                                        score: hourly_analysis.best_flyability_score(),
                                        distance_km: SiteLoader::distance_to_site(center_location, site),
                                        reasoning: Self::generate_hourly_site_reasoning(&hourly_analysis),
                                        wind_analysis: best_hour_score.analysis.clone(),
                                        hourly_analysis,
                                    };
                                    site_ratings.push(rating);
                                }
                            }
                        } else {
                            debug!("No daylight weather data for site {} on day {}", site.name, day_offset);
                        }
                    } else {
                        debug!("No weather data for site {} on day {}", site.name, day_offset);
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch weather for site {}: {}", site.name, e);
                    // Continue with other sites rather than failing the entire forecast
                }
            }
        }

        // Sort sites by flyability score
        site_ratings.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let day_rating = Self::determine_day_rating(&site_ratings);
        let confidence = Self::calculate_confidence(day_offset);
        let explanation = Self::generate_day_explanation(&day_rating, &site_ratings);

        Ok(DailyFlyabilityForecast {
            date,
            day_name,
            weather_summary,
            flyable_sites: site_ratings,
            day_rating,
            confidence,
            explanation,
        })
    }

    /// Format day name (Today, Tomorrow, day of week)
    fn format_day_name(day_offset: usize, date: NaiveDate) -> String {
        match day_offset {
            0 => "Today".to_string(),
            1 => "Tomorrow".to_string(),
            _ => date.format("%A, %B %d").to_string(),
        }
    }

    /// Create weather summary from hourly data
    fn create_weather_summary(day_weather: &[&WeatherData]) -> DailyWeatherSummary {
        if day_weather.is_empty() {
            return DailyWeatherSummary {
                description: "No data".to_string(),
                temperature_range: TemperatureRange { min: 0.0, max: 0.0 },
                wind_summary: WindSummary {
                    direction: "Unknown".to_string(),
                    speed_range: SpeedRange { min: 0.0, max: 0.0 },
                    direction_degrees: 0,
                },
                precipitation_probability: 0,
                cloud_cover: 0,
            };
        }

        let temps: Vec<f32> = day_weather.iter().map(|w| w.temperature).collect();
        let min_temp = temps.iter().fold(f32::INFINITY, |a, &b| a.min(b));
        let max_temp = temps.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));

        let winds: Vec<f32> = day_weather.iter().map(|w| w.wind_speed * 3.6).collect(); // Convert to km/h
        let min_wind = winds.iter().fold(f32::INFINITY, |a, &b| a.min(b));
        let max_wind = winds.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));

        let avg_cloud_cover = day_weather
            .iter()
            .map(|w| f32::from(w.cloud_cover))
            .sum::<f32>()
            / day_weather.len().max(1) as f32;
        let max_precip = day_weather
            .iter()
            .map(|w| w.precipitation)
            .fold(0.0f32, f32::max);

        // Use midday weather for primary description and wind direction
        let midday = day_weather[day_weather.len() / 2];
        let wind_direction =
            crate::models::WeatherData::wind_direction_to_cardinal(midday.wind_direction)
                .to_string();

        DailyWeatherSummary {
            description: midday.description.clone(),
            temperature_range: TemperatureRange {
                min: min_temp,
                max: max_temp,
            },
            wind_summary: WindSummary {
                direction: wind_direction,
                speed_range: SpeedRange {
                    min: min_wind,
                    max: max_wind,
                },
                direction_degrees: midday.wind_direction,
            },
            precipitation_probability: if max_precip > 0.0 {
                (max_precip * 10.0).clamp(0.0, 100.0).round() as u8
            } else {
                0
            },
            cloud_cover: avg_cloud_cover.clamp(0.0, 100.0).round() as u8,
        }
    }

    /// Determine overall day rating from site ratings
    fn determine_day_rating(site_ratings: &[SiteFlyabilityRating]) -> DayRating {
        if site_ratings.is_empty() {
            return DayRating::NotFlyable;
        }

        let best_score = site_ratings.first().map_or(0.0, |s| s.score);
        match best_score {
            s if s >= 8.0 => DayRating::Excellent,
            s if s >= 6.0 => DayRating::Good,
            s if s >= 4.0 => DayRating::Marginal,
            s if s >= 2.0 => DayRating::Poor,
            _ => DayRating::NotFlyable,
        }
    }

    /// Calculate forecast confidence based on time ahead
    fn calculate_confidence(day_offset: usize) -> f32 {
        // Confidence decreases over time
        
        match day_offset {
            0 => 0.95,     // Today - very high confidence
            1 => 0.90,     // Tomorrow - high confidence
            2 => 0.85,     // Day after - good confidence
            3..=4 => 0.75, // 3-4 days - moderate confidence
            5..=7 => 0.65, // 5-7 days - fair confidence
            _ => 0.50,     // Beyond week - low confidence
        }
    }

    /// Generate explanation for the day
    fn generate_day_explanation(
        day_rating: &DayRating,
        site_ratings: &[SiteFlyabilityRating],
    ) -> String {
        if site_ratings.is_empty() {
            return "No flyable sites found for this day".to_string();
        }

        let site_count = site_ratings.len();
        
        // Calculate average percentage of favorable conditions across all sites
        let avg_favorable_pct = if site_ratings.is_empty() {
            0.0
        } else {
            site_ratings.iter()
                .map(|s| s.hourly_analysis.favorable_hours_percentage)
                .sum::<f32>() / site_ratings.len() as f32
        };

        match day_rating {
            DayRating::Excellent => {
                format!(
                    "Excellent ({:.0}% favorable conditions, {} flyable site{})",
                    avg_favorable_pct,
                    site_count,
                    if site_count == 1 { "" } else { "s" }
                )
            }
            DayRating::Good => {
                format!(
                    "Good ({:.0}% favorable conditions, {} flyable site{})",
                    avg_favorable_pct,
                    site_count,
                    if site_count == 1 { "" } else { "s" }
                )
            }
            DayRating::Marginal => {
                format!(
                    "Marginal ({:.0}% favorable conditions, {} flyable site{})",
                    avg_favorable_pct,
                    site_count,
                    if site_count == 1 { "" } else { "s" }
                )
            }
            DayRating::Poor => {
                format!(
                    "Poor ({:.0}% favorable conditions, {} flyable site{})",
                    avg_favorable_pct,
                    site_count,
                    if site_count == 1 { "" } else { "s" }
                )
            }
            DayRating::NotFlyable => "Not suitable for flying".to_string(),
        }
    }

    /// Generate reasoning text for a site
    fn generate_site_reasoning(analysis: &FlyabilityAnalysis) -> String {
        let mut reasons = Vec::new();

        // Wind direction reasoning
        match analysis.wind_direction.direction_compatibility {
            WindDirectionCompatibility::Perfect => {
                reasons.push("perfect wind direction".to_string());
            }
            WindDirectionCompatibility::Favorable => {
                reasons.push("favorable wind direction".to_string());
            }
            WindDirectionCompatibility::Marginal => {
                reasons.push("marginal wind direction".to_string());
            }
            WindDirectionCompatibility::Unfavorable => {
                reasons.push("unfavorable wind direction".to_string());
            }
            WindDirectionCompatibility::Dangerous => {
                reasons.push("dangerous wind direction".to_string());
            }
        }

        // Wind speed reasoning
        match analysis.wind_speed.speed_category {
            WindSpeedCategory::Light => {
                reasons.push("light winds".to_string());
            }
            WindSpeedCategory::Moderate => {
                reasons.push("moderate winds".to_string());
            }
            WindSpeedCategory::Strong => {
                reasons.push("strong winds".to_string());
            }
            WindSpeedCategory::Dangerous => {
                reasons.push("dangerous wind speeds".to_string());
            }
        }

        // Join reasons
        match reasons.len() {
            0 => "No specific reasoning available".to_string(),
            1 => reasons[0].clone(),
            2 => format!("{} and {}", reasons[0], reasons[1]),
            _ => {
                let last = reasons.pop().unwrap();
                format!("{}, and {}", reasons.join(", "), last)
            }
        }
    }

    /// Generate reasoning text for hourly analysis
    fn generate_hourly_site_reasoning(hourly_analysis: &HourlyFlyabilityAnalysis) -> String {
        let favorable_pct = hourly_analysis.favorable_hours_percentage;
        let best_score = hourly_analysis.best_score;
        let total_hours = hourly_analysis.hourly_scores.len();

        if total_hours == 0 {
            return "No daylight hours available".to_string();
        }

        let mut reasoning = Vec::new();

        // Add percentage of favorable conditions
        reasoning.push(format!("{:.0}% favorable conditions", favorable_pct));

        // Add best window information if available
        if let Some((start, end, avg_score)) = &hourly_analysis.best_flying_window {
            reasoning.push(format!(
                "best window: {}:00-{}:00 (score: {:.1})",
                start.hour(),
                end.hour(),
                avg_score
            ));
        }

        // Add overall score information
        if best_score >= 7.0 {
            reasoning.push("excellent peak conditions".to_string());
        } else if best_score >= 5.0 {
            reasoning.push("good peak conditions".to_string());
        } else {
            reasoning.push("marginal peak conditions".to_string());
        }

        reasoning.join(", ")
    }
}

impl std::fmt::Display for DayRating {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DayRating::Excellent => write!(f, "Excellent"),
            DayRating::Good => write!(f, "Good"),
            DayRating::Marginal => write!(f, "Marginal"),
            DayRating::Poor => write!(f, "Poor"),
            DayRating::NotFlyable => write!(f, "Not Flyable"),
        }
    }
}

impl DayRating {
    #[must_use] 
    pub fn emoji(&self) -> &'static str {
        match self {
            DayRating::Excellent => "🟢",
            DayRating::Good => "🟡",
            DayRating::Marginal => "🟠",
            DayRating::Poor => "🔴",
            DayRating::NotFlyable => "⚫",
        }
    }
}
