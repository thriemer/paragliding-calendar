use std::sync::Arc;

use anyhow::Result;

use crate::{
    adapters::store::PersistentStore,
    domain::{
        location::Location,
        paragliding::{ParaglidingSite, ParaglidingSiteProvider, UserSettings},
    },
};

const SETTINGS_KEY: &str = "user_settings";

pub struct ParaglidingSiteRepository {
    store: Arc<PersistentStore>,
}

impl ParaglidingSiteRepository {
    pub fn new(store: Arc<PersistentStore>) -> Self {
        Self { store }
    }

    pub async fn save_site(&self, site: ParaglidingSite) -> Result<()> {
        let key = format!("site_{}", site.name);
        self.store.put(&key, site).await
    }

    pub async fn delete_site(&self, name: &str) -> Result<()> {
        let key = format!("site_{}", name);
        self.store.remove(&key).await
    }

    pub async fn get_settings(&self) -> Result<Option<UserSettings>> {
        self.store.get::<UserSettings>(SETTINGS_KEY).await
    }

    pub async fn save_settings(&self, settings: &UserSettings) -> Result<()> {
        self.store.put(SETTINGS_KEY, settings.clone()).await
    }
}

impl ParaglidingSiteProvider for ParaglidingSiteRepository {
    async fn fetch_launches_within_radius(
        &self,
        center: &Location,
        radius_km: f64,
    ) -> Vec<(ParaglidingSite, f64)> {
        let sites: Vec<ParaglidingSite> = match self.store.get_all_starting_with("site_").await {
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
        match self.store.get_all_starting_with("site_").await {
            Ok(sites) => sites,
            Err(e) => {
                tracing::error!("Failed to fetch all sites from store: {}", e);
                vec![]
            }
        }
    }
}
