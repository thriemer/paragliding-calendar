//! Application layer for TravelAI
//!
//! This module contains application services that coordinate domain logic
//! and infrastructure concerns to fulfill user stories.

pub mod paragliding_calendar_service;

pub use paragliding_calendar_service::{CalendarConfig, ParaglidingCalendarService};
