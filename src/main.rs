use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use travelai::{TravelAiConfig, TravelAiError};

/// Load configuration from environment variables only
fn load_config_from_env() -> Result<TravelAiConfig> {
    use config::{Config, Environment};
    
    let builder = Config::builder()
        .add_source(
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
        // Display user-friendly error message
        if let Some(travel_err) = e.downcast_ref::<TravelAiError>() {
            eprintln!("Error: {}", travel_err.user_message());
        } else {
            eprintln!("Error: {e}");
        }
        std::process::exit(1);
    }

    Ok(())
}

fn run_cli(cli: &Cli) -> Result<()> {
    // Load configuration with environment variable support
    let config = match TravelAiConfig::load_from_path(cli.config.clone()) {
        Ok(config) => config,
        Err(e) => {
            if cli.verbose {
                eprintln!("Warning: Could not load configuration file: {e}");
                eprintln!("Attempting to load from environment variables only...");
            }
            // Try to load with just environment variables
            if let Ok(config) = load_config_from_env() { 
                config 
            } else {
                if cli.verbose {
                    eprintln!("Using default configuration (API key required for weather commands)");
                }
                TravelAiConfig::default()
            }
        }
    };

    if cli.verbose {
        if let Some(config_path) = &cli.config {
            println!("Using config from: {}", config_path.display());
        } else if let Some(default_path) = TravelAiConfig::get_config_path() {
            println!("Using config from: {}", default_path.display());
        }
        println!("Cache location: {}", config.cache.location);
        println!("Log level: {}", config.logging.level);
    }

    match &cli.command {
        Some(Commands::Weather { location }) => {
            if location.is_empty() {
                return Err(TravelAiError::validation("Location cannot be empty").into());
            }

            // Check for API key from environment if config doesn't have one
            let api_key = if !config.weather.api_key.is_empty() {
                config.weather.api_key.clone()
            } else if let Ok(env_key) = std::env::var("TRAVELAI_WEATHER__API_KEY") {
                env_key
            } else {
                return Err(TravelAiError::config(
                    "Weather API key is required. Please set TRAVELAI_WEATHER__API_KEY environment variable or add to config file."
                ).into());
            };

            if api_key.len() < 8 {
                return Err(TravelAiError::config(
                    "Weather API key appears to be invalid (too short). Please check your API key."
                ).into());
            }

            if cli.verbose {
                println!("Fetching weather for: {location}");
                println!("Using API endpoint: {}", config.weather.base_url);
            }
            println!("Weather command not yet implemented for location: {location}");
            Ok(())
        }
        None => {
            println!("TravelAI - Intelligent paragliding and outdoor adventure travel planning");
            println!("Use --help for available commands");
            
            // Show configuration hint if no config found
            if config.weather.api_key.is_empty() && !cli.verbose {
                println!("\nTo use weather commands, set up configuration:");
                println!("  1. Copy config/default.toml to ~/.config/travelai/config.toml");
                println!("  2. Add your OpenWeatherMap API key");
                println!("  3. Or set TRAVELAI_WEATHER__API_KEY environment variable");
            }
            Ok(())
        }
    }
}
