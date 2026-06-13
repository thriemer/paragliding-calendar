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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::paragliding::{ParaglidingLaunch, SiteType};
    use tempfile::TempDir;

    fn fresh_repo() -> (TempDir, ParaglidingSiteRepository) {
        let dir = tempfile::tempdir().unwrap();
        let db = fjall::Database::builder(dir.path()).open().unwrap();
        let ks = db
            .keyspace("store", fjall::KeyspaceCreateOptions::default)
            .unwrap();
        let store = Arc::new(PersistentStore::from_keyspace(ks));
        (dir, ParaglidingSiteRepository::new(store))
    }

    fn site_at(name: &str, lat: f64, lon: f64) -> ParaglidingSite {
        ParaglidingSite {
            name: name.into(),
            launches: vec![ParaglidingLaunch {
                site_type: SiteType::Hang,
                location: Location::new(lat, lon, name.into(), "DE".into()),
                direction_degrees_start: 0.0,
                direction_degrees_stop: 360.0,
                elevation: 500.0,
            }],
            landings: vec![],
            country: Some("DE".into()),
            data_source: "test".into(),
            parking_location: None,
            mute_alerts: None,
            rating: None,
            preferred_weather_model: None,
        }
    }

    #[tokio::test]
    async fn save_and_get_settings_round_trip() {
        let (_dir, repo) = fresh_repo();
        let s = UserSettings {
            location_name: "Foo".into(),
            location_latitude: 50.0,
            location_longitude: 13.0,
            search_radius_km: 75.0,
            calendar_name: "Cal".into(),
            minimum_flyable_hours: 3,
            excluded_calendar_names: vec!["work".into()],
        };
        repo.save_settings(&s).await.unwrap();
        let got = repo.get_settings().await.unwrap().unwrap();
        assert_eq!(got.location_name, "Foo");
        assert_eq!(got.search_radius_km, 75.0);
        assert_eq!(got.minimum_flyable_hours, 3);
        assert_eq!(got.excluded_calendar_names, vec!["work".to_string()]);
    }

    #[tokio::test]
    async fn get_settings_returns_none_when_unset() {
        let (_dir, repo) = fresh_repo();
        let got = repo.get_settings().await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn fetch_within_radius_filters_by_distance() {
        let (_dir, repo) = fresh_repo();
        repo.save_site(site_at("near", 50.71, 13.01)).await.unwrap();
        repo.save_site(site_at("far", 52.5, 13.4)).await.unwrap();

        let home = Location::new(50.7, 13.0, "Home".into(), "DE".into());
        let result = repo.fetch_launches_within_radius(&home, 50.0).await;

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0.name, "near");
    }

    #[tokio::test]
    async fn fetch_within_radius_sorts_by_proximity_ascending() {
        let (_dir, repo) = fresh_repo();
        repo.save_site(site_at("mid", 50.75, 13.0)).await.unwrap();
        repo.save_site(site_at("near", 50.71, 13.0)).await.unwrap();
        repo.save_site(site_at("far", 50.85, 13.0)).await.unwrap();

        let home = Location::new(50.7, 13.0, "Home".into(), "DE".into());
        let result = repo.fetch_launches_within_radius(&home, 50.0).await;

        assert_eq!(result.len(), 3);
        let names: Vec<&str> = result.iter().map(|(s, _)| s.name.as_str()).collect();
        assert_eq!(names, vec!["near", "mid", "far"]);
    }

    #[tokio::test]
    async fn fetch_all_sites_returns_every_stored_site() {
        let (_dir, repo) = fresh_repo();
        repo.save_site(site_at("A", 50.71, 13.0)).await.unwrap();
        repo.save_site(site_at("B", 60.0, 20.0)).await.unwrap();

        let all = repo.fetch_all_sites().await;
        assert_eq!(all.len(), 2);
        let names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"A"));
        assert!(names.contains(&"B"));
    }

    #[tokio::test]
    async fn delete_site_removes_it_from_subsequent_fetches() {
        let (_dir, repo) = fresh_repo();
        repo.save_site(site_at("A", 50.71, 13.0)).await.unwrap();
        repo.save_site(site_at("B", 50.72, 13.0)).await.unwrap();
        repo.delete_site("A").await.unwrap();

        let home = Location::new(50.7, 13.0, "Home".into(), "DE".into());
        let result = repo.fetch_launches_within_radius(&home, 50.0).await;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0.name, "B");
    }
}
