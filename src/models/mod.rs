//! Data models for the TravelAI application
//!
//! This module contains the core domain models organized by concern:
//! - Location: Geographic coordinates and metadata
//! - Weather: Current weather data and measurements  
//! - Forecast: Weather forecast collections and utilities

pub mod forecast;
pub mod location;
pub mod weather;

// Re-export all public types for convenient access
pub use forecast::WeatherForecast;
pub use location::Location;
pub use weather::WeatherData;