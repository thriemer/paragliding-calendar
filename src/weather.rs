use crate::Cache;
use crate::Location;
use crate::LocationInput;
use crate::TravelAiError;
use crate::WeatherApiClient;
use crate::WeatherForecast;
use anyhow::Result;
use tracing::debug;

/// Get weather forecast for a location with caching
pub fn get_weather_forecast(
    api_client: &mut WeatherApiClient,
    cache: &Cache,
    location_input: LocationInput,
) -> Result<WeatherForecast> {
    // Resolve location to coordinates
    let location = match location_input {
        LocationInput::Coordinates(lat, lon) => {
            // Try reverse geocoding to get a proper name
            match api_client.reverse_geocode(lat, lon) {
                Ok(results) if !results.is_empty() => {
                    Location::from(results.into_iter().next().unwrap())
                }
                _ => Location::new(lat, lon, format!("{lat:.4}, {lon:.4}")),
            }
        }
        LocationInput::Name(name) => {
            debug!("Geocoding location: {}", name);
            let geocoding_results = api_client.geocode(&name)?;
            if geocoding_results.is_empty() {
                return Err(
                    TravelAiError::validation(format!("Location not found: {name}")).into(),
                );
            }

            // Use the first result
            let geocoding = geocoding_results.into_iter().next().unwrap();
            debug!(
                "Found: {} ({:.4}, {:.4})",
                geocoding.name, geocoding.lat, geocoding.lon
            );
            Location::from(geocoding)
        }
        LocationInput::PostalCode(postal) => {
            debug!("Geocoding postal code: {}", postal);
            let geocoding_results = api_client.geocode(&postal)?;
            if geocoding_results.is_empty() {
                return Err(TravelAiError::validation(format!(
                    "Postal code not found: {postal}"
                ))
                .into());
            }

            // Use the first result
            let geocoding = geocoding_results.into_iter().next().unwrap();
            debug!(
                "Found: {} ({:.4}, {:.4})",
                geocoding.name, geocoding.lat, geocoding.lon
            );
            Location::from(geocoding)
        }
    };

    // Generate cache key
    let today = chrono::Utc::now().date_naive();
    let cache_key = location.cache_key(&today.format("%Y-%m-%d").to_string());

    // Check cache first
    debug!("Checking cache for key: {}", cache_key);

    if let Ok(cached_forecast) = cache.get_weather_forecast(&cache_key) {
        if cached_forecast.is_fresh(6) {
            // 6 hour TTL
            debug!("Using cached weather data");
            return Ok(cached_forecast);
        }
        debug!("Cached data is stale, fetching fresh data");
    } else {
        debug!("No cached data found, fetching from API");
    }

    // Fetch from API
    debug!("Fetching weather forecast from API...");

    let forecast = api_client.get_forecast(location.latitude, location.longitude)?;

    // Cache the result
    if let Err(e) = cache.set_weather_forecast(&cache_key, forecast.clone()) {
        debug!("Warning: Failed to cache weather data: {}", e);
    } else {
        debug!("Weather data cached successfully");
    }

    Ok(forecast)
}

/// Display weather forecast in human-readable format
pub fn display_weather_forecast(forecast: &WeatherForecast) {
    println!("\n🌤️  Weather Forecast for {}", forecast.location.name);
    println!("📍 Location: {}", forecast.location.format_coordinates());

    if let Some(country) = &forecast.location.country {
        println!("🏳️  Country: {country}");
    }

    println!(
        "🕒 Retrieved: {}",
        forecast.retrieved_at.format("%Y-%m-%d %H:%M UTC")
    );
    println!();

    // Current weather
    if let Some(current) = forecast.current_weather() {
        println!("📊 Current Conditions:");
        println!("   Temperature: {}", current.format_temperature());
        println!("   Description: {}", current.description);
        println!("   Wind: {}", current.format_wind());
        println!("   Pressure: {:.1} hPa", current.pressure);
        println!("   Cloud Cover: {}%", current.cloud_cover);
        println!("   Visibility: {:.1} km", current.visibility);
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
            let target_date = chrono::Utc::now().date_naive() + chrono::Duration::days(i64::try_from(day).unwrap_or(0));
            target_date.format("%a, %b %d").to_string()
        };

        let temps: Vec<f32> = daily_forecasts.iter().map(|w| w.temperature).collect();
        let min_temp = temps.iter().fold(f32::INFINITY, |a, &b| a.min(b));
        let max_temp = temps.iter().fold(f32::NEG_INFINITY, |a, &b| a.max(b));

        // Get description from midday forecast or first available
        let midday_forecast = daily_forecasts
            .get(daily_forecasts.len() / 2)
            .or_else(|| daily_forecasts.first());

        if let Some(midday) = midday_forecast {
            println!(
                "   {:<12} {:.1}°C - {:.1}°C  {} ({})",
                date,
                min_temp,
                max_temp,
                midday.description,
                if midday.is_suitable_for_paragliding() {
                    "✅"
                } else {
                    "❌"
                }
            );
        }
    }
}
