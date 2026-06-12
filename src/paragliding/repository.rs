use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    location::Location,
    paragliding::{ParaglidingSite, ParaglidingSiteProvider},
    store,
};

const SETTINGS_KEY: &str = "user_settings";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettings {
    pub location_name: String,
    pub location_latitude: f64,
    pub location_longitude: f64,
    pub search_radius_km: f64,
    pub calendar_name: String,
    pub minimum_flyable_hours: u32,
    pub excluded_calendar_names: Vec<String>,
}

impl Default for UserSettings {
    fn default() -> Self {
        let calendar_name = "Paragliding".to_string();
        Self {
            //TODO: replace with real location
            location_name: "Gornau/Erz".to_string(),
            location_latitude: 50.7,
            location_longitude: 13.0,
            search_radius_km: 150.0,
            calendar_name: calendar_name.clone(),
            minimum_flyable_hours: 2,
            excluded_calendar_names: vec![calendar_name],
        }
    }
}

pub struct ParaglidingSiteRepository;

impl ParaglidingSiteRepository {
    pub fn new() -> Self {
        Self
    }

    pub async fn save_site(&self, site: ParaglidingSite) -> Result<()> {
        let key = format!("site_{}", site.name);
        store::put(&key, site).await
    }

    pub async fn delete_site(&self, name: &str) -> Result<()> {
        let key = format!("site_{}", name);
        store::remove(&key).await
    }

    pub async fn get_settings() -> Result<Option<UserSettings>> {
        store::get::<UserSettings>(SETTINGS_KEY).await
    }

    pub async fn save_settings(settings: &UserSettings) -> Result<()> {
        store::put(SETTINGS_KEY, settings.clone()).await
    }
}

impl Default for ParaglidingSiteRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl ParaglidingSiteProvider for ParaglidingSiteRepository {
    async fn fetch_launches_within_radius(
        &self,
        center: &Location,
        radius_km: f64,
    ) -> Vec<(ParaglidingSite, f64)> {
        let sites: Vec<ParaglidingSite> = match store::get_all_starting_with("site_").await {
            Ok(sites) => sites,
            Err(e) => {
                tracing::error!("Failed to fetch sites from store: {}", e);
                return vec![];
            }
        };

        if sites.is_empty() {
            tracing::warn!("No sites found in store");
            return vec![];
        }

        let mut results = Vec::new();

        for site in &sites {
            let mut min_distance = f64::INFINITY;

            for launch in &site.launches {
                let distance = center.distance_to(&launch.location);
                if distance < min_distance {
                    min_distance = distance;
                }
            }

            if min_distance <= radius_km {
                results.push((site.clone(), min_distance));
            }
        }

        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        results
    }

    async fn fetch_all_sites(&self) -> Vec<ParaglidingSite> {
        match store::get_all_starting_with("site_").await {
            Ok(sites) => sites,
            Err(e) => {
                tracing::error!("Failed to fetch all sites from store: {}", e);
                vec![]
            }
        }
    }
}
