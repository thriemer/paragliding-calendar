use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;

use crate::domain::{
    location::Location,
    paragliding::{ParaglidingLanding, ParaglidingLaunch, ParaglidingSite, SiteType, UserSettings},
    ports::{SettingsRepository, SiteRepository},
};

pub struct PostgresRepository {
    pool: PgPool,
}

impl PostgresRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SiteRepository for PostgresRepository {
    async fn save(&self, site: ParaglidingSite) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        let (parking_lon, parking_lat, parking_name, parking_country) = match &site.parking_location
        {
            Some(loc) => (
                Some(loc.longitude),
                Some(loc.latitude),
                Some(loc.name.as_str()),
                Some(loc.country.as_str()),
            ),
            None => (None, None, None, None),
        };

        sqlx::query(
            "INSERT INTO paragliding_sites (name, country, data_source, parking_location, parking_name, parking_country, mute_alerts, rating, preferred_weather_model)
             VALUES ($1, $2, $3, CASE WHEN $4::double precision IS NOT NULL THEN ST_SetSRID(ST_MakePoint($4, $5), 4326) END, $6, $7, $8, $9::smallint, $10)
             ON CONFLICT (name) DO UPDATE SET
                country = EXCLUDED.country,
                data_source = EXCLUDED.data_source,
                parking_location = EXCLUDED.parking_location,
                parking_name = EXCLUDED.parking_name,
                parking_country = EXCLUDED.parking_country,
                mute_alerts = EXCLUDED.mute_alerts,
                rating = EXCLUDED.rating,
                preferred_weather_model = EXCLUDED.preferred_weather_model",
        )
        .bind(&site.name)
        .bind(&site.country)
        .bind(&site.data_source)
        .bind(parking_lon)
        .bind(parking_lat)
        .bind(parking_name)
        .bind(parking_country)
        .bind(site.mute_alerts)
        .bind(site.rating.map(|r| r as i16))
        .bind(&site.preferred_weather_model)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM paragliding_launches WHERE site_name = $1")
            .bind(&site.name)
            .execute(&mut *tx)
            .await?;

        for launch in &site.launches {
            let site_type_str = match launch.site_type {
                SiteType::Hang => "Hang",
                SiteType::Winch => "Winch",
            };
            sqlx::query(
                "INSERT INTO paragliding_launches (site_name, site_type, location, name, country, direction_degrees_start, direction_degrees_stop, elevation)
                 VALUES ($1, $2, ST_SetSRID(ST_MakePoint($3, $4), 4326), $5, $6, $7, $8, $9)",
            )
            .bind(&site.name)
            .bind(site_type_str)
            .bind(launch.location.longitude)
            .bind(launch.location.latitude)
            .bind(&launch.location.name)
            .bind(&launch.location.country)
            .bind(launch.direction_degrees_start)
            .bind(launch.direction_degrees_stop)
            .bind(launch.elevation)
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query("DELETE FROM paragliding_landings WHERE site_name = $1")
            .bind(&site.name)
            .execute(&mut *tx)
            .await?;

        for landing in &site.landings {
            sqlx::query(
                "INSERT INTO paragliding_landings (site_name, location, name, country, elevation)
                 VALUES ($1, ST_SetSRID(ST_MakePoint($2, $3), 4326), $4, $5, $6)",
            )
            .bind(&site.name)
            .bind(landing.location.longitude)
            .bind(landing.location.latitude)
            .bind(&landing.location.name)
            .bind(&landing.location.country)
            .bind(landing.elevation)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn delete(&self, name: &str) -> Result<()> {
        sqlx::query("DELETE FROM paragliding_sites WHERE name = $1")
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn find_all(&self) -> Result<Vec<ParaglidingSite>> {
        let site_rows: Vec<SiteRow> = sqlx::query_as(
            "SELECT name, country, data_source, ST_X(parking_location) AS parking_lon, ST_Y(parking_location) AS parking_lat, parking_name, parking_country, mute_alerts, rating, preferred_weather_model
             FROM paragliding_sites",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut sites = Vec::with_capacity(site_rows.len());
        for row in site_rows {
            sites.push(build_site(&self.pool, row).await?);
        }
        Ok(sites)
    }

    async fn find_within_radius(
        &self,
        center: &Location,
        radius_km: f64,
    ) -> Result<Vec<(ParaglidingSite, f64)>> {
        let rows: Vec<SiteWithDistanceRow> = sqlx::query_as(
            "SELECT s.name, s.country, s.data_source,
                    ST_X(s.parking_location) AS parking_lon, ST_Y(s.parking_location) AS parking_lat,
                    s.parking_name, s.parking_country, s.mute_alerts, s.rating, s.preferred_weather_model,
                    MIN(ST_Distance(l.location::geography, ST_SetSRID(ST_MakePoint($1, $2), 4326)::geography)) / 1000.0 AS distance_km
             FROM paragliding_sites s
             JOIN paragliding_launches l ON l.site_name = s.name
             WHERE ST_DWithin(l.location::geography, ST_SetSRID(ST_MakePoint($1, $2), 4326)::geography, $3)
             GROUP BY s.name
             ORDER BY distance_km",
        )
        .bind(center.longitude)
        .bind(center.latitude)
        .bind(radius_km * 1000.0)
        .fetch_all(&self.pool)
        .await?;

        let mut result = Vec::with_capacity(rows.len());
        for r in rows {
            result.push((build_site(&self.pool, r.site).await?, r.distance_km));
        }
        Ok(result)
    }
}

#[async_trait]
impl SettingsRepository for PostgresRepository {
    async fn get(&self) -> Result<Option<UserSettings>> {
        let row: Option<(String, f64, f64, f64, String, i32, Vec<String>)> = sqlx::query_as(
            "SELECT location_name, ST_X(location), ST_Y(location), search_radius_km, calendar_name, minimum_flyable_hours, excluded_calendar_names
             FROM user_settings WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(
            |(location_name, lon, lat, search_radius_km, calendar_name, min_hours, excluded)| {
                UserSettings {
                    location_name,
                    location_latitude: lat,
                    location_longitude: lon,
                    search_radius_km,
                    calendar_name,
                    minimum_flyable_hours: min_hours as u32,
                    excluded_calendar_names: excluded,
                }
            },
        ))
    }

    async fn save(&self, settings: &UserSettings) -> Result<()> {
        sqlx::query(
            "INSERT INTO user_settings (id, location_name, location, search_radius_km, calendar_name, minimum_flyable_hours, excluded_calendar_names)
             VALUES (1, $1, ST_SetSRID(ST_MakePoint($2, $3), 4326), $4, $5, $6, $7)
             ON CONFLICT (id) DO UPDATE SET
                location_name = EXCLUDED.location_name,
                location = EXCLUDED.location,
                search_radius_km = EXCLUDED.search_radius_km,
                calendar_name = EXCLUDED.calendar_name,
                minimum_flyable_hours = EXCLUDED.minimum_flyable_hours,
                excluded_calendar_names = EXCLUDED.excluded_calendar_names",
        )
        .bind(&settings.location_name)
        .bind(settings.location_longitude)
        .bind(settings.location_latitude)
        .bind(settings.search_radius_km)
        .bind(&settings.calendar_name)
        .bind(settings.minimum_flyable_hours as i32)
        .bind(&settings.excluded_calendar_names)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct SiteRow {
    name: String,
    country: Option<String>,
    data_source: String,
    parking_lon: Option<f64>,
    parking_lat: Option<f64>,
    parking_name: Option<String>,
    parking_country: Option<String>,
    mute_alerts: Option<bool>,
    rating: Option<i16>,
    preferred_weather_model: Option<String>,
}

#[derive(sqlx::FromRow)]
struct LaunchRow {
    site_type: String,
    lon: f64,
    lat: f64,
    name: String,
    country: String,
    direction_degrees_start: f64,
    direction_degrees_stop: f64,
    elevation: f64,
}

#[derive(sqlx::FromRow)]
struct LandingRow {
    lon: f64,
    lat: f64,
    name: String,
    country: String,
    elevation: f64,
}

#[derive(sqlx::FromRow)]
struct SiteWithDistanceRow {
    #[sqlx(flatten)]
    site: SiteRow,
    distance_km: f64,
}

async fn build_site(pool: &PgPool, row: SiteRow) -> Result<ParaglidingSite> {
    let launches: Vec<LaunchRow> = sqlx::query_as(
        "SELECT site_type, ST_X(location) AS lon, ST_Y(location) AS lat, name, country, direction_degrees_start, direction_degrees_stop, elevation
         FROM paragliding_launches WHERE site_name = $1",
    )
    .bind(&row.name)
    .fetch_all(pool)
    .await?;

    let landings: Vec<LandingRow> = sqlx::query_as(
        "SELECT ST_X(location) AS lon, ST_Y(location) AS lat, name, country, elevation
         FROM paragliding_landings WHERE site_name = $1",
    )
    .bind(&row.name)
    .fetch_all(pool)
    .await?;

    let parking_location = match (row.parking_lon, row.parking_lat) {
        (Some(lon), Some(lat)) => Some(Location::new(
            lat,
            lon,
            row.parking_name.unwrap_or_default(),
            row.parking_country.unwrap_or_default(),
        )),
        _ => None,
    };

    Ok(ParaglidingSite {
        name: row.name,
        launches: launches
            .into_iter()
            .map(|l| ParaglidingLaunch {
                site_type: match l.site_type.as_str() {
                    "Winch" => SiteType::Winch,
                    _ => SiteType::Hang,
                },
                location: Location::new(l.lat, l.lon, l.name, l.country),
                direction_degrees_start: l.direction_degrees_start,
                direction_degrees_stop: l.direction_degrees_stop,
                elevation: l.elevation,
            })
            .collect(),
        landings: landings
            .into_iter()
            .map(|l| ParaglidingLanding {
                location: Location::new(l.lat, l.lon, l.name, l.country),
                elevation: l.elevation,
            })
            .collect(),
        country: row.country,
        data_source: row.data_source,
        parking_location,
        mute_alerts: row.mute_alerts,
        rating: row.rating.map(|r| r as u8),
        preferred_weather_model: row.preferred_weather_model,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::test_pool;

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
    async fn save_and_find_all() {
        let repo = PostgresRepository::new(test_pool().await);
        SiteRepository::save(&repo, site_at("A", 50.71, 13.0))
            .await
            .unwrap();
        SiteRepository::save(&repo, site_at("B", 60.0, 20.0))
            .await
            .unwrap();

        let all = repo.find_all().await.unwrap();
        assert_eq!(all.len(), 2);
        let names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"A"));
        assert!(names.contains(&"B"));
    }

    #[tokio::test]
    async fn save_upserts_existing_site() {
        let repo = PostgresRepository::new(test_pool().await);
        let mut site = site_at("A", 50.71, 13.0);
        SiteRepository::save(&repo, site.clone()).await.unwrap();

        site.rating = Some(5);
        SiteRepository::save(&repo, site).await.unwrap();

        let all = repo.find_all().await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].rating, Some(5));
    }

    #[tokio::test]
    async fn delete_removes_site_and_children() {
        let repo = PostgresRepository::new(test_pool().await);
        SiteRepository::save(&repo, site_at("A", 50.71, 13.0))
            .await
            .unwrap();
        SiteRepository::save(&repo, site_at("B", 50.72, 13.0))
            .await
            .unwrap();
        repo.delete("A").await.unwrap();

        let all = repo.find_all().await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "B");
    }

    #[tokio::test]
    async fn find_within_radius_filters_by_distance() {
        let repo = PostgresRepository::new(test_pool().await);
        SiteRepository::save(&repo, site_at("near", 50.71, 13.01))
            .await
            .unwrap();
        SiteRepository::save(&repo, site_at("far", 52.5, 13.4))
            .await
            .unwrap();

        let home = Location::new(50.7, 13.0, "Home".into(), "DE".into());
        let result = repo.find_within_radius(&home, 50.0).await.unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0.name, "near");
    }

    #[tokio::test]
    async fn find_within_radius_sorts_by_proximity() {
        let repo = PostgresRepository::new(test_pool().await);
        SiteRepository::save(&repo, site_at("mid", 50.75, 13.0))
            .await
            .unwrap();
        SiteRepository::save(&repo, site_at("near", 50.71, 13.0))
            .await
            .unwrap();
        SiteRepository::save(&repo, site_at("far", 50.85, 13.0))
            .await
            .unwrap();

        let home = Location::new(50.7, 13.0, "Home".into(), "DE".into());
        let result = repo.find_within_radius(&home, 50.0).await.unwrap();

        assert_eq!(result.len(), 3);
        let names: Vec<&str> = result.iter().map(|(s, _)| s.name.as_str()).collect();
        assert_eq!(names, vec!["near", "mid", "far"]);
    }

    #[tokio::test]
    async fn save_and_get_settings_round_trip() {
        let repo = PostgresRepository::new(test_pool().await);
        let s = UserSettings {
            location_name: "Foo".into(),
            location_latitude: 50.0,
            location_longitude: 13.0,
            search_radius_km: 75.0,
            calendar_name: "Cal".into(),
            minimum_flyable_hours: 3,
            excluded_calendar_names: vec!["work".into()],
        };
        SettingsRepository::save(&repo, &s).await.unwrap();
        let got = SettingsRepository::get(&repo).await.unwrap().unwrap();
        assert_eq!(got.location_name, "Foo");
        assert_eq!(got.search_radius_km, 75.0);
        assert_eq!(got.minimum_flyable_hours, 3);
        assert_eq!(got.excluded_calendar_names, vec!["work".to_string()]);
    }

    #[tokio::test]
    async fn get_settings_returns_none_when_unset() {
        let repo = PostgresRepository::new(test_pool().await);
        let got = SettingsRepository::get(&repo).await.unwrap();
        assert!(got.is_none());
    }
}
