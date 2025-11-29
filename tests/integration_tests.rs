//! Integration tests for TravelAI CLI

use std::process::Command;

/// Test that the CLI shows help when run without arguments
#[test]
fn test_cli_help_without_args() {
    let output = Command::new("cargo")
        .args(&["run", "--", "--help"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("travelai") || stdout.contains("TravelAI"));
    assert!(stdout.contains("Intelligent paragliding"));
}

/// Test that the CLI shows help with explicit help flag
#[test]
fn test_cli_explicit_help() {
    let output = Command::new("cargo")
        .args(&["run", "--", "help"])
        .output()
        .expect("Failed to execute command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should show help or provide guidance
    assert!(!stdout.is_empty());
}

/// Test weather subcommand with location (requires API key)
#[test]
fn test_weather_command() {
    let output = Command::new("cargo")
        .env("TRAVELAI_WEATHER__API_KEY", "test_api_key_for_integration")
        .args(&["run", "--", "weather", "--location", "Chamonix"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("not yet implemented"));
    assert!(stdout.contains("Chamonix"));
}

/// Test weather subcommand with verbose flag
#[test]
fn test_weather_command_verbose() {
    let output = Command::new("cargo")
        .env("TRAVELAI_WEATHER__API_KEY", "test_api_key_for_integration")
        .args(&["run", "--", "--verbose", "weather", "--location", "Test"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Fetching weather for: Test"));
}

/// Test error handling for empty location
#[test]
fn test_weather_empty_location_error() {
    let output = Command::new("cargo")
        .env("TRAVELAI_WEATHER__API_KEY", "test_api_key_for_integration")
        .args(&["run", "--", "weather", "--location", ""])
        .output()
        .expect("Failed to execute command");

    // Should fail with error
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Invalid input") || stderr.contains("Location cannot be empty"));
}

/// Test configuration error when no API key is provided
#[test]
fn test_weather_no_api_key_error() {
    let output = Command::new("cargo")
        .args(&["run", "--", "weather", "--location", "Chamonix"])
        .output()
        .expect("Failed to execute command");

    // Should fail with configuration error
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("API key is required") || stderr.contains("Configuration error"));
}

/// Test default CLI output shows configuration hints
#[test]
fn test_default_output_shows_config_hints() {
    let output = Command::new("cargo")
        .args(&["run"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("TravelAI"));
    assert!(stdout.contains("To use weather commands, set up configuration"));
    assert!(stdout.contains("OpenWeatherMap API key"));
}

/// Test verbose output shows configuration details
#[test]
fn test_verbose_output_shows_config_details() {
    let output = Command::new("cargo")
        .args(&["run", "--", "--verbose"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Using config from"));
    assert!(stdout.contains("Cache location"));
    assert!(stdout.contains("Log level"));
}

/// Test custom config file option
#[test]
fn test_custom_config_option() {
    let output = Command::new("cargo")
        .args(&["run", "--", "--config", "config/default.toml", "--verbose"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Using config from: config/default.toml"));
}
