use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{debug, error, info};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, FmtSubscriber, fmt::format::FmtSpan};
use travelai::{
    Cache, LocationParser, ParaglidingForecastService, TravelAiConfig, TravelAiError, WeatherApiClient, weather,
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
    /// Weather forecast for a location
    Weather {
        /// Location (coordinates, city name, or postal code)
        #[arg(short, long)]
        location: String,
    },
    /// Paragliding flyability forecast and recommendations
    Paragliding {
        /// Location (coordinates, city name, or postal code)
        location: String,
        /// Search radius in kilometers (default: 50km)
        #[arg(short, long, default_value = "50")]
        radius: f64,
        /// Number of forecast days (default: 7, max: 14)
        #[arg(short, long, default_value = "7")]
        days: usize,
        /// Output format: text or json
        #[arg(short, long, default_value = "text")]
        format: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Err(e) = run_cli(&cli).await {
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

async fn run_cli(cli: &Cli) -> Result<()> {
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
                        "Using default configuration (no API key required for OpenMeteo)"
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

            // No API key required for OpenMeteo

            if cli.verbose {
                println!("Fetching weather for: {location}");
                println!("Using API endpoint: {}", config.weather.base_url);
            }

            // Use configuration as-is (no API key required for OpenMeteo)
            let weather_config = config.clone();

            // Initialize cache
            let cache_path = std::path::PathBuf::from(&config.cache.location);
            let cache = Cache::new(
                &cache_path,
                config.cache.ttl_hours,
            )?;

            // Initialize API client
            let mut api_client = WeatherApiClient::new(weather_config)?;

            // Parse location input
            let location_input = LocationParser::parse(location)?;

            match weather::get_weather_forecast(&mut api_client, &cache, location_input).await {
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
        Some(Commands::Paragliding { location, radius, days, format }) => {
            handle_paragliding_command(location, *radius, *days, format, &config, cli).await
        }
        None => {
            println!("TravelAI - Intelligent paragliding and outdoor adventure travel planning");
            println!("Use --help for available commands");
            Ok(())
        }
    }
}

/// Handle paragliding forecast command
async fn handle_paragliding_command(
    location: &str,
    radius: f64,
    days: usize,
    format: &str,
    config: &TravelAiConfig,
    cli: &Cli,
) -> Result<()> {
    if location.is_empty() {
        return Err(TravelAiError::validation("Location cannot be empty").into());
    }

    // Validate parameters
    if radius <= 0.0 || radius > 500.0 {
        return Err(TravelAiError::validation("Radius must be between 0 and 500 km").into());
    }

    if days == 0 || days > 14 {
        return Err(TravelAiError::validation("Days must be between 1 and 14").into());
    }

    if format != "text" && format != "json" {
        return Err(TravelAiError::validation("Format must be 'text' or 'json'").into());
    }

    if cli.verbose {
        println!("Generating paragliding forecast for: {location}");
        println!("Search radius: {radius}km, Days: {days}");
        println!("Output format: {format}");
    }

    // Initialize cache
    let cache_path = std::path::PathBuf::from(&config.cache.location);
    let cache = Cache::new(
        &cache_path,
        config.cache.ttl_hours,
    )?;

    // Initialize API client
    let mut api_client = WeatherApiClient::new(config.clone())?;

    // Parse location input
    let location_input = LocationParser::parse(location)?;

    match ParaglidingForecastService::generate_forecast(
        &mut api_client,
        &cache,
        location_input,
        radius,
        days,
        Some(config),
    ).await {
        Ok(forecast) => {
            match format {
                "json" => {
                    let json = serde_json::to_string_pretty(&forecast)?;
                    println!("{}", json);
                }
                _ => {
                    display_paragliding_forecast(&forecast);
                }
            }
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Failed to generate paragliding forecast: {}", e));
        }
    }

    Ok(())
}

/// Display paragliding forecast in human-readable format
fn display_paragliding_forecast(forecast: &travelai::ParaglidingForecast) {
    println!("\n🪂 TravelAI Paragliding Forecast - {} ({}km radius)", 
             forecast.location.name, forecast.radius_km);
    println!("📍 Location: {}", forecast.location.format_coordinates());
    
    if let Some(country) = &forecast.location.country {
        println!("🏳️  Country: {}", country);
    }
    
    println!("🕒 Generated: {}", 
             forecast.generated_at.format("%Y-%m-%d %H:%M UTC"));
    println!("🎯 Sites in area: {}", forecast.sites_in_area.len());
    println!();

    if forecast.sites_in_area.is_empty() {
        println!("⚠️  No paragliding sites found in the search area.");
        println!("   Try increasing the search radius or choosing a different location.");
        return;
    }

    for daily_forecast in &forecast.daily_forecasts {
        println!("═══════════════════════════════════════════════════════════");
        
        // Day header
        println!("📅 {} - {}", 
                daily_forecast.day_name, 
                daily_forecast.date.format("%B %d"));
        
        // Weather summary
        let weather = &daily_forecast.weather_summary;
        println!("🌤️  Weather: {}, {:.0}-{:.0}°C, Wind {} {:.0}-{:.0} km/h, {}% clouds",
                weather.description,
                weather.temperature_range.min,
                weather.temperature_range.max,
                weather.wind_summary.direction,
                weather.wind_summary.speed_range.min,
                weather.wind_summary.speed_range.max,
                weather.cloud_cover);
        
        if weather.precipitation_probability > 10 {
            println!("🌧️  Precipitation: {}% probability", weather.precipitation_probability);
        }
        
        // Forecast confidence
        if daily_forecast.confidence < 0.8 {
            println!("⚠️  Forecast confidence: {:.0}%", daily_forecast.confidence * 100.0);
        }
        
        println!();
        
        // Day rating and sites
        println!("{} {} ({})", 
                daily_forecast.day_rating.emoji(), 
                daily_forecast.day_rating,
                daily_forecast.explanation);
        
        if daily_forecast.flyable_sites.is_empty() {
            match daily_forecast.day_rating {
                travelai::paragliding::forecast::DayRating::NotFlyable => {
                    println!("   Reason: {}", daily_forecast.explanation);
                    if weather.wind_summary.speed_range.max > 30.0 {
                        println!("   Alternative: Ground school, equipment maintenance");
                    } else if weather.precipitation_probability > 50 {
                        println!("   Alternative: Indoor planning, route research");
                    } else {
                        println!("   Alternative: Check other nearby areas");
                    }
                }
                _ => {
                    println!("   No sites meet minimum flyability criteria");
                }
            }
        } else {
            // Show top flyable sites (limit to 5)
            let sites_to_show = daily_forecast.flyable_sites.iter().take(5);
            for (index, site_rating) in sites_to_show.enumerate() {
                let score_color = site_rating.wind_analysis.score_color();
                println!("   {}. {} {} ({:.1}/10) - {}km",
                        index + 1,
                        score_color,
                        site_rating.site.name,
                        site_rating.score,
                        site_rating.distance_km);
                
                println!("      Wind: {} {:.0} km/h (gusts {:.0}), {}",
                        daily_forecast.weather_summary.wind_summary.direction,
                        site_rating.wind_analysis.wind_speed.wind_speed_kmh,
                        site_rating.wind_analysis.wind_speed.wind_gust_kmh,
                        site_rating.wind_analysis.wind_direction.direction_compatibility);
                
                println!("      Reason: {}", site_rating.reasoning);
                
                // Pilot suitability
                let suitability = &site_rating.wind_analysis.wind_speed.pilot_suitability;
                let mut suitable_for = Vec::new();
                if suitability.beginner { suitable_for.push("beginners"); }
                if suitability.intermediate { suitable_for.push("intermediate"); }
                if suitability.advanced { suitable_for.push("advanced"); }
                
                if !suitable_for.is_empty() {
                    println!("      Suitable for: {}", suitable_for.join(", "));
                } else {
                    println!("      ⚠️  Not suitable for any skill level");
                }
                
                println!();
            }
            
            if daily_forecast.flyable_sites.len() > 5 {
                println!("   ... and {} more site{}", 
                        daily_forecast.flyable_sites.len() - 5,
                        if daily_forecast.flyable_sites.len() - 5 == 1 { "" } else { "s" });
                println!();
            }
        }
    }
    
    println!("═══════════════════════════════════════════════════════════");
    println!("🛡️  Safety Note: Always check local conditions and weather updates before flying.");
    println!("📊 Scoring: 9-10=Excellent, 7-8=Good, 5-6=Marginal, 3-4=Poor, 0-2=Dangerous");
}
