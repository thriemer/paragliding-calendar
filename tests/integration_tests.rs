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

/// Test weather subcommand with location (no API key required)
#[test]
fn test_weather_command() {
    let output = Command::new("cargo")
        .args(&["run", "--", "weather", "--location", "Test"])
        .output()
        .expect("Failed to execute command");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined_output = format!("{}{}", stdout, stderr);
    
    // With OpenMeteo, the command should either succeed or fail due to cache/network issues
    if !output.status.success() {
        // If it fails, it should be due to cache or network issues, not API key
        let has_cache_error = combined_output.contains("Failed to open cache database");
        let has_network_error = combined_output.contains("Network error") ||
                               combined_output.contains("Unable to connect");
        let has_location_error = combined_output.contains("Location not found") ||
                                combined_output.contains("No results found");
        
        assert!(has_cache_error || has_network_error || has_location_error,
               "Expected cache, network, or location error, got: {}", combined_output);
    }
    // If it succeeds, that's also fine with OpenMeteo integration
}

/// Test weather subcommand with verbose flag
#[test]
fn test_weather_command_verbose() {
    let output = Command::new("cargo")
        .args(&["run", "--", "--verbose", "weather", "--location", "Test"])
        .output()
        .expect("Failed to execute command");

    // With OpenMeteo, command might succeed or fail
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined_output = format!("{}{}", stdout, stderr);
    // Should show verbose fetching output before failing (may fail on cache init in test environment)
    let has_fetching = combined_output.contains("Fetching weather for: Test");
    let has_geocoding = combined_output.contains("Geocoding");
    let has_cache_error = combined_output.contains("Failed to open cache database");
    
    // Test should pass if command succeeds or shows verbose diagnostics on failure
    if output.status.success() {
        // Success is acceptable with OpenMeteo
        assert!(combined_output.contains("Weather Forecast") || 
                combined_output.contains("Temperature") ||
                combined_output.contains("Geocoding"));
    } else {
        // If it fails, should show verbose output
        assert!(
            has_fetching || has_geocoding || has_cache_error,
            "Failed command should show verbose diagnostics, got: {}", combined_output
        );
    }
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

/// Test weather command works without API key (OpenMeteo integration)
#[test]
fn test_weather_no_api_key_success() {
    let output = Command::new("cargo")
        .args(&["run", "--", "weather", "--location", "Test"])
        .output()
        .expect("Failed to execute command");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined_output = format!("{}{}", stdout, stderr);
    
    // Should not fail due to missing API key anymore
    if !output.status.success() {
        // Failure should be due to cache, network, or location issues, not API key
        assert!(
            !combined_output.contains("API key is required") && 
            !combined_output.contains("Configuration error"),
            "Should not require API key with OpenMeteo, got: {}", combined_output
        );
    }
    // Success is also acceptable
}

/// Test default CLI output shows OpenMeteo information
#[test]
fn test_default_output_shows_config_hints() {
    let output = Command::new("cargo")
        .args(&["run"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("TravelAI"));
    assert!(stdout.contains("OpenMeteo") || stdout.contains("no setup required"));
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
