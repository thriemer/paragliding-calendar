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

/// Test weather subcommand with location
#[test]
fn test_weather_command() {
    let output = Command::new("cargo")
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
        .args(&["run", "--", "weather", "--location", ""])
        .output()
        .expect("Failed to execute command");

    // Should fail with error
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Invalid input") || stderr.contains("Location cannot be empty"));
}
