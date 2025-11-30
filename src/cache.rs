//! Cache layer for storing weather data locally
//!
//! This module provides a caching layer using Sled embedded database
//! to store weather forecasts with TTL support.

use crate::models::WeatherForecast;
use crate::{ErrorCode, TravelAiError};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sled::Db;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::{debug, error, info, instrument, warn};

/// Cache metadata for stored entries
#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry<T> {
    /// The cached data
    data: T,
    /// When this entry was stored
    stored_at: DateTime<Utc>,
    /// TTL in hours
    ttl_hours: u32,
}

impl<T> CacheEntry<T> {
    /// Create a new cache entry
    fn new(data: T, ttl_hours: u32) -> Self {
        Self {
            data,
            stored_at: Utc::now(),
            ttl_hours,
        }
    }

    /// Check if this cache entry is still valid
    fn is_valid(&self) -> bool {
        let age = Utc::now() - self.stored_at;
        age.num_hours() < i64::from(self.ttl_hours)
    }

    /// Get the data if the entry is still valid
    fn get_if_valid(self) -> Result<T> {
        if self.is_valid() {
            Ok(self.data)
        } else {
            Err(TravelAiError::Cache {
                message: "Entry reached TTL".into(),
                code: ErrorCode::CacheReadFailed,
                context: HashMap::default(),
            })?
        }
    }
}

/// Cache layer for weather data
pub struct Cache {
    /// Sled database instance
    db: Db,
    /// Default TTL in hours
    default_ttl_hours: u32,
}

impl Cache {
    /// Create a new cache instance
    #[instrument(fields(cache_dir = %cache_dir.display(), default_ttl_hours))]
    pub fn new(cache_dir: &Path, default_ttl_hours: u32) -> Result<Self> {
        info!(
            "Initializing cache at {} with {}h TTL",
            cache_dir.display(),
            default_ttl_hours
        );
        let start_time = Instant::now();

        // Ensure cache directory exists
        debug!("Ensuring cache directory exists: {}", cache_dir.display());
        std::fs::create_dir_all(cache_dir).with_context(|| {
            format!("Failed to create cache directory: {}", cache_dir.display())
        })?;

        // Open Sled database
        debug!("Opening Sled database at: {}", cache_dir.display());
        let db = sled::open(cache_dir)
            .with_context(|| format!("Failed to open cache database at: {}", cache_dir.display()))
            .map_err(|e| {
                error!("Cache database initialization failed: {}", e);
                TravelAiError::cache_with_context(
                    format!("Failed to open cache database at: {}", cache_dir.display()),
                    ErrorCode::CacheInitFailed,
                    HashMap::from([("path".to_string(), cache_dir.display().to_string())]),
                )
            })?;

        let duration = start_time.elapsed();
        info!(
            "Cache initialized successfully in {:.3}s",
            duration.as_secs_f64()
        );

        Ok(Self {
            db,
            default_ttl_hours,
        })
    }

    /// Create cache with default location and TTL
    pub fn with_defaults() -> Result<Self> {
        let cache_dir = Self::default_cache_dir()?;
        Self::new(&cache_dir, 6) // Default 6-hour TTL
    }

    /// Get the default cache directory
    pub fn default_cache_dir() -> Result<PathBuf> {
        dirs::cache_dir()
            .map(|dir| dir.join("travelai"))
            .ok_or_else(|| TravelAiError::cache("Unable to determine cache directory").into())
    }

    /// Get a value from the cache
    pub fn get<T>(&self, key: &str) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        if let Some(data) = self
            .db
            .get(key)
            .with_context(|| format!("Failed to read from cache key: {key}"))?
        {
            let entry: CacheEntry<T> = serde_json::from_slice(&data)
                .with_context(|| format!("Failed to deserialize cache entry for key: {key}"))?;

            entry.get_if_valid()
        } else {
            Err(TravelAiError::Cache {
                message: "Cache Entry not found".into(),
                code: ErrorCode::CacheReadFailed,
                context: HashMap::default(),
            })?
        }
    }

    /// Store a value in the cache
    pub fn set<T>(&self, key: &str, value: T) -> Result<()>
    where
        T: Serialize,
    {
        self.set_with_ttl(key, value, self.default_ttl_hours)
    }

    /// Store a value in the cache with custom TTL
    pub fn set_with_ttl<T>(&self, key: &str, value: T, ttl_hours: u32) -> Result<()>
    where
        T: Serialize,
    {
        let entry = CacheEntry::new(value, ttl_hours);
        let serialized = serde_json::to_vec(&entry)
            .with_context(|| format!("Failed to serialize cache entry for key: {key}"))?;

        self.db
            .insert(key, serialized)
            .with_context(|| format!("Failed to write to cache key: {key}"))?;

        self.db
            .flush()
            .with_context(|| "Failed to flush cache to disk")?;

        Ok(())
    }

    /// Remove a value from the cache
    pub fn remove(&self, key: &str) -> Result<bool> {
        let removed = self
            .db
            .remove(key)
            .with_context(|| format!("Failed to remove cache key: {key}"))?
            .is_some();

        if removed {
            self.db
                .flush()
                .with_context(|| "Failed to flush cache to disk")?;
        }

        Ok(removed)
    }

    /// Check if a key exists in the cache and is valid
    #[must_use] 
    pub fn contains(&self, key: &str) -> bool {
        self.get::<serde_json::Value>(key).is_ok()
    }

    /// Get all keys in the cache
    pub fn keys(&self) -> Result<Vec<String>> {
        let keys: Result<Vec<String>> = self
            .db
            .iter()
            .map(|item| {
                item.with_context(|| "Failed to iterate cache keys")
                    .map(|(key, _)| String::from_utf8_lossy(&key).into_owned())
            })
            .collect();

        keys
    }

    /// Clear expired entries from the cache
    pub fn cleanup_expired(&self) -> Result<usize> {
        let mut removed_count = 0;

        // Collect keys to remove (can't modify while iterating)
        let mut keys_to_remove = Vec::new();

        for item in self.db.iter() {
            let (key, value) = item.with_context(|| "Failed to iterate cache during cleanup")?;

            // Try to deserialize as cache entry to check TTL
            if let Ok(entry) = serde_json::from_slice::<CacheEntry<serde_json::Value>>(&value) {
                if !entry.is_valid() {
                    keys_to_remove.push(key.to_vec());
                }
            } else {
                // If we can't deserialize, it's probably corrupted - remove it
                keys_to_remove.push(key.to_vec());
            }
        }

        // Remove expired entries
        for key in keys_to_remove {
            self.db.remove(&key).with_context(|| {
                format!(
                    "Failed to remove expired key: {}",
                    String::from_utf8_lossy(&key)
                )
            })?;
            removed_count += 1;
        }

        if removed_count > 0 {
            self.db
                .flush()
                .with_context(|| "Failed to flush cache after cleanup")?;
        }

        Ok(removed_count)
    }

    /// Get cache statistics
    pub fn stats(&self) -> Result<CacheStats> {
        let total_entries = self.db.len();
        let size_on_disk = self
            .db
            .size_on_disk()
            .with_context(|| "Failed to get cache size")?;

        // Count valid entries by iterating
        let mut valid_entries = 0;
        let mut expired_entries = 0;

        for item in self.db.iter() {
            let (_, value) = item.with_context(|| "Failed to iterate cache for stats")?;

            if let Ok(entry) = serde_json::from_slice::<CacheEntry<serde_json::Value>>(&value) {
                if entry.is_valid() {
                    valid_entries += 1;
                } else {
                    expired_entries += 1;
                }
            }
        }

        Ok(CacheStats {
            total_entries,
            valid_entries,
            expired_entries,
            size_bytes: size_on_disk,
        })
    }

    /// Clear all entries from the cache
    pub fn clear(&self) -> Result<()> {
        self.db.clear().with_context(|| "Failed to clear cache")?;

        self.db
            .flush()
            .with_context(|| "Failed to flush cache after clear")?;

        Ok(())
    }

    /// Get weather forecast from cache
    pub fn get_weather_forecast(&self, key: &str) -> Result<WeatherForecast> {
        self.get(key)
    }

    /// Store weather forecast in cache
    pub fn set_weather_forecast(&self, key: &str, forecast: WeatherForecast) -> Result<()> {
        self.set(key, forecast)
    }

    /// Store weather forecast with custom TTL
    pub fn set_weather_forecast_with_ttl(
        &self,
        key: &str,
        forecast: WeatherForecast,
        ttl_hours: u32,
    ) -> Result<()> {
        self.set_with_ttl(key, forecast, ttl_hours)
    }
}

/// Cache statistics
#[derive(Debug)]
pub struct CacheStats {
    /// Total number of entries in cache
    pub total_entries: usize,
    /// Number of valid (non-expired) entries
    pub valid_entries: usize,
    /// Number of expired entries
    pub expired_entries: usize,
    /// Total size in bytes
    pub size_bytes: u64,
}

impl CacheStats {
    /// Format size in human-readable format
    #[must_use] 
    pub fn format_size(&self) -> String {
        // Use integer arithmetic to avoid precision loss until final formatting
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;
        
        if self.size_bytes < KB {
            format!("{} B", self.size_bytes)
        } else if self.size_bytes < MB {
            format!("{:.1} KB", self.size_bytes as f64 / KB as f64)
        } else if self.size_bytes < GB {
            format!("{:.1} MB", self.size_bytes as f64 / MB as f64)
        } else {
            format!("{:.1} GB", self.size_bytes as f64 / GB as f64)
        }
    }

    /// Calculate hit rate percentage
    #[must_use] 
    pub fn hit_rate(&self) -> f64 {
        if self.total_entries == 0 {
            0.0
        } else {
            (100.0 * self.valid_entries as f64) / self.total_entries as f64
        }
    }
}

// Helper function for generating cache keys
impl Cache {
    /// Generate a standardized cache key for weather data
    #[must_use] 
    pub fn weather_cache_key(lat: f64, lon: f64, date: &str) -> String {
        format!("weather:{lat:.2}:{lon:.2}:{date}")
    }

    /// Generate a daily cache key for weather data (rounded to day)
    #[must_use] 
    pub fn daily_weather_key(lat: f64, lon: f64, date: &chrono::NaiveDate) -> String {
        Self::weather_cache_key(lat, lon, &date.format("%Y-%m-%d").to_string())
    }

    /// Generate a current weather cache key
    #[must_use] 
    pub fn current_weather_key(lat: f64, lon: f64) -> String {
        let today = Utc::now().date_naive();
        Self::daily_weather_key(lat, lon, &today)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Location, WeatherData, WeatherForecast};
    use tempfile::TempDir;

    fn create_test_cache() -> (Cache, TempDir) {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let cache_path = temp_dir.path().join("test_cache");
        let cache = Cache::new(&cache_path, 1).expect("Failed to create test cache");
        (cache, temp_dir)
    }

    fn create_test_forecast() -> WeatherForecast {
        let location = Location::new(46.8182, 8.2275, "Interlaken".to_string());
        let weather = WeatherData {
            timestamp: Utc::now(),
            temperature: 15.0,
            wind_speed: 5.0,
            wind_direction: 180,
            wind_gust: 14.4,
            precipitation: 0.0,
            cloud_cover: 20,
            pressure: 1013.0,
            visibility: 10.0,
            description: "Clear sky".to_string(),
            icon: Some("01d".to_string()),
        };

        WeatherForecast::new(location, vec![weather])
    }

    #[test]
    fn test_cache_basic_operations() {
        let (cache, _temp) = create_test_cache();

        // Test setting and getting a string value
        let key = "test_key";
        let value = "test_value".to_string();

        cache.set(key, &value).expect("Failed to set value");
        let retrieved: Option<String> = cache.get(key).expect("Failed to get value");
        assert_eq!(retrieved, Some(value));
    }

    #[test]
    fn test_cache_weather_forecast() {
        let (cache, _temp) = create_test_cache();
        let forecast = create_test_forecast();
        let key = "weather_test";

        cache
            .set_weather_forecast(key, forecast.clone())
            .expect("Failed to set forecast");
        let retrieved = cache
            .get_weather_forecast(key)
            .expect("Failed to get forecast");

        let retrieved_forecast = retrieved;
        assert_eq!(retrieved_forecast.location.name, forecast.location.name);
        assert_eq!(retrieved_forecast.forecasts.len(), forecast.forecasts.len());
    }

    #[test]
    fn test_cache_ttl_expiry() {
        let (cache, _temp) = create_test_cache();

        // Set a value with 0 TTL (should expire immediately)
        let key = "expire_test";
        let value = "test_value".to_string();

        cache
            .set_with_ttl(key, &value, 0)
            .expect("Failed to set value");

        // Should be None because TTL is 0
        let retrieved: Result<String> = cache.get(key);
        assert!(retrieved.is_err());
    }

    #[test]
    fn test_cache_contains() {
        let (cache, _temp) = create_test_cache();
        let key = "contains_test";

        assert!(!cache.contains(key));

        cache
            .set(key, &"value".to_string())
            .expect("Failed to set value");
        assert!(cache.contains(key));
    }

    #[test]
    fn test_cache_remove() {
        let (cache, _temp) = create_test_cache();
        let key = "remove_test";
        let value = "test_value".to_string();

        cache.set(key, &value).expect("Failed to set value");
        assert!(cache.contains(key));

        let removed = cache.remove(key).expect("Failed to remove key");
        assert!(removed);
        assert!(!cache.contains(key));
    }

    #[test]
    fn test_cache_keys() {
        let (cache, _temp) = create_test_cache();

        // Add some test data
        cache
            .set("key1", &"value1".to_string())
            .expect("Failed to set key1");
        cache
            .set("key2", &"value2".to_string())
            .expect("Failed to set key2");

        let keys = cache.keys().expect("Failed to get keys");
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"key1".to_string()));
        assert!(keys.contains(&"key2".to_string()));
    }

    #[test]
    fn test_cache_clear() {
        let (cache, _temp) = create_test_cache();

        // Add some test data
        cache
            .set("key1", &"value1".to_string())
            .expect("Failed to set key1");
        cache
            .set("key2", &"value2".to_string())
            .expect("Failed to set key2");

        let stats_before = cache.stats().expect("Failed to get stats");
        assert_eq!(stats_before.total_entries, 2);

        cache.clear().expect("Failed to clear cache");

        let stats_after = cache.stats().expect("Failed to get stats");
        assert_eq!(stats_after.total_entries, 0);
    }

    #[test]
    fn test_cache_stats() {
        let (cache, _temp) = create_test_cache();

        let stats_empty = cache.stats().expect("Failed to get stats");
        assert_eq!(stats_empty.total_entries, 0);
        assert_eq!(stats_empty.valid_entries, 0);

        // Add some valid entries
        cache
            .set_with_ttl("key1", &"value1".to_string(), 10)
            .expect("Failed to set key1");
        cache
            .set_with_ttl("key2", &"value2".to_string(), 10)
            .expect("Failed to set key2");

        let stats_with_data = cache.stats().expect("Failed to get stats");
        assert_eq!(stats_with_data.total_entries, 2);
        assert_eq!(stats_with_data.valid_entries, 2);
        assert_eq!(stats_with_data.expired_entries, 0);
    }

    #[test]
    fn test_cache_key_generation() {
        let key1 = Cache::weather_cache_key(46.8182, 8.2275, "2023-12-01");
        let key2 = Cache::daily_weather_key(
            46.8182,
            8.2275,
            &chrono::NaiveDate::from_ymd_opt(2023, 12, 1).unwrap(),
        );

        assert_eq!(key1, "weather:46.82:8.23:2023-12-01");
        assert_eq!(key2, "weather:46.82:8.23:2023-12-01");
    }

    #[test]
    fn test_cache_cleanup_expired() {
        let (cache, _temp) = create_test_cache();

        // Add some entries with different TTLs
        cache
            .set_with_ttl("valid", &"value".to_string(), 10)
            .expect("Failed to set valid");
        cache
            .set_with_ttl("expired", &"value".to_string(), 0)
            .expect("Failed to set expired");

        let stats_before = cache.stats().expect("Failed to get stats before cleanup");
        assert_eq!(stats_before.total_entries, 2);

        let removed = cache.cleanup_expired().expect("Failed to cleanup");
        assert_eq!(removed, 1); // Should remove the expired entry

        let stats_after = cache.stats().expect("Failed to get stats after cleanup");
        assert_eq!(stats_after.total_entries, 1);
        assert_eq!(stats_after.valid_entries, 1);
    }
}
