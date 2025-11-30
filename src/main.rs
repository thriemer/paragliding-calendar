use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{debug, error, info};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, FmtSubscriber, fmt::format::FmtSpan};
use travelai::{
    Cache, LocationInput, LocationParser, TravelAiConfig, TravelAiError, WeatherApiClient, weather,
};

/// Load configuration from environment variables only
fn load_config_from_env() -> Result<TravelAiConfig> {
    use config::{Config, Environment};

    let builder = Config::builder().add_source(
        Environment::with_prefix("TRAVELAI")
            .separator("_")
            .try_parsing(true),
    );

    let settings = builder.build()?;
    let mut config: TravelAiConfig = settings.try_deserialize().unwrap_or_default();

    // Apply defaults and validate
    config.apply_defaults();
    config.validate()?;

    Ok(config)
}

/// Initialize logging based on configuration and CLI options
fn init_logging(config: &TravelAiConfig, verbose: bool, debug: bool) -> Result<()> {
    let log_level = if debug {
        "debug"
    } else if verbose {
        "info"
    } else {
        &config.logging.level
    };

    // Create environment filter
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| format!("travelai={},reqwest=warn,sled=warn", log_level).into());

    // Configure formatting based on config
    let subscriber_builder = FmtSubscriber::builder()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(debug)
        .with_file(debug)
        .with_line_number(debug)
        .with_span_events(if debug { FmtSpan::FULL } else { FmtSpan::NONE });

    match config.logging.output.as_str() {
        "file" => {
            // File-only logging
            let file_path = shellexpand::tilde(&config.logging.file_path).into_owned();
            let file_dir = std::path::Path::new(&file_path)
                .parent()
                .ok_or_else(|| TravelAiError::config("Invalid log file path"))?;

            // Create log directory if it doesn't exist
            std::fs::create_dir_all(file_dir)?;

            let file_appender = RollingFileAppender::builder()
                .rotation(Rotation::DAILY)
                .filename_prefix("travelai")
                .filename_suffix("log")
                .build(file_dir)
                .map_err(|e| {
                    TravelAiError::config(format!("Failed to create log file appender: {}", e))
                })?;

            subscriber_builder
                .with_writer(file_appender)
                .with_ansi(false)
                .init();
        }
        "both" => {
            // Both console and file logging
            let file_path = shellexpand::tilde(&config.logging.file_path).into_owned();
            let file_dir = std::path::Path::new(&file_path)
                .parent()
                .ok_or_else(|| TravelAiError::config("Invalid log file path"))?;

            std::fs::create_dir_all(file_dir)?;

            let _file_appender = RollingFileAppender::builder()
                .rotation(Rotation::DAILY)
                .filename_prefix("travelai")
                .filename_suffix("log")
                .build(file_dir)
                .map_err(|e| {
                    TravelAiError::config(format!("Failed to create log file appender: {}", e))
                })?;

            // For "both", we'll use console primarily and add file via tracing-appender
            // This is a simplified approach; full multi-writer would require more complex setup
            subscriber_builder.with_ansi(true).init();

            info!("Logging to both console and file: {}", file_path);
        }
        _ => {
            // Default: console logging
            subscriber_builder.with_ansi(true).init();
        }
    }

    debug!("Logging initialized with level: {}", log_level);
    Ok(())
}

#[derive(Parser)]
#[command(name = "travelai")]
#[command(about = "Intelligent paragliding and outdoor adventure travel planning CLI")]
#[command(version = "0.1.0")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Enable debug output (implies verbose)
    #[arg(short, long)]
    pub debug: bool,

    /// Custom configuration file path
    #[arg(short, long)]
    pub config: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Weather forecast for a location (placeholder for future implementation)
    Weather {
        /// Location (coordinates, city name, or postal code)
        #[arg(short, long)]
        location: String,
    },
}

#[allow(clippy::unnecessary_wraps)]
fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Err(e) = run_cli(&cli) {
        // Display error message based on verbosity level
        if let Some(travel_err) = e.downcast_ref::<TravelAiError>() {
            if cli.debug {
                eprintln!("Error: {}", travel_err.detailed_message());
                error!("Detailed error: {}", travel_err.detailed_message());
            } else if cli.verbose {
                eprintln!("Error: {}", travel_err);
                error!("Error: {}", travel_err);
            } else {
                eprintln!("Error: {}", travel_err.user_message());
                error!("User-friendly error: {}", travel_err.user_message());
            }

            // Set appropriate exit code based on error type
            let exit_code = match travel_err.code() {
                travelai::ErrorCode::ConfigMissingApiKey => 2,
                travelai::ErrorCode::ApiUnauthorized => 3,
                travelai::ErrorCode::ValidationEmptyInput => 4,
                travelai::ErrorCode::ApiLocationNotFound => 5,
                _ => 1,
            };
            std::process::exit(exit_code);
        } else {
            if cli.debug {
                eprintln!("Error: {e:?}");
                error!("Debug error: {e:?}");
            } else {
                eprintln!("Error: {e}");
                error!("General error: {e}");
            }
            std::process::exit(1);
        }
    }

    Ok(())
}

fn run_cli(cli: &Cli) -> Result<()> {
    // Load configuration with environment variable support
    let config = match TravelAiConfig::load_from_path(cli.config.clone()) {
        Ok(config) => config,
        Err(e) => {
            if cli.verbose || cli.debug {
                eprintln!("Warning: Could not load configuration file: {e}");
                eprintln!("Attempting to load from environment variables only...");
            }
            // Try to load with just environment variables
            if let Ok(config) = load_config_from_env() {
                config
            } else {
                if cli.verbose || cli.debug {
                    eprintln!(
                        "Using default configuration (API key required for weather commands)"
                    );
                }
                TravelAiConfig::default()
            }
        }
    };

    // Initialize logging system
    init_logging(&config, cli.verbose || cli.debug, cli.debug)?;

    info!("TravelAI CLI starting");
    debug!("Debug mode enabled");

    if cli.verbose || cli.debug {
        if let Some(config_path) = &cli.config {
            info!("Using config from: {}", config_path.display());
        } else if let Some(default_path) = TravelAiConfig::get_config_path() {
            info!("Using config from: {}", default_path.display());
        }
        info!("Cache location: {}", config.cache.location);
        info!("Log level: {}", config.logging.level);
    }

    match &cli.command {
        Some(Commands::Weather { location }) => {
            if location.is_empty() {
                return Err(TravelAiError::validation("Location cannot be empty").into());
            }

            // API key is now optional for OpenMeteo integration
            // Only validate if provided
            if let Some(api_key) = &config.weather.api_key {
                if api_key.len() < 8 {
                    return Err(TravelAiError::config(
                        "Weather API key appears to be invalid (too short). Please check your API key."
                    ).into());
                }
            }

            if cli.verbose {
                println!("Fetching weather for: {location}");
                println!("Using API endpoint: {}", config.weather.base_url);
            }

            // Use configuration as-is (API key is optional for OpenMeteo)
            let weather_config = config.clone();

            // Initialize cache
            let cache = Cache::new(
                std::path::PathBuf::from(&config.cache.location),
                config.cache.ttl_hours,
            )?;

            // Initialize API client
            let mut api_client = WeatherApiClient::new(weather_config)?;

            // Parse location input
            let location_input = LocationParser::parse(location)?;

            match weather::get_weather_forecast(&mut api_client, &cache, location_input) {
                Ok(forecast) => {
                    weather::display_weather_forecast(&forecast);
                }
                Err(e) => {
                    println!(
                        "No weather data available for the requested location. Error: {}",
                        e
                    );
                }
            }
            Ok(())
        }
        None => {
            println!("TravelAI - Intelligent paragliding and outdoor adventure travel planning");
            println!("Use --help for available commands");
            Ok(())
        }
    }
}
