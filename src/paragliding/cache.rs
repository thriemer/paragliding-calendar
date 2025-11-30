use std::path::Path;
use std::time::{Duration, SystemTime};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use tracing::{info, warn, debug};

use super::{Result};
use crate::cache::Cache as CacheManager;
use super::{ParaglidingSite, Coordinates};

/// Cache entry for paragliding sites
#[derive(Debug, Serialize, Deserialize)]
pub struct SiteCacheEntry {
    pub sites: Vec<ParaglidingSite>,
    pub cached_at: SystemTime,
    pub expires_at: SystemTime,
    pub source_file_mtime: Option<SystemTime>,
}

/// Cache key for geographic searches
#[derive(Debug, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct SearchCacheKey {
    pub center_lat: i64,  // Lat * 1000000 for precision
    pub center_lng: i64,  // Lng * 1000000 for precision
    pub radius_km: u32,   // Radius in km
    pub data_source: String,
}

impl SearchCacheKey {
    #[must_use] 
    pub fn new(center: &Coordinates, radius_km: f64, data_source: &str) -> Self {
        // Safe conversions: coordinates have limited range, radius is clamped
        let lat_micro = (center.latitude * 1_000_000.0).round();
        let lng_micro = (center.longitude * 1_000_000.0).round();
        let radius_clamped = radius_km.max(0.0).min(f64::from(u32::MAX));
        
        Self {
            center_lat: lat_micro as i64,
            center_lng: lng_micro as i64,
            radius_km: radius_clamped as u32,
            data_source: data_source.to_string(),
        }
    }
}

/// Paragliding site cache manager
pub struct SiteCache {
    cache: CacheManager,
    dhv_cache_duration: Duration,
    api_cache_duration: Duration,
}

impl SiteCache {
    /// Create a new site cache
    #[must_use] 
    pub fn new(cache_manager: CacheManager) -> Self {
        Self {
            cache: cache_manager,
            dhv_cache_duration: Duration::from_secs(24 * 60 * 60), // 24 hours
            api_cache_duration: Duration::from_secs(60 * 60),      // 1 hour
        }
    }
    
    /// Cache DHV sites with file modification time tracking
    pub fn cache_dhv_sites<P: AsRef<Path>>(
        &self,
        xml_path: P,
        sites: &[ParaglidingSite],
    ) -> Result<()> {
        let xml_path = xml_path.as_ref();
        let source_mtime = super::dhv::DHVParser::get_file_mtime(xml_path)?;
        
        let cache_entry = SiteCacheEntry {
            sites: sites.to_vec(),
            cached_at: SystemTime::now(),
            expires_at: SystemTime::now() + self.dhv_cache_duration,
            source_file_mtime: Some(source_mtime),
        };
        
        let cache_key = format!("dhv_sites_{}", 
            xml_path.file_name().unwrap_or_default().to_string_lossy());
        
        self.cache.set(&cache_key, &cache_entry)?;
        info!("Cached {} DHV sites with key: {}", sites.len(), cache_key);
        
        Ok(())
    }
    
    /// Get cached DHV sites if valid and file hasn't changed
    pub fn get_dhv_sites<P: AsRef<Path>>(
        &self,
        xml_path: P,
    ) -> Result<Option<Vec<ParaglidingSite>>> {
        let xml_path = xml_path.as_ref();
        let cache_key = format!("dhv_sites_{}", 
            xml_path.file_name().unwrap_or_default().to_string_lossy());
            
        let entry: Option<SiteCacheEntry> = self.cache.get(&cache_key)?;
        
        if let Some(entry) = entry {
            let now = SystemTime::now();
            
            // Check if cache has expired
            if now > entry.expires_at {
                debug!("DHV cache expired for key: {}", cache_key);
                return Ok(None);
            }
            
            // Check if source file has been modified
            if let Some(cached_mtime) = entry.source_file_mtime {
                if let Ok(current_mtime) = super::dhv::DHVParser::get_file_mtime(xml_path) {
                    if current_mtime > cached_mtime {
                        debug!("DHV file modified, cache invalid for: {}", cache_key);
                        return Ok(None);
                    }
                } else {
                    warn!("Could not check DHV file mtime, assuming cache invalid");
                    return Ok(None);
                }
            }
            
            info!("Retrieved {} sites from DHV cache", entry.sites.len());
            return Ok(Some(entry.sites));
        }
        
        Ok(None)
    }
    
    /// Cache API search results
    pub fn cache_api_search(
        &self,
        search_key: &SearchCacheKey,
        sites: &[ParaglidingSite],
    ) -> Result<()> {
        let cache_entry = SiteCacheEntry {
            sites: sites.to_vec(),
            cached_at: SystemTime::now(),
            expires_at: SystemTime::now() + self.api_cache_duration,
            source_file_mtime: None,
        };
        
        let cache_key = format!("api_search_{search_key:?}");
        self.cache.set(&cache_key, &cache_entry)?;
        
        info!("Cached {} API sites for search: {:?}", sites.len(), search_key);
        Ok(())
    }
    
    /// Get cached API search results
    pub fn get_api_search(
        &self,
        search_key: &SearchCacheKey,
    ) -> Result<Option<Vec<ParaglidingSite>>> {
        let cache_key = format!("api_search_{search_key:?}");
        let entry: Option<SiteCacheEntry> = self.cache.get(&cache_key)?;
        
        if let Some(entry) = entry {
            let now = SystemTime::now();
            
            if now > entry.expires_at {
                debug!("API cache expired for key: {}", cache_key);
                return Ok(None);
            }
            
            info!("Retrieved {} sites from API cache", entry.sites.len());
            return Ok(Some(entry.sites));
        }
        
        Ok(None)
    }
    
    /// Clear all cached site data
    pub fn clear_all(&self) -> Result<()> {
        // This would require extending CacheManager to support pattern-based clearing
        // For now, we'll implement individual key clearing
        info!("Clearing site cache (specific implementation needed)");
        Ok(())
    }
    
    /// Get cache statistics
    #[must_use] 
    pub fn get_stats(&self) -> HashMap<String, usize> {
        // This would require extending CacheManager to provide statistics
        // For now, return empty stats
        HashMap::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use crate::cache::Cache;
    use super::super::{DataSource, SiteCharacteristics};
    
    fn create_test_site() -> ParaglidingSite {
        ParaglidingSite {
            id: "test_site".to_string(),
            name: "Test Site".to_string(),
            coordinates: Coordinates { latitude: 45.0, longitude: 6.0 },
            elevation: Some(1000.0),
            launch_directions: vec![],
            site_type: None,
            country: None,
            data_source: DataSource::DHV,
            characteristics: SiteCharacteristics {
                height_difference_max: None,
                site_url: None,
                access_by_car: None,
                access_by_foot: None,
                access_by_public_transport: None,
                hanggliding: None,
                paragliding: None,
            },
        }
    }
    
    #[test]
    fn test_search_cache_key() {
        let center = Coordinates { latitude: 45.123_456, longitude: 6.789_123 };
        let key = SearchCacheKey::new(&center, 50.0, "test_source");
        
        assert_eq!(key.center_lat, 45_123_456);
        assert_eq!(key.center_lng, 6_789_123);
        assert_eq!(key.radius_km, 50);
        assert_eq!(key.data_source, "test_source");
    }
    
    #[test]
    fn test_site_cache() {
        let temp_dir = TempDir::new().unwrap();
        let cache_manager = Cache::new(temp_dir.path(), 24).unwrap();
        let site_cache = SiteCache::new(cache_manager);
        
        let sites = vec![create_test_site()];
        let search_key = SearchCacheKey::new(
            &Coordinates { latitude: 45.0, longitude: 6.0 },
            50.0,
            "test"
        );
        
        // Test API search caching
        site_cache.cache_api_search(&search_key, &sites).unwrap();
        let cached_sites = site_cache.get_api_search(&search_key).unwrap();
        
        assert!(cached_sites.is_some());
        let cached_sites = cached_sites.unwrap();
        assert_eq!(cached_sites.len(), 1);
        assert_eq!(cached_sites[0].name, "Test Site");
    }
}