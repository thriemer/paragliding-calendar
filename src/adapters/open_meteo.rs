use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use tracing::instrument;

use crate::{
    adapters::cache::PersistentCache,
    domain::{
        location::Location,
        ports::{GeoProvider, WeatherProvider},
        weather::{WeatherForecast, WeatherModel},
    },
};

pub struct OpenMeteoClient {
    cache: Arc<PersistentCache>,
}

impl OpenMeteoClient {
    pub fn new(cache: Arc<PersistentCache>) -> Self {
        Self { cache }
    }
}

#[async_trait]
impl WeatherProvider for OpenMeteoClient {
    #[instrument(skip_all, fields(lat = %source.latitude, lon = %source.longitude))]
    async fn get_forecast(
        &self,
        source: Location,
        model: Option<String>,
    ) -> Result<WeatherForecast> {
        let model_suffix = model
            .as_deref()
            .map(|m| format!("_{}", m))
            .unwrap_or_default();
        // v2: WeatherData layout changed (Option fields); postcard can't decode v1 entries.
        let key = format!("weather_v2_for_{}{}", source.to_key(), model_suffix);

        if let Some(cached) = self.cache.get::<WeatherForecast>(&key).await? {
            return Ok(cached);
        }

        let forecast = get_forecast_raw(source.clone(), model.as_deref()).await?;
        self.cache
            .put(&key, forecast.clone(), Duration::from_hours(6u64))
            .await?;
        tracing::debug!(location = %source.to_key(), "Weather fetch successful");
        Ok(forecast)
    }

    fn available_models(&self) -> Vec<WeatherModel> {
        vec![
            WeatherModel {
                id: "ecmwf_ifs04".to_string(),
                name: "ECMWF IFS ( deterministic)".to_string(),
            },
            WeatherModel {
                id: "icon".to_string(),
                name: "ICON (DWD)".to_string(),
            },
            WeatherModel {
                id: "icon_eu".to_string(),
                name: "ICON EU (DWD)".to_string(),
            },
            WeatherModel {
                id: "gfs".to_string(),
                name: "GFS (NOAA)".to_string(),
            },
            WeatherModel {
                id: "gem".to_string(),
                name: "GEM (CMC)".to_string(),
            },
            WeatherModel {
                id: "arpege_world".to_string(),
                name: "ARPEGE World (Météo-France)".to_string(),
            },
        ]
    }
}

#[async_trait]
impl GeoProvider for OpenMeteoClient {
    #[instrument(skip(self), fields(location_name = %location_name))]
    async fn geocode(&self, location_name: &str) -> Result<Vec<Location>> {
        geocode_raw(location_name).await
    }

    #[instrument(skip(self))]
    async fn fetch_elevation(&self, latitude: f64, longitude: f64) -> Result<f64> {
        let rounded_lat = (latitude * 1000.0).round() / 1000.0;
        let rounded_lon = (longitude * 1000.0).round() / 1000.0;
        let cache_key = format!("elevation_{}_{}", rounded_lat, rounded_lon);

        if let Some(cached) = self.cache.get::<f64>(&cache_key).await? {
            return Ok(cached);
        }

        let url = format!(
            "https://api.open-meteo.com/v1/elevation?latitude={}&longitude={}",
            latitude, longitude
        );

        let response = reqwest::get(&url).await?;
        let data: serde_json::Value = response.json().await?;

        let elevation = data["elevation"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_f64())
            .ok_or(anyhow!("No elevation provided in response"))?;

        let _ = self
            .cache
            .put(
                &cache_key,
                elevation,
                std::time::Duration::from_secs(365 * 24 * 60 * 60),
            )
            .await;

        Ok(elevation)
    }
}

async fn get_forecast_raw(location: Location, model: Option<&str>) -> Result<WeatherForecast> {
    let mut url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}&hourly=temperature_2m,windspeed_10m,winddirection_10m,windgusts_10m,precipitation,cloudcover,surface_pressure,visibility,weathercode&timezone=UTC&forecast_days=7&wind_speed_unit=ms",
        location.latitude, location.longitude
    );

    if let Some(model) = model {
        url.push_str(&format!("&models={}", model));
    }

    let response = reqwest::get(url).await?;

    let forecast_response: openmeteo::ForecastResponse = response
        .json()
        .await
        .with_context(|| "Failed to parse OpenMeteo forecast response")?;

    let forecast = WeatherForecast::from_openmeteo(&forecast_response, location);
    Ok(forecast)
}

async fn geocode_raw(location_name: &str) -> Result<Vec<Location>> {
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=5&language=en&format=json",
        urlencoding::encode(location_name)
    );

    let response = reqwest::get(url).await?;

    let openmeteo_response: openmeteo::GeocodingResponse = response
        .json()
        .await
        .with_context(|| "Failed to parse OpenMeteo geocoding response")?;

    let geocoding_results: Vec<Location> = openmeteo_response
        .results
        .unwrap_or_default()
        .into_iter()
        .map(|geocoding_result| geocoding_result.into())
        .collect();

    tracing::debug!(
        count = geocoding_results.len(),
        query = %location_name,
        "Geocoding results returned"
    );
    Ok(geocoding_results)
}

mod openmeteo {
    
    use serde::Deserialize;

    use super::{Location, WeatherForecast};
    use crate::domain::weather::WeatherData;

    #[derive(Debug, Deserialize)]
    pub struct ForecastResponse {
        pub hourly: Option<HourlyData>,
    }

    /// Every value array carries per-element nulls — Open-Meteo returns `null` for hours a
    /// model doesn't cover.
    #[derive(Debug, Deserialize)]
    pub struct HourlyData {
        pub time: Vec<String>,
        #[serde(rename = "temperature_2m")]
        pub temperature: Option<Vec<Option<f32>>>,
        #[serde(rename = "windspeed_10m")]
        pub wind_speed: Option<Vec<Option<f32>>>,
        #[serde(rename = "winddirection_10m")]
        pub wind_direction: Option<Vec<Option<u16>>>,
        #[serde(rename = "windgusts_10m")]
        pub wind_gusts: Option<Vec<Option<f32>>>,
        pub precipitation: Option<Vec<Option<f32>>>,
        #[serde(rename = "cloudcover")]
        pub cloud_cover: Option<Vec<Option<u8>>>,
        #[serde(rename = "surface_pressure")]
        pub pressure: Option<Vec<Option<f32>>>,
        pub visibility: Option<Vec<Option<f32>>>,
        #[serde(rename = "weathercode")]
        pub weather_code: Option<Vec<Option<u8>>>,
    }

    fn at<T: Copy>(field: &Option<Vec<Option<T>>>, i: usize) -> Option<T> {
        field.as_ref()?.get(i).copied().flatten()
    }

    #[derive(Debug, Deserialize)]
    pub struct GeocodingResponse {
        pub results: Option<Vec<GeocodingResult>>,
    }

    #[derive(Debug, Deserialize)]
    pub struct GeocodingResult {
        pub name: String,
        pub latitude: f64,
        pub longitude: f64,
        pub country: Option<String>,
    }

    impl From<GeocodingResult> for Location {
        fn from(value: GeocodingResult) -> Location {
            Location {
                latitude: value.latitude,
                longitude: value.longitude,
                name: value.name,
                country: value.country.unwrap_or("Unknown".into()),
            }
        }
    }

    #[must_use]
    pub fn weather_code_to_description(code: u8) -> &'static str {
        match code {
            0 => "Clear sky",
            1 => "Mainly clear",
            2 => "Partly cloudy",
            3 => "Overcast",
            45 => "Fog",
            48 => "Depositing rime fog",
            51 => "Light drizzle",
            53 => "Moderate drizzle",
            55 => "Dense drizzle",
            56 => "Light freezing drizzle",
            57 => "Dense freezing drizzle",
            61 => "Slight rain",
            63 => "Moderate rain",
            65 => "Heavy rain",
            66 => "Light freezing rain",
            67 => "Heavy freezing rain",
            71 => "Slight snow fall",
            73 => "Moderate snow fall",
            75 => "Heavy snow fall",
            77 => "Snow grains",
            80 => "Slight rain showers",
            81 => "Moderate rain showers",
            82 => "Violent rain showers",
            85 => "Slight snow showers",
            86 => "Heavy snow showers",
            95 => "Thunderstorm",
            96 => "Thunderstorm with slight hail",
            99 => "Thunderstorm with heavy hail",
            _ => "Unknown",
        }
    }

    impl WeatherForecast {
        /// Hours with a missing scoring-relevant value are dropped rather than filled with
        /// sentinels — a fabricated wind speed scores as flyable weather.
        #[must_use]
        pub fn from_openmeteo(response: &ForecastResponse, location: Location) -> Self {
            let mut forecasts = Vec::new();
            let mut skipped = 0usize;

            if let Some(hourly) = &response.hourly {
                for i in 0..hourly.time.len() {
                    let complete = (|| {
                        let timestamp = chrono::NaiveDateTime::parse_from_str(
                            &hourly.time[i],
                            "%Y-%m-%dT%H:%M",
                        )
                        .ok()?
                        .and_utc();
                        let weather_code = at(&hourly.weather_code, i).unwrap_or(0);
                        Some(WeatherData {
                            timestamp,
                            temperature: at(&hourly.temperature, i)?,
                            wind_speed_ms: at(&hourly.wind_speed, i)?,
                            wind_direction: at(&hourly.wind_direction, i)?,
                            wind_gust_ms: at(&hourly.wind_gusts, i)?,
                            precipitation: at(&hourly.precipitation, i)?,
                            cloud_cover: at(&hourly.cloud_cover, i)?,
                            pressure: at(&hourly.pressure, i)?,
                            visibility: at(&hourly.visibility, i),
                            description: weather_code_to_description(weather_code).to_string(),
                        })
                    })();
                    match complete {
                        Some(data) => forecasts.push(data),
                        None => skipped += 1,
                    }
                }
            }
            if skipped > 0 {
                tracing::debug!(skipped, kept = forecasts.len(), "dropped incomplete forecast hours");
            }

            Self {
                location,
                forecast: forecasts,
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn incomplete_hours_are_dropped_not_fabricated() {
            // Hour 0 complete, hour 1 has a null wind speed, hour 2 unparseable timestamp.
            let body = r#"{
                "hourly": {
                    "time": ["2026-07-02T10:00", "2026-07-02T11:00", "not-a-time"],
                    "temperature_2m": [20.0, 21.0, 22.0],
                    "windspeed_10m": [3.0, null, 3.5],
                    "winddirection_10m": [180, 190, 200],
                    "windgusts_10m": [4.0, 4.5, 5.0],
                    "precipitation": [0.0, 0.0, 0.0],
                    "cloudcover": [25, 30, 35],
                    "surface_pressure": [1013.0, 1012.0, 1011.0],
                    "visibility": [null, 10000.0, 10000.0],
                    "weathercode": [1, 1, 1]
                }
            }"#;
            let response: ForecastResponse = serde_json::from_str(body).unwrap();
            let loc = Location::new(50.7, 13.0, "Test".into(), "DE".into());
            let forecast = WeatherForecast::from_openmeteo(&response, loc);

            assert_eq!(forecast.forecast.len(), 1, "only the complete hour survives");
            let hour = &forecast.forecast[0];
            assert_eq!(hour.wind_speed_ms, 3.0);
            assert_eq!(hour.visibility, None, "missing visibility is None, not a sentinel");
        }
    }
}
