use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::domain::{
    happening::{Happening, HappeningDate},
    location::Location,
    paragliding::{ParaglidingLanding, ParaglidingLaunch, ParaglidingSite, SiteType},
    ports::{HappeningRepository, SettingsRepository, SiteRepository, TourRepository},
    settings::UserSettings,
    tour::Tour,
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

#[async_trait]
impl TourRepository for PostgresRepository {
    async fn count(&self) -> Result<i64> {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM outdoor_tours")
            .fetch_one(&self.pool)
            .await?;
        Ok(n)
    }

    async fn save_batch(&self, tours: Vec<Tour>) -> Result<usize> {
        let mut tx = self.pool.begin().await?;
        let mut saved = 0usize;
        for tour in &tours {
            sqlx::query(
                "INSERT INTO outdoor_tours (id, title, category, location, location_name, description,
                    duration_minutes, length_meters, ascent_meters, descent_meters,
                    difficulty, stamina, landscape, experience, is_loop, season_bitmask, raw_json, source_url)
                 VALUES ($1, $2, $3, ST_SetSRID(ST_MakePoint($4, $5), 4326), $6, $7,
                    $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18::jsonb, $19)
                 ON CONFLICT (id) DO UPDATE SET
                    title = EXCLUDED.title, category = EXCLUDED.category,
                    location = EXCLUDED.location, location_name = EXCLUDED.location_name,
                    description = EXCLUDED.description,
                    duration_minutes = EXCLUDED.duration_minutes, length_meters = EXCLUDED.length_meters,
                    ascent_meters = EXCLUDED.ascent_meters, descent_meters = EXCLUDED.descent_meters,
                    difficulty = EXCLUDED.difficulty, stamina = EXCLUDED.stamina,
                    landscape = EXCLUDED.landscape, experience = EXCLUDED.experience,
                    is_loop = EXCLUDED.is_loop, season_bitmask = EXCLUDED.season_bitmask,
                    raw_json = EXCLUDED.raw_json, source_url = EXCLUDED.source_url",
            )
            .bind(&tour.id)
            .bind(&tour.title)
            .bind(&tour.category)
            .bind(tour.location.longitude)
            .bind(tour.location.latitude)
            .bind(&tour.location.name)
            .bind(&tour.description)
            .bind(tour.duration_minutes as i32)
            .bind(tour.length_meters as i32)
            .bind(tour.ascent_meters as i32)
            .bind(tour.descent_meters as i32)
            .bind(tour.difficulty as i16)
            .bind(tour.stamina as i16)
            .bind(tour.landscape as i16)
            .bind(tour.experience as i16)
            .bind(tour.is_loop)
            .bind(tour.season_bitmask as i16)
            .bind(&tour.raw_json)
            .bind(&tour.source_url)
            .execute(&mut *tx)
            .await?;
            saved += 1;
        }
        tx.commit().await?;
        Ok(saved)
    }

    async fn find_within_radius(
        &self,
        center: &Location,
        radius_km: f64,
    ) -> Result<Vec<(Tour, f64)>> {
        let rows: Vec<TourWithDistRow> = sqlx::query_as(
            "SELECT id, title, category, ST_X(location) AS lon, ST_Y(location) AS lat,
                    location_name, description, duration_minutes, length_meters,
                    ascent_meters, descent_meters, difficulty, stamina, landscape, experience,
                    is_loop, season_bitmask, raw_json::text AS raw_json, source_url,
                    ST_Distance(location::geography, ST_SetSRID(ST_MakePoint($1, $2), 4326)::geography) / 1000.0 AS distance_km
             FROM outdoor_tours
             WHERE ST_DWithin(location::geography, ST_SetSRID(ST_MakePoint($1, $2), 4326)::geography, $3)
             ORDER BY distance_km",
        )
        .bind(center.longitude)
        .bind(center.latitude)
        .bind(radius_km * 1000.0)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into_tour()).collect())
    }
}

#[derive(sqlx::FromRow)]
struct TourWithDistRow {
    id: String,
    title: String,
    category: String,
    lon: f64,
    lat: f64,
    location_name: String,
    description: String,
    duration_minutes: i32,
    length_meters: i32,
    ascent_meters: i32,
    descent_meters: i32,
    difficulty: i16,
    stamina: i16,
    landscape: i16,
    experience: i16,
    is_loop: bool,
    season_bitmask: i16,
    raw_json: String,
    source_url: String,
    distance_km: f64,
}

impl TourWithDistRow {
    fn into_tour(self) -> (Tour, f64) {
        (
            Tour {
                id: self.id,
                title: self.title,
                category: self.category,
                location: Location::new(self.lat, self.lon, self.location_name, String::new()),
                description: self.description,
                duration_minutes: self.duration_minutes as u32,
                length_meters: self.length_meters as u32,
                ascent_meters: self.ascent_meters as u32,
                descent_meters: self.descent_meters as u32,
                difficulty: self.difficulty as u8,
                stamina: self.stamina as u8,
                landscape: self.landscape as u8,
                experience: self.experience as u8,
                is_loop: self.is_loop,
                season_bitmask: self.season_bitmask as u16,
                source_url: self.source_url,
                raw_json: self.raw_json,
            },
            self.distance_km,
        )
    }
}

#[async_trait]
impl HappeningRepository for PostgresRepository {
    async fn count(&self) -> Result<i64> {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM outdooractive_events")
            .fetch_one(&self.pool)
            .await?;
        Ok(n)
    }

    async fn save_batch(&self, events: Vec<Happening>) -> Result<usize> {
        let mut saved = 0usize;
        // Chunked transactions: the event set can be large, so avoid one giant transaction.
        for chunk in events.chunks(500) {
            let mut tx = self.pool.begin().await?;
            for event in chunk {
                let (lon, lat) = match &event.location {
                    Some(l) => (Some(l.longitude), Some(l.latitude)),
                    None => (None, None),
                };
                sqlx::query(
                    r#"INSERT INTO outdooractive_events
                       (id, title, location, category_id, category_title, category_keys,
                        description_short, description_long, homepage, address, organizer,
                        schedule_rules, data, source_url)
                       VALUES ($1, $2,
                           CASE WHEN $3::double precision IS NOT NULL
                           THEN ST_SetSRID(ST_MakePoint($3, $4), 4326) END,
                           $5, $6, $7, $8, $9, $10, $11::jsonb, $12,
                           $13::jsonb, $14::jsonb, $15)
                       ON CONFLICT (id) DO UPDATE SET
                           title = EXCLUDED.title,
                           location = EXCLUDED.location,
                           category_id = EXCLUDED.category_id,
                           category_title = EXCLUDED.category_title,
                           category_keys = EXCLUDED.category_keys,
                           description_short = EXCLUDED.description_short,
                           description_long = EXCLUDED.description_long,
                           homepage = EXCLUDED.homepage,
                           address = EXCLUDED.address,
                           organizer = EXCLUDED.organizer,
                           schedule_rules = EXCLUDED.schedule_rules,
                           data = EXCLUDED.data,
                           source_url = EXCLUDED.source_url,
                           updated_at = now()"#,
                )
                .bind(&event.id)
                .bind(&event.title)
                .bind(lon)
                .bind(lat)
                .bind(&event.category_id)
                .bind(&event.category_title)
                .bind(&event.category_keys)
                .bind(&event.description_short)
                .bind(&event.description_long)
                .bind(&event.homepage)
                .bind(event.address.as_ref())
                .bind(&event.organizer)
                .bind(event.schedule_rules.as_ref())
                .bind(&event.data)
                .bind(&event.source_url)
                .execute(&mut *tx)
                .await?;

                sqlx::query("DELETE FROM outdooractive_event_dates WHERE event_id = $1")
                    .bind(&event.id)
                    .execute(&mut *tx)
                    .await?;

                for date in &event.dates {
                    sqlx::query(
                        "INSERT INTO outdooractive_event_dates (event_id, time_from, time_to, date_text)
                         VALUES ($1, $2, $3, $4)",
                    )
                    .bind(&event.id)
                    .bind(date.time_from)
                    .bind(date.time_to)
                    .bind(&date.date_text)
                    .execute(&mut *tx)
                    .await?;
                }
                saved += 1;
            }
            tx.commit().await?;
        }
        Ok(saved)
    }

    async fn find_within_radius_and_time(
        &self,
        center: &Location,
        radius_km: f64,
        time_from: DateTime<Utc>,
        time_to: DateTime<Utc>,
    ) -> Result<Vec<(Happening, f64)>> {
        let rows: Vec<EventWithDistRow> = sqlx::query_as(
            r#"SELECT
                e.id, e.title,
                ST_X(e.location) AS lon, ST_Y(e.location) AS lat,
                e.category_id, e.category_title, e.category_keys,
                e.description_short, e.description_long, e.homepage,
                e.address, e.organizer, e.schedule_rules, e.data, e.source_url,
                jsonb_agg(
                    jsonb_build_object(
                        'time_from', d.time_from,
                        'time_to', d.time_to,
                        'date_text', d.date_text
                    ) ORDER BY d.time_from
                ) AS dates,
                ST_Distance(e.location::geography, ST_SetSRID(ST_MakePoint($1, $2), 4326)::geography) / 1000.0 AS distance_km
             FROM outdooractive_events e
             INNER JOIN outdooractive_event_dates d ON d.event_id = e.id
                AND d.time_from < $4
                AND d.time_to > $3
             WHERE e.location IS NOT NULL
                AND ST_DWithin(e.location::geography, ST_SetSRID(ST_MakePoint($1, $2), 4326)::geography, $5)
             GROUP BY e.id
             ORDER BY distance_km"#,
        )
        .bind(center.longitude)
        .bind(center.latitude)
        .bind(time_from)
        .bind(time_to)
        .bind(radius_km * 1000.0)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(|r| r.try_into_event()).collect()
    }
}

#[derive(sqlx::FromRow)]
struct EventWithDistRow {
    id: String,
    title: String,
    lon: Option<f64>,
    lat: Option<f64>,
    category_id: Option<String>,
    category_title: Option<String>,
    category_keys: Vec<String>,
    description_short: Option<String>,
    description_long: Option<String>,
    homepage: Option<String>,
    address: Option<serde_json::Value>,
    organizer: Option<String>,
    schedule_rules: Option<serde_json::Value>,
    data: serde_json::Value,
    source_url: String,
    dates: serde_json::Value,
    distance_km: f64,
}

impl EventWithDistRow {
    fn try_into_event(self) -> Result<(Happening, f64)> {
        let location = match (self.lon, self.lat) {
            (Some(lon), Some(lat)) => Some(Location::new(lat, lon, String::new(), String::new())),
            _ => None,
        };

        let dates = self
            .dates
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        let time_from = v
                            .get("time_from")?
                            .as_str()?
                            .parse::<DateTime<Utc>>()
                            .ok()?;
                        let time_to = v.get("time_to")?.as_str()?.parse::<DateTime<Utc>>().ok()?;
                        let date_text = v
                            .get("date_text")
                            .and_then(|v| v.as_str().map(String::from));
                        Some(HappeningDate {
                            time_from,
                            time_to,
                            date_text,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let event = Happening {
            id: self.id,
            title: self.title,
            location,
            category_id: self.category_id,
            category_title: self.category_title,
            category_keys: self.category_keys,
            description_short: self.description_short,
            description_long: self.description_long,
            homepage: self.homepage,
            address: self.address,
            organizer: self.organizer,
            schedule_rules: self.schedule_rules,
            dates,
            source_url: self.source_url,
            data: self.data,
        };

        Ok((event, self.distance_km))
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
        let result = SiteRepository::find_within_radius(&repo, &home, 50.0)
            .await
            .unwrap();

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
        let result = SiteRepository::find_within_radius(&repo, &home, 50.0)
            .await
            .unwrap();

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

    async fn insert_event(pool: &PgPool, id: &str, title: &str, lat: f64, lon: f64) {
        sqlx::query(
            "INSERT INTO outdooractive_events (id, title, location, category_keys, data)
             VALUES ($1, $2, ST_SetSRID(ST_MakePoint($3, $4), 4326), $5, '{}'::jsonb)",
        )
        .bind(id)
        .bind(title)
        .bind(lon)
        .bind(lat)
        .bind(Vec::<String>::new())
        .execute(pool)
        .await
        .unwrap();
    }

    async fn insert_date(
        pool: &PgPool,
        event_id: &str,
        time_from: DateTime<Utc>,
        time_to: DateTime<Utc>,
    ) {
        sqlx::query(
            "INSERT INTO outdooractive_event_dates (event_id, time_from, time_to)
             VALUES ($1, $2, $3)",
        )
        .bind(event_id)
        .bind(time_from)
        .bind(time_to)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn find_events_within_radius_and_time_returns_matching_events() {
        let repo = PostgresRepository::new(test_pool().await);
        let pool = &repo.pool;

        insert_event(pool, "e1", "Mountain Hike", 50.71, 13.01).await;
        let now = Utc::now();
        insert_date(
            pool,
            "e1",
            now + chrono::Duration::hours(1),
            now + chrono::Duration::hours(3),
        )
        .await;

        let home = Location::new(50.7, 13.0, "Home".into(), "DE".into());
        let result = repo
            .find_within_radius_and_time(&home, 50.0, now, now + chrono::Duration::days(7))
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0.title, "Mountain Hike");
    }

    #[tokio::test]
    async fn find_events_excludes_events_outside_radius() {
        let repo = PostgresRepository::new(test_pool().await);
        let pool = &repo.pool;

        insert_event(pool, "e1", "Far Event", 55.0, 13.0).await;
        let now = Utc::now();
        insert_date(
            pool,
            "e1",
            now + chrono::Duration::hours(1),
            now + chrono::Duration::hours(3),
        )
        .await;

        let home = Location::new(50.7, 13.0, "Home".into(), "DE".into());
        let result = repo
            .find_within_radius_and_time(&home, 50.0, now, now + chrono::Duration::days(7))
            .await
            .unwrap();

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn find_events_excludes_events_outside_time_window() {
        let repo = PostgresRepository::new(test_pool().await);
        let pool = &repo.pool;

        insert_event(pool, "e1", "Past Event", 50.71, 13.01).await;
        let now = Utc::now();
        insert_date(
            pool,
            "e1",
            now - chrono::Duration::days(10),
            now - chrono::Duration::days(9),
        )
        .await;

        let home = Location::new(50.7, 13.0, "Home".into(), "DE".into());
        let result = repo
            .find_within_radius_and_time(&home, 50.0, now, now + chrono::Duration::days(7))
            .await
            .unwrap();

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn find_events_returns_distance_in_km() {
        let repo = PostgresRepository::new(test_pool().await);
        let pool = &repo.pool;

        insert_event(pool, "e1", "Near", 50.71, 13.01).await;
        let now = Utc::now();
        insert_date(
            pool,
            "e1",
            now + chrono::Duration::hours(1),
            now + chrono::Duration::hours(3),
        )
        .await;

        let home = Location::new(50.7, 13.0, "Home".into(), "DE".into());
        let result = repo
            .find_within_radius_and_time(&home, 50.0, now, now + chrono::Duration::days(7))
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        let dist = result[0].1;
        assert!(dist > 0.0, "distance should be positive, got {dist}");
        assert!(dist < 2.0, "expected ~1 km, got {dist}");
    }

    #[tokio::test]
    async fn find_events_returns_multiple_dates_per_event() {
        let repo = PostgresRepository::new(test_pool().await);
        let pool = &repo.pool;

        insert_event(pool, "e1", "Recurring", 50.71, 13.01).await;
        let now = Utc::now();
        insert_date(
            pool,
            "e1",
            now + chrono::Duration::hours(1),
            now + chrono::Duration::hours(2),
        )
        .await;
        insert_date(
            pool,
            "e1",
            now + chrono::Duration::days(1),
            now + chrono::Duration::days(1) + chrono::Duration::hours(2),
        )
        .await;

        let home = Location::new(50.7, 13.0, "Home".into(), "DE".into());
        let result = repo
            .find_within_radius_and_time(&home, 50.0, now, now + chrono::Duration::days(7))
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0.dates.len(), 2);
    }

    #[tokio::test]
    async fn find_events_filters_by_overlap_not_containment() {
        let repo = PostgresRepository::new(test_pool().await);
        let pool = &repo.pool;

        insert_event(pool, "e1", "Ongoing", 50.71, 13.01).await;
        let now = Utc::now();
        // Event starts before the query window but ends inside it
        insert_date(
            pool,
            "e1",
            now - chrono::Duration::days(1),
            now + chrono::Duration::days(1),
        )
        .await;

        let home = Location::new(50.7, 13.0, "Home".into(), "DE".into());
        let result = repo
            .find_within_radius_and_time(&home, 50.0, now, now + chrono::Duration::days(7))
            .await
            .unwrap();

        assert_eq!(result.len(), 1, "overlapping events should be included");
    }

    #[tokio::test]
    async fn find_events_sorts_by_proximity() {
        let repo = PostgresRepository::new(test_pool().await);
        let pool = &repo.pool;

        insert_event(pool, "mid", "Mid", 50.75, 13.0).await;
        insert_event(pool, "near", "Near", 50.71, 13.0).await;
        insert_event(pool, "far", "Far", 50.85, 13.0).await;
        let now = Utc::now();
        for id in &["mid", "near", "far"] {
            insert_date(pool, id, now, now + chrono::Duration::hours(1)).await;
        }

        let home = Location::new(50.7, 13.0, "Home".into(), "DE".into());
        let result = repo
            .find_within_radius_and_time(&home, 50.0, now, now + chrono::Duration::days(7))
            .await
            .unwrap();

        assert_eq!(result.len(), 3);
        let titles: Vec<&str> = result.iter().map(|(e, _)| e.title.as_str()).collect();
        assert_eq!(titles, vec!["Near", "Mid", "Far"]);
    }
}
