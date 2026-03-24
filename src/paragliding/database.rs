use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    database::{self, Db},
    location::Location,
    paragliding::{ParaglidingSite, ParaglidingSiteProvider},
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
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            location_name: "Gornau/Erz".to_string(),
            location_latitude: 50.7,
            location_longitude: 13.0,
            search_radius_km: 150.0,
            calendar_name: "Paragliding".to_string(),
            minimum_flyable_hours: 2,
        }
    }
}

pub struct CachedParaglidingSiteProvider {
    db: Db,
}

impl CachedParaglidingSiteProvider {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub async fn save_site(&self, site: ParaglidingSite) -> Result<()> {
        let key = format!("site_{}", site.name);
        database::save(&self.db, &key, site).await
    }

    pub async fn delete_site(&self, name: &str) -> Result<()> {
        let key = format!("site_{}", name);
        database::delete(&self.db, &key).await
    }

    pub async fn get_settings(&self) -> Result<Option<UserSettings>> {
        database::get::<UserSettings>(&self.db, SETTINGS_KEY).await
    }

    pub async fn save_settings(&self, settings: &UserSettings) -> Result<()> {
        database::save(&self.db, SETTINGS_KEY, settings.clone()).await
    }
}

impl Default for CachedParaglidingSiteProvider {
    fn default() -> Self {
        panic!("CachedParaglidingSiteProvider requires a database instance");
    }
}

impl ParaglidingSiteProvider for CachedParaglidingSiteProvider {
    async fn fetch_launches_within_radius(
        &self,
        center: &Location,
        radius_km: f64,
    ) -> Vec<(ParaglidingSite, f64)> {
        let sites: Vec<ParaglidingSite> =
            match database::find_by_prefix::<ParaglidingSite>(&self.db, "site_").await {
                Ok(sites) => sites,
                Err(e) => {
                    tracing::error!("Failed to fetch sites from database: {}", e);
                    return vec![];
                }
            };

        if sites.is_empty() {
            tracing::warn!("No sites found in database");
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
        match database::find_by_prefix::<ParaglidingSite>(&self.db, "site_").await {
            Ok(sites) => sites,
            Err(e) => {
                tracing::error!("Failed to fetch all sites from database: {}", e);
                vec![]
            }
        }
    }
}
