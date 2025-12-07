//! Paragliding module
//!
//! This module provides comprehensive paragliding functionality including:
//! - Site data integration from multiple sources (DHV, Paragliding Earth)
//! - Weather analysis for paragliding conditions
//! - Wind analysis and flyability assessment
//! - Flyability forecasting and recommendations
//! - Geographic search and distance calculations

pub mod cache;
pub mod dhv;
pub mod error;
pub mod forecast;
pub mod paragliding_earth;
pub mod sites;
pub mod wind_analysis;

// Re-export commonly used types from submodules
pub use cache::{SearchCacheKey, SiteCache};
pub use dhv::DHVParser;
pub use error::{Result, TravelAIError};
pub use forecast::{DailyFlyabilityForecast, ParaglidingForecast, ParaglidingForecastService};
pub use paragliding_earth::ParaglidingEarthClient;
pub use sites::{
    Coordinates, DataSource, GeographicSearch, LaunchDirection, ParaglidingSite,
    SiteCharacteristics,
};
pub use wind_analysis::{
    FlyabilityAnalysis, WindDirectionAnalysis, WindDirectionCompatibility, WindSpeedAnalysis,
    WindSpeedCategory,
};