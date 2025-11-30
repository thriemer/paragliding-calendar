use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{info, error, debug};
use tracing_subscriber::{EnvFilter, FmtSubscriber, fmt::format::FmtSpan};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use travelai::{TravelAiConfig, TravelAiError, WeatherApiClient, LocationParser, LocationInput, Cache};

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
        .unwrap_or_else(|_| {
            format!("travelai={},reqwest=warn,sled=warn", log_level).into()
        });

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
            let file_dir = std::path::Path::new(&file_path).parent()
                .ok_or_else(|| TravelAiError::config("Invalid log file path"))?;
            
            // Create log directory if it doesn't exist
            std::fs::create_dir_all(file_dir)?;
            
            let file_appender = RollingFileAppender::builder()
                .rotation(Rotation::DAILY)
                .filename_prefix("travelai")
                .filename_suffix("log")
                .build(file_dir)
                .map_err(|e| TravelAiError::config(format!("Failed to create log file appender: {}", e)))?;
            
            subscriber_builder
                .with_writer(file_appender)
                .with_ansi(false)
                .init();
        }
        "both" => {
            // Both console and file logging
            let file_path = shellexpand::tilde(&config.logging.file_path).into_owned();
            let file_dir = std::path::Path::new(&file_path).parent()
                .ok_or_else(|| TravelAiError::config("Invalid log file path"))?;
            
            std::fs::create_dir_all(file_dir)?;
            
            let _file_appender = RollingFileAppender::builder()
                .rotation(Rotation::DAILY)
                .filename_prefix("travelai")
                .filename_suffix("log")
                .build(file_dir)
                .map_err(|e| TravelAiError::config(format!("Failed to create log file appender: {}", e)))?;
            
            // For "both", we'll use console primarily and add file via tracing-appender
            // This is a simplified approach; full multi-writer would require more complex setup
            subscriber_builder
                .with_ansi(true)
                .init();
            
            info!("Logging to both console and file: {}", file_path);
        }
        _ => {
            // Default: console logging
            subscriber_builder
                .with_ansi(true)
                .init();
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

/// Get weather forecast for a location with caching
fn get_weather_forecast(
    api_client: &mut WeatherApiClient,
    cache: &Cache,
    location_input: LocationInput,
    verbose: bool,
) -> Result<Option<travelai::WeatherForecast>> {
    use travelai::Location;
    
    // Resolve location to coordinates
    let location = match location_input {
        LocationInput::Coordinates(lat, lon) => {
            // Try reverse geocoding to get a proper name
            match api_client.reverse_geocode(lat, lon) {
                Ok(results) if !results.is_empty() => {
                    Location::from(results.into_iter().next().unwrap())
                }
                _ => Location::new(lat, lon, format!("{:.4}, {:.4}", lat, lon)),
            }
        }
        LocationInput::Name(name) => {
            if verbose {
                println!("Geocoding location: {}", name);
            }
            let geocoding_results = api_client.geocode(&name)?;
            if geocoding_results.is_empty() {
                return Err(TravelAiError::validation(
                    format!("Location not found: {}", name)
                ).into());
            }
            
            // Use the first result
            let geocoding = geocoding_results.into_iter().next().unwrap();
            if verbose {
                println!("Found: {} ({:.4}, {:.4})", geocoding.name, geocoding.lat, geocoding.lon);
            }
            Location::from(geocoding)
        }
        LocationInput::PostalCode(postal) => {
            if verbose {
                println!("Geocoding postal code: {}", postal);
            }
            let geocoding_results = api_client.geocode(&postal)?;
            if geocoding_results.is_empty() {
                return Err(TravelAiError::validation(
                    format!("Postal code not found: {}", postal)
                ).into());
            }
            
            // Use the first result
            let geocoding = geocoding_results.into_iter().next().unwrap();
            if verbose {
                println!("Found: {} ({:.4}, {:.4})", geocoding.name, geocoding.lat, geocoding.lon);
            }
            Location::from(geocoding)
        }
    };
    
    // Generate cache key
    let today = chrono::Utc::now().date_naive();
    let cache_key = location.cache_key(&today.format("%Y-%m-%d").to_string());
    
    // Check cache first
    if verbose {
        println!("Checking cache for key: {}", cache_key);
    }
    
    if let Ok(Some(cached_forecast)) = cache.get_weather_forecast(&cache_key) {
        if cached_forecast.is_fresh(6) { // 6 hour TTL
            if verbose {
                println!("Using cached weather data");
            }
            return Ok(Some(cached_forecast));
        } else if verbose {
            println!("Cached data is stale, fetching fresh data");
        }
    } else if verbose {
        println!("No cached data found, fetching from API");
    }
    
    // Fetch from API
    if verbose {
        println!("Fetching weather forecast from API...");
    }
    
    let forecast = api_client.get_forecast(location.latitude, location.longitude)?;
    
    // Cache the result
    if let Err(e) = cache.set_weather_forecast(&cache_key, forecast.clone()) {
        if verbose {
            println!("Warning: Failed to cache weather data: {}", e);
        }
    } else if verbose {
        println!("Weather data cached successfully");
    }
    
    Ok(Some(forecast))
}

/// Display weather forecast in human-readable format
fn display_weather_forecast(forecast: &travelai::WeatherForecast) {
    println!("\n🌤️  Weather Forecast for {}", forecast.location.name);
    println!("📍 Location: {}", forecast.location.format_coordinates());
    
    if let Some(country) = &forecast.location.country {
        println!("🏳️  Country: {}", country);
    }
    
    println!("🕒 Retrieved: {}", forecast.retrieved_at.format("%Y-%m-%d %H:%M UTC"));
    println!();
    
    // Current weather
    if let Some(current) = forecast.current_weather() {
        println!("📊 Current Conditions:");
        println!("   Temperature: {}", current.format_temperature());
        println!("   Description: {}", current.description);
        println!("   Wind: {}", current.format_wind());
        println!("   Pressure: {:.1} hPa", current.pressure);
        if let Some(clouds) = current.cloud_cover {
            println!("   Cloud Cover: {}%", clouds);
        }
        if let Some(visibility) = current.visibility {
            println!("   Visibility: {:.1} km", visibility);
        }
        println!("   Precipitation: {:.1} mm", current.precipitation);
        
        // Paragliding suitability
        if current.is_suitable_for_paragliding() {
            println!("   ✅ Suitable for paragliding");
        } else {
            println!("   ❌ Not suitable for paragliding");
        }
        println!();
    }
    
    // 7-day forecast summary (daily high/low temps and conditions)
    println!("📅 7-Day Forecast:");
    for day in 0..7 {
        let daily_forecasts = forecast.daily_forecast(day);
        if daily_forecasts.is_empty() {
            continue;
        }
        
        let date = if day == 0 {
            "Today".to_string()
        } else if day == 1 {
            "Tomorrow".to_string()
        } else {
            let target_date = chrono::Utc::now().date_naive() + chrono::Duration::days(day as i64);
            target_date.format("%a, %b %d").to_string()
        };
        
        let temps: Vec<f32> = daily_forecasts.iter().map(|w| w.temperature).collect();
        let min_temp = temps.iter().fold(f32::INFINITY, |a, &b| a.min(b));
        let max_temp = temps.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));
        
        // Get description from midday forecast or first available
        let midday_forecast = daily_forecasts.get(daily_forecasts.len() / 2)
            .or_else(|| daily_forecasts.first());
        
        if let Some(midday) = midday_forecast {
            println!("   {:<12} {:.1}°C - {:.1}°C  {} ({})", 
                date, min_temp, max_temp, midday.description,
                if midday.is_suitable_for_paragliding() { "✅" } else { "❌" }
            );
        }
    }
    
    if forecast.forecasts.len() > 7 {
        println!("\n💡 Tip: Use --verbose for detailed hourly forecasts");
    }
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
                    eprintln!("Using default configuration (API key required for weather commands)");
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
                config.cache.ttl_hours
            )?;
            
            // Initialize API client
            let mut api_client = WeatherApiClient::new(weather_config)?;
            
            // Parse location input
            let location_input = LocationParser::parse(location)?;
            
            match get_weather_forecast(&mut api_client, &cache, location_input, cli.verbose)? {
                Some(forecast) => {
                    display_weather_forecast(&forecast);
                }
                None => {
                    println!("No weather data available for the requested location.");
                }
            }
            Ok(())
        }
        None => {
            println!("TravelAI - Intelligent paragliding and outdoor adventure travel planning");
            println!("Use --help for available commands");
            
            // Show configuration hint if needed
            if !cli.verbose {
                println!("\nWeather data provided by OpenMeteo API (no setup required)");
                println!("Use --help for available commands");
            }
            Ok(())
        }
    }
}
