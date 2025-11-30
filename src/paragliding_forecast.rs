//! Paragliding Flyability Forecast Module
//!
//! This module provides daily flyability recommendations by combining site data,
//! weather forecasts, and wind analysis to generate comprehensive paragliding forecasts.

use crate::models::{Location, WeatherData, WeatherForecast};
use crate::paragliding::{Coordinates, GeographicSearch, ParaglidingSite};
use crate::wind_analysis::{FlyabilityAnalysis, WindSpeedCategory};
use crate::{Cache, LocationInput, LocationParser, WeatherApiClient};
use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    /// Flyability score (0-10)
    pub score: f32,
    /// Distance from search center in km
    pub distance_km: f64,
    /// Wind analysis for this site
    pub wind_analysis: FlyabilityAnalysis,
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
    pub fn generate_forecast(
        api_client: &mut WeatherApiClient,
        cache: &Cache,
        location_input: LocationInput,
        radius_km: f64,
        days: usize,
    ) -> Result<ParaglidingForecast> {
        info!(
            "Generating {}-day paragliding forecast for radius {}km",
            days, radius_km
        );

        // Resolve location
        let location = Self::resolve_location(api_client, location_input)?;
        debug!(
            "Resolved location: {} at ({}, {})",
            location.name, location.latitude, location.longitude
        );

        // Load sites in area
        let sites = Self::load_sites_in_area(&location, radius_km)?;
        info!("Found {} sites within {}km radius", sites.len(), radius_km);

        if sites.is_empty() {
            warn!("No paragliding sites found in search area");
        }

        // Get weather forecast
        let weather_forecast = Self::get_weather_forecast(api_client, cache, &location)?;
        debug!(
            "Retrieved weather forecast with {} data points",
            weather_forecast.forecasts.len()
        );

        // Generate daily forecasts
        let daily_forecasts = Self::generate_daily_forecasts(&weather_forecast, &sites, days)?;

        Ok(ParaglidingForecast {
            location,
            radius_km,
            daily_forecasts,
            generated_at: Utc::now(),
            sites_in_area: sites,
        })
    }

    /// Resolve location from input
    fn resolve_location(
        api_client: &mut WeatherApiClient,
        location_input: LocationInput,
    ) -> Result<Location> {
        match location_input {
            LocationInput::Coordinates(lat, lon) => {
                // Try reverse geocoding to get a proper name
                match api_client.reverse_geocode(lat, lon) {
                    Ok(results) if !results.is_empty() => {
                        Ok(Location::from(results.into_iter().next().unwrap()))
                    }
                    _ => Ok(Location::new(lat, lon, format!("{:.4}, {:.4}", lat, lon))),
                }
            }
            LocationInput::Name(name) => {
                debug!("Geocoding location: {}", name);
                let geocoding_results = api_client.geocode(&name)?;
                if geocoding_results.is_empty() {
                    return Err(anyhow::anyhow!("Location not found: {}", name));
                }
                let geocoding = geocoding_results.into_iter().next().unwrap();
                Ok(Location::from(geocoding))
            }
            LocationInput::PostalCode(postal) => {
                debug!("Geocoding postal code: {}", postal);
                let geocoding_results = api_client.geocode(&postal)?;
                if geocoding_results.is_empty() {
                    return Err(anyhow::anyhow!("Postal code not found: {}", postal));
                }
                let geocoding = geocoding_results.into_iter().next().unwrap();
                Ok(Location::from(geocoding))
            }
        }
    }

    /// Load paragliding sites within radius of location
    fn load_sites_in_area(location: &Location, radius_km: f64) -> Result<Vec<ParaglidingSite>> {
        // For now, use the DHV XML file that should be present
        let dhv_file_path = "dhvgelaende_dhvxml_de.xml";

        let sites = if std::path::Path::new(dhv_file_path).exists() {
            crate::paragliding::DHVParser::load_sites(dhv_file_path)?
        } else {
            warn!(
                "DHV XML file not found at {}, using empty site list",
                dhv_file_path
            );
            Vec::new()
        };

        let search_center = Coordinates {
            latitude: location.latitude,
            longitude: location.longitude,
        };

        let nearby_sites = GeographicSearch::sites_within_radius(&sites, &search_center, radius_km);
        Ok(nearby_sites.into_iter().cloned().collect())
    }

    /// Get weather forecast for location
    fn get_weather_forecast(
        api_client: &mut WeatherApiClient,
        cache: &Cache,
        location: &Location,
    ) -> Result<WeatherForecast> {
        let location_input = LocationInput::Coordinates(location.latitude, location.longitude);
        let forecast = crate::weather::get_weather_forecast(api_client, cache, location_input)?;
        Ok(forecast)
    }

    /// Generate daily forecasts from weather data and sites
    fn generate_daily_forecasts(
        weather_forecast: &WeatherForecast,
        sites: &[ParaglidingSite],
        days: usize,
    ) -> Result<Vec<DailyFlyabilityForecast>> {
        let mut daily_forecasts = Vec::new();

        for day in 0..days {
            let day_weather = weather_forecast.daily_forecast(day);
            if day_weather.is_empty() {
                debug!(
                    "No weather data for day {}, stopping forecast generation",
                    day
                );
                break;
            }

            let date = if !weather_forecast.forecasts.is_empty() {
                weather_forecast.forecasts[0].timestamp.date_naive()
                    + chrono::Duration::days(day as i64)
            } else {
                Utc::now().date_naive() + chrono::Duration::days(day as i64)
            };

            let daily_forecast = Self::generate_daily_forecast(date, day, &day_weather, sites)?;
            daily_forecasts.push(daily_forecast);
        }

        Ok(daily_forecasts)
    }

    /// Generate forecast for a single day
    fn generate_daily_forecast(
        date: NaiveDate,
        day_offset: usize,
        day_weather: &[&WeatherData],
        sites: &[ParaglidingSite],
    ) -> Result<DailyFlyabilityForecast> {
        let day_name = Self::format_day_name(day_offset, date);
        let weather_summary = Self::create_weather_summary(day_weather);

        // Calculate flyability for each site
        let mut site_ratings = Vec::new();
        for site in sites {
            // Use midday weather for site analysis
            if let Some(midday_weather) = day_weather.get(day_weather.len() / 2) {
                let hours_ahead = day_offset as f32 * 24.0 + 12.0; // Midday of the day
                let analysis = FlyabilityAnalysis::analyze(midday_weather, site, hours_ahead);

                // Only include sites with reasonable flyability scores
                if analysis.flyability_score >= 2.0 {
                    let rating = SiteFlyabilityRating {
                        site: site.clone(),
                        score: analysis.flyability_score,
                        distance_km: 0.0, // TODO: Calculate actual distance
                        reasoning: Self::generate_site_reasoning(&analysis),
                        wind_analysis: analysis,
                    };
                    site_ratings.push(rating);
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
            .map(|w| w.cloud_cover as f32)
            .sum::<f32>()
            / day_weather.len() as f32;
        let max_precip = day_weather
            .iter()
            .map(|w| w.precipitation)
            .fold(0.0f32, |a, b| a.max(b));

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
                ((max_precip * 10.0).min(100.0)) as u8
            } else {
                0
            },
            cloud_cover: avg_cloud_cover as u8,
        }
    }

    /// Determine overall day rating from site ratings
    fn determine_day_rating(site_ratings: &[SiteFlyabilityRating]) -> DayRating {
        if site_ratings.is_empty() {
            return DayRating::NotFlyable;
        }

        let best_score = site_ratings.first().map(|s| s.score).unwrap_or(0.0);
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
        let base_confidence = match day_offset {
            0 => 0.95,     // Today - very high confidence
            1 => 0.90,     // Tomorrow - high confidence
            2 => 0.85,     // Day after - good confidence
            3..=4 => 0.75, // 3-4 days - moderate confidence
            5..=7 => 0.65, // 5-7 days - fair confidence
            _ => 0.50,     // Beyond week - low confidence
        };
        base_confidence
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
        let best_score = site_ratings.first().map(|s| s.score).unwrap_or(0.0);

        match day_rating {
            DayRating::Excellent => {
                format!(
                    "Excellent flying conditions with {} flyable site{} (best score: {:.1}/10)",
                    site_count,
                    if site_count == 1 { "" } else { "s" },
                    best_score
                )
            }
            DayRating::Good => {
                format!(
                    "Good flying conditions with {} suitable site{} (best score: {:.1}/10)",
                    site_count,
                    if site_count == 1 { "" } else { "s" },
                    best_score
                )
            }
            DayRating::Marginal => {
                format!(
                    "Marginal conditions - {} site{} flyable with caution (best score: {:.1}/10)",
                    site_count,
                    if site_count == 1 { " is" } else { "s are" },
                    best_score
                )
            }
            DayRating::Poor => {
                format!(
                    "Poor conditions - {} site{} potentially flyable (best score: {:.1}/10)",
                    site_count,
                    if site_count == 1 { " is" } else { "s are" },
                    best_score
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
            crate::wind_analysis::WindDirectionCompatibility::Perfect => {
                reasons.push("perfect wind direction".to_string());
            }
            crate::wind_analysis::WindDirectionCompatibility::Favorable => {
                reasons.push("favorable wind direction".to_string());
            }
            crate::wind_analysis::WindDirectionCompatibility::Marginal => {
                reasons.push("marginal wind direction".to_string());
            }
            crate::wind_analysis::WindDirectionCompatibility::Unfavorable => {
                reasons.push("unfavorable wind direction".to_string());
            }
            crate::wind_analysis::WindDirectionCompatibility::Dangerous => {
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
