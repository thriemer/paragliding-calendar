use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use std::collections::HashMap;

use crate::domain::{
    activities::ActivityKind,
    happening::{Happening, HappeningDate},
    image::{ActivityImageLink, DownloadedImage, ImageEmbedding},
    location::Location,
    paragliding::{ParaglidingLanding, ParaglidingLaunch, ParaglidingSite, SiteType},
    ports::{
        ActivityEmbeddingRow, EmbeddingRepository, HappeningRepository, ImageRepository,
        PreferenceRepository, SettingsRepository, SiteRepository, TourRepository,
    },
    preferences::{FeatureWeight, KindModel},
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

    async fn count(&self) -> Result<i64> {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM paragliding_sites")
            .fetch_one(&self.pool)
            .await?;
        Ok(n)
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
                    MIN(ST_Distance(l.location::geography, ST_SetSRID(ST_MakePoint($1, $2), 4326)::geography)) / 1000.0::double precision AS distance_km
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
                    difficulty, stamina, landscape, experience, is_loop, season_bitmask, raw_json, source_url, image_urls)
                 VALUES ($1, $2, $3, ST_SetSRID(ST_MakePoint($4, $5), 4326), $6, $7,
                    $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18::jsonb, $19, $20)
                 ON CONFLICT (id) DO UPDATE SET
                    title = EXCLUDED.title, category = EXCLUDED.category,
                    location = EXCLUDED.location, location_name = EXCLUDED.location_name,
                    description = EXCLUDED.description,
                    duration_minutes = EXCLUDED.duration_minutes, length_meters = EXCLUDED.length_meters,
                    ascent_meters = EXCLUDED.ascent_meters, descent_meters = EXCLUDED.descent_meters,
                    difficulty = EXCLUDED.difficulty, stamina = EXCLUDED.stamina,
                    landscape = EXCLUDED.landscape, experience = EXCLUDED.experience,
                    is_loop = EXCLUDED.is_loop, season_bitmask = EXCLUDED.season_bitmask,
                    raw_json = EXCLUDED.raw_json, source_url = EXCLUDED.source_url,
                    image_urls = EXCLUDED.image_urls",
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
            .bind(&tour.image_urls)
            .execute(&mut *tx)
            .await?;
            saved += 1;
        }
        tx.commit().await?;
        Ok(saved)
    }

    async fn find_all(&self) -> Result<Vec<Tour>> {
        // Reuses TourWithDistRow with a dummy distance — no spatial filter.
        let rows: Vec<TourWithDistRow> = sqlx::query_as(
            "SELECT id, title, category, ST_X(location) AS lon, ST_Y(location) AS lat,
                    location_name, description, duration_minutes, length_meters,
                    ascent_meters, descent_meters, difficulty, stamina, landscape, experience,
                    is_loop, season_bitmask, raw_json::text AS raw_json, source_url, image_urls,
                    0.0::double precision AS distance_km
             FROM outdoor_tours",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.into_tour().0).collect())
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
                    is_loop, season_bitmask, raw_json::text AS raw_json, source_url, image_urls,
                    ST_Distance(location::geography, ST_SetSRID(ST_MakePoint($1, $2), 4326)::geography) / 1000.0::double precision AS distance_km
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
    image_urls: Vec<String>,
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
                image_urls: self.image_urls,
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
                        schedule_rules, data, source_url, image_urls)
                       VALUES ($1, $2,
                           CASE WHEN $3::double precision IS NOT NULL
                           THEN ST_SetSRID(ST_MakePoint($3, $4), 4326) END,
                           $5, $6, $7, $8, $9, $10, $11::jsonb, $12,
                           $13::jsonb, $14::jsonb, $15, $16)
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
                           image_urls = EXCLUDED.image_urls,
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
                .bind(&event.image_urls)
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

    async fn find_all(&self) -> Result<Vec<Happening>> {
        // LEFT JOIN so events with no dates are still returned; FILTER keeps the
        // dates array empty (not [null]) for those. Dummy distance reuses the row.
        let rows: Vec<EventWithDistRow> = sqlx::query_as(
            r#"SELECT
                e.id, e.title,
                ST_X(e.location) AS lon, ST_Y(e.location) AS lat,
                e.category_id, e.category_title, e.category_keys,
                e.description_short, e.description_long, e.homepage,
                e.address, e.organizer, e.schedule_rules, e.data, e.source_url, e.image_urls,
                COALESCE(
                    jsonb_agg(
                        jsonb_build_object(
                            'time_from', d.time_from,
                            'time_to', d.time_to,
                            'date_text', d.date_text
                        ) ORDER BY d.time_from
                    ) FILTER (WHERE d.id IS NOT NULL),
                    '[]'::jsonb
                ) AS dates,
                0.0::double precision AS distance_km
             FROM outdooractive_events e
             LEFT JOIN outdooractive_event_dates d ON d.event_id = e.id
             GROUP BY e.id"#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| r.try_into_event().map(|(h, _)| h))
            .collect()
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
                e.address, e.organizer, e.schedule_rules, e.data, e.source_url, e.image_urls,
                jsonb_agg(
                    jsonb_build_object(
                        'time_from', d.time_from,
                        'time_to', d.time_to,
                        'date_text', d.date_text
                    ) ORDER BY d.time_from
                ) AS dates,
                ST_Distance(e.location::geography, ST_SetSRID(ST_MakePoint($1, $2), 4326)::geography) / 1000.0::double precision AS distance_km
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
    image_urls: Vec<String>,
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
            image_urls: self.image_urls,
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

#[derive(sqlx::FromRow)]
#[allow(dead_code)] // fields read via find_all (Phase 4 planner read path; tested)
struct ActivityEmbeddingDbRow {
    activity_id: String,
    kind: String,
    embedding: Vec<f64>,
    pca_dims: Vec<f64>,
    features: Vec<f64>,
}

/// One element of `preference_model.features` (JSONB). Shape mirrors PLAN.md §0b.
#[derive(serde::Serialize, serde::Deserialize)]
struct FeatureWeightJson {
    name: String,
    weight: f64,
    norm_mean: f64,
    norm_std: f64,
}

#[derive(sqlx::FromRow)]
#[allow(dead_code)] // fields read in load_model; constructed via sqlx::FromRow
struct PreferenceModelDbRow {
    kind: String,
    base_pref: f64,
    features: sqlx::types::Json<Vec<FeatureWeightJson>>,
}

#[async_trait]
impl PreferenceRepository for PostgresRepository {
    async fn record_comparison(&self, winner_id: &str, loser_id: &str) -> Result<()> {
        sqlx::query("INSERT INTO preference_comparisons (winner_id, loser_id) VALUES ($1, $2)")
            .bind(winner_id)
            .bind(loser_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn record_rating(&self, activity_id: &str, rating: i16) -> Result<()> {
        sqlx::query("INSERT INTO preference_ratings (activity_id, rating) VALUES ($1, $2)")
            .bind(activity_id)
            .bind(rating)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn count_comparisons(&self) -> Result<i64> {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM preference_comparisons")
            .fetch_one(&self.pool)
            .await?;
        Ok(n)
    }

    async fn count_ratings(&self) -> Result<i64> {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM preference_ratings")
            .fetch_one(&self.pool)
            .await?;
        Ok(n)
    }

    async fn comparison_counts(&self) -> Result<HashMap<String, i64>> {
        // Fold winner and loser columns into one appearance tally per activity.
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT id, count(*) FROM (
                 SELECT winner_id AS id FROM preference_comparisons
                 UNION ALL
                 SELECT loser_id AS id FROM preference_comparisons
             ) t GROUP BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().collect())
    }

    async fn list_comparisons(&self) -> Result<Vec<(String, String)>> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT winner_id, loser_id FROM preference_comparisons")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows)
    }

    async fn list_ratings(&self) -> Result<Vec<(String, i16)>> {
        let rows: Vec<(String, i16)> =
            sqlx::query_as("SELECT activity_id, rating FROM preference_ratings")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows)
    }

    async fn save_model(&self, models: &[KindModel]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for m in models {
            let features: Vec<FeatureWeightJson> = m
                .features
                .iter()
                .map(|f| FeatureWeightJson {
                    name: f.name.clone(),
                    weight: f.weight,
                    norm_mean: f.norm_mean,
                    norm_std: f.norm_std,
                })
                .collect();
            sqlx::query(
                "INSERT INTO preference_model (kind, base_pref, features, updated_at)
                 VALUES ($1, $2, $3, now())
                 ON CONFLICT (kind) DO UPDATE SET
                    base_pref = EXCLUDED.base_pref,
                    features = EXCLUDED.features,
                    updated_at = now()",
            )
            .bind(m.kind.as_str())
            .bind(m.base_pref)
            .bind(sqlx::types::Json(features))
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn load_model(&self) -> Result<Vec<KindModel>> {
        let rows: Vec<PreferenceModelDbRow> =
            sqlx::query_as("SELECT kind, base_pref, features FROM preference_model")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                let Some(kind) = ActivityKind::parse(&r.kind) else {
                    tracing::warn!(kind = %r.kind, "preference_model: skipping unknown kind");
                    return None;
                };
                let features = r
                    .features
                    .0
                    .into_iter()
                    .map(|f| FeatureWeight {
                        name: f.name,
                        weight: f.weight,
                        norm_mean: f.norm_mean,
                        norm_std: f.norm_std,
                    })
                    .collect();
                Some(KindModel {
                    kind,
                    base_pref: r.base_pref,
                    features,
                })
            })
            .collect())
    }
}

#[async_trait]
impl EmbeddingRepository for PostgresRepository {
    async fn upsert_text_embeddings(
        &self,
        rows: &[(String, ActivityKind, Vec<f64>)],
    ) -> Result<usize> {
        let mut saved = 0usize;
        for chunk in rows.chunks(500) {
            let mut tx = self.pool.begin().await?;
            for (activity_id, kind, text_embedding) in chunk {
                // Checkpoint only the text vector; the reduce-stage columns stay
                // untouched so a re-run of the embed job never clobbers them.
                sqlx::query(
                    "INSERT INTO activity_embeddings (activity_id, kind, text_embedding)
                     VALUES ($1, $2, $3)
                     ON CONFLICT (activity_id, kind) DO UPDATE SET
                        text_embedding = EXCLUDED.text_embedding",
                )
                .bind(activity_id)
                .bind(kind.as_str())
                .bind(text_embedding)
                .execute(&mut *tx)
                .await?;
                saved += 1;
            }
            tx.commit().await?;
        }
        Ok(saved)
    }

    async fn raw_text_embeddings(&self) -> Result<Vec<(String, ActivityKind, Vec<f64>)>> {
        let rows: Vec<(String, String, Vec<f64>)> = sqlx::query_as(
            "SELECT activity_id, kind, text_embedding FROM activity_embeddings
             WHERE text_embedding IS NOT NULL",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, kind, text)| match ActivityKind::parse(&kind) {
                Some(k) => Some((id, k, text)),
                None => {
                    tracing::warn!(kind = %kind, "activity_embeddings: skipping unknown kind");
                    None
                }
            })
            .collect())
    }

    async fn upsert_batch(&self, rows: Vec<ActivityEmbeddingRow>) -> Result<usize> {
        let mut saved = 0usize;
        // Chunked transactions: the corpus can be tens of thousands of rows.
        for chunk in rows.chunks(500) {
            let mut tx = self.pool.begin().await?;
            for row in chunk {
                sqlx::query(
                    "INSERT INTO activity_embeddings (activity_id, kind, embedding, pca_dims, features)
                     VALUES ($1, $2, $3, $4, $5)
                     ON CONFLICT (activity_id, kind) DO UPDATE SET
                        embedding = EXCLUDED.embedding,
                        pca_dims = EXCLUDED.pca_dims,
                        features = EXCLUDED.features",
                )
                .bind(&row.activity_id)
                .bind(row.kind.as_str())
                .bind(&row.embedding)
                .bind(&row.pca_dims)
                .bind(&row.features)
                .execute(&mut *tx)
                .await?;
                saved += 1;
            }
            tx.commit().await?;
        }
        Ok(saved)
    }

    async fn count(&self) -> Result<i64> {
        let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM activity_embeddings")
            .fetch_one(&self.pool)
            .await?;
        Ok(n)
    }

    async fn find_all(&self) -> Result<Vec<ActivityEmbeddingRow>> {
        // Only fully-reduced rows: text-only checkpoints have NULL embedding/
        // pca_dims/features, which don't decode into the non-optional row struct.
        let rows: Vec<ActivityEmbeddingDbRow> = sqlx::query_as(
            "SELECT activity_id, kind, embedding, pca_dims, features FROM activity_embeddings
             WHERE features IS NOT NULL",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| {
                let kind = ActivityKind::parse(&r.kind)
                    .ok_or_else(|| anyhow::anyhow!("unknown activity kind in db: {}", r.kind))?;
                Ok(ActivityEmbeddingRow {
                    activity_id: r.activity_id,
                    kind,
                    embedding: r.embedding,
                    pca_dims: r.pca_dims,
                    features: r.features,
                })
            })
            .collect()
    }

    async fn feature_vectors(&self) -> Result<Vec<(String, ActivityKind, Vec<f64>)>> {
        // Skip the 384-dim `embedding` column — the read paths only need the
        // normalized feature vector.
        let rows: Vec<(String, String, Vec<f64>)> = sqlx::query_as(
            "SELECT activity_id, kind, features FROM activity_embeddings
             WHERE features IS NOT NULL",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, kind, features)| match ActivityKind::parse(&kind) {
                Some(k) => Some((id, k, features)),
                None => {
                    tracing::warn!(kind = %kind, "activity_embeddings: skipping unknown kind");
                    None
                }
            })
            .collect())
    }
}

/// Parse the stored `kind` TEXT into an [`ActivityKind`], erroring on unknown
/// values (the column is written from `ActivityKind::as_str`, so this is a data-
/// integrity check).
fn parse_kind(kind: &str) -> Result<ActivityKind> {
    ActivityKind::parse(kind).ok_or_else(|| anyhow::anyhow!("unknown activity kind in db: {kind}"))
}

#[async_trait]
impl ImageRepository for PostgresRepository {
    async fn upsert_links(&self, links: &[ActivityImageLink]) -> Result<usize> {
        let mut written = 0usize;
        for chunk in links.chunks(500) {
            let mut tx = self.pool.begin().await?;
            for link in chunk {
                // Preserve any existing download/embed state — only the URL is
                // refreshed, so re-syncing links never re-triggers a download.
                sqlx::query(
                    "INSERT INTO activity_images (activity_id, kind, position, source_url)
                     VALUES ($1, $2, $3, $4)
                     ON CONFLICT (activity_id, kind, position)
                        DO UPDATE SET source_url = EXCLUDED.source_url",
                )
                .bind(&link.activity_id)
                .bind(link.kind.as_str())
                .bind(link.position)
                .bind(&link.source_url)
                .execute(&mut *tx)
                .await?;
                written += 1;
            }
            tx.commit().await?;
        }
        Ok(written)
    }

    async fn pending_downloads(&self) -> Result<Vec<ActivityImageLink>> {
        let rows: Vec<(String, String, i16, String)> = sqlx::query_as(
            "SELECT activity_id, kind, position, source_url
             FROM activity_images
             WHERE content_hash IS NULL
             ORDER BY activity_id, position",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(activity_id, kind, position, source_url)| {
                Ok(ActivityImageLink {
                    activity_id,
                    kind: parse_kind(&kind)?,
                    position,
                    source_url,
                })
            })
            .collect()
    }

    async fn mark_downloaded(
        &self,
        link: &ActivityImageLink,
        content_hash: &str,
        content_type: Option<String>,
        width: Option<i32>,
        height: Option<i32>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE activity_images
             SET content_hash = $4, content_type = $5, width = $6, height = $7,
                 downloaded_at = now()
             WHERE activity_id = $1 AND kind = $2 AND position = $3",
        )
        .bind(&link.activity_id)
        .bind(link.kind.as_str())
        .bind(link.position)
        .bind(content_hash)
        .bind(content_type)
        .bind(width)
        .bind(height)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn all_downloaded(&self) -> Result<Vec<DownloadedImage>> {
        let rows: Vec<(String, String, i16, String)> = sqlx::query_as(
            "SELECT activity_id, kind, position, content_hash
             FROM activity_images
             WHERE content_hash IS NOT NULL
             ORDER BY activity_id, position",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(activity_id, kind, position, content_hash)| {
                Ok(DownloadedImage {
                    activity_id,
                    kind: parse_kind(&kind)?,
                    position,
                    content_hash,
                })
            })
            .collect()
    }

    async fn pending_image_embeddings(&self) -> Result<Vec<DownloadedImage>> {
        let rows: Vec<(String, String, i16, String)> = sqlx::query_as(
            "SELECT activity_id, kind, position, content_hash
             FROM activity_images
             WHERE content_hash IS NOT NULL AND embedding IS NULL
             ORDER BY activity_id, position",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(activity_id, kind, position, content_hash)| {
                Ok(DownloadedImage {
                    activity_id,
                    kind: parse_kind(&kind)?,
                    position,
                    content_hash,
                })
            })
            .collect()
    }

    async fn store_embeddings(&self, embeddings: &[ImageEmbedding]) -> Result<()> {
        for chunk in embeddings.chunks(500) {
            let mut tx = self.pool.begin().await?;
            for e in chunk {
                sqlx::query(
                    "UPDATE activity_images
                     SET embedding = $4, embedded_at = now()
                     WHERE activity_id = $1 AND kind = $2 AND position = $3",
                )
                .bind(&e.activity_id)
                .bind(e.kind.as_str())
                .bind(e.position)
                .bind(&e.embedding)
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await?;
        }
        Ok(())
    }

    async fn all_image_embeddings(&self) -> Result<Vec<ImageEmbedding>> {
        let rows: Vec<(String, String, i16, Vec<f64>)> = sqlx::query_as(
            "SELECT activity_id, kind, position, embedding
             FROM activity_images
             WHERE embedding IS NOT NULL
             ORDER BY activity_id, position",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|(activity_id, kind, position, embedding)| {
                Ok(ImageEmbedding {
                    activity_id,
                    kind: parse_kind(&kind)?,
                    position,
                    embedding,
                })
            })
            .collect()
    }

    async fn downloaded_hashes_by_activity(&self) -> Result<HashMap<String, Vec<String>>> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT activity_id, content_hash
             FROM activity_images
             WHERE content_hash IS NOT NULL
             ORDER BY activity_id, position",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for (activity_id, hash) in rows {
            map.entry(activity_id).or_default().push(hash);
        }
        Ok(map)
    }
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

        let all = SiteRepository::find_all(&repo).await.unwrap();
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

        let all = SiteRepository::find_all(&repo).await.unwrap();
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

        let all = SiteRepository::find_all(&repo).await.unwrap();
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

    fn tour_with_id(id: &str) -> Tour {
        Tour {
            id: id.into(),
            title: "T".into(),
            category: "Wanderung".into(),
            location: Location::new(50.0, 13.0, "L".into(), "DE".into()),
            description: "d".into(),
            duration_minutes: 60,
            length_meters: 5000,
            ascent_meters: 200,
            descent_meters: 200,
            difficulty: 1,
            stamina: 2,
            landscape: 3,
            experience: 4,
            is_loop: false,
            season_bitmask: 0,
            source_url: String::new(),
            image_urls: vec![],
            raw_json: "{}".into(),
        }
    }

    fn minimal_happening(id: &str) -> Happening {
        Happening {
            id: id.into(),
            title: "H".into(),
            location: Some(Location::new(50.0, 13.0, String::new(), String::new())),
            category_id: None,
            category_title: None,
            category_keys: vec![],
            description_short: None,
            description_long: Some("lang".into()),
            homepage: None,
            address: None,
            organizer: None,
            schedule_rules: None,
            dates: vec![],
            source_url: String::new(),
            image_urls: vec![],
            data: serde_json::Value::Null,
        }
    }

    #[tokio::test]
    async fn activity_images_lifecycle_roundtrips() {
        let repo = PostgresRepository::new(test_pool().await);

        let link = |pos: i16, url: &str| ActivityImageLink {
            activity_id: "a1".into(),
            kind: ActivityKind::Hiking,
            position: pos,
            source_url: url.into(),
        };
        let links = vec![link(0, "https://x/0.jpg"), link(1, "https://x/1.jpg")];
        assert_eq!(
            ImageRepository::upsert_links(&repo, &links).await.unwrap(),
            2
        );

        // Both start pending (no content hash yet).
        assert_eq!(repo.pending_downloads().await.unwrap().len(), 2);
        assert!(repo.all_downloaded().await.unwrap().is_empty());

        // Download position 0.
        repo.mark_downloaded(&links[0], "hash0", Some("image/jpeg".into()), Some(300), Some(300))
            .await
            .unwrap();

        assert_eq!(repo.pending_downloads().await.unwrap().len(), 1);
        let downloaded = repo.all_downloaded().await.unwrap();
        assert_eq!(downloaded.len(), 1);
        assert_eq!(downloaded[0].content_hash, "hash0");
        assert_eq!(downloaded[0].position, 0);

        // Re-upserting links must NOT clobber the download state.
        ImageRepository::upsert_links(&repo, &links).await.unwrap();
        assert_eq!(repo.pending_downloads().await.unwrap().len(), 1);

        // Store an embedding for the downloaded image, then read hashes back.
        repo.store_embeddings(&[ImageEmbedding {
            activity_id: "a1".into(),
            kind: ActivityKind::Hiking,
            position: 0,
            embedding: vec![0.1, 0.2, 0.3],
        }])
        .await
        .unwrap();

        let by_activity = repo.downloaded_hashes_by_activity().await.unwrap();
        assert_eq!(by_activity.get("a1"), Some(&vec!["hash0".to_string()]));
    }

    #[tokio::test]
    async fn tour_find_all_returns_every_tour() {
        let repo = PostgresRepository::new(test_pool().await);
        TourRepository::save_batch(&repo, vec![tour_with_id("a"), tour_with_id("b")])
            .await
            .unwrap();
        let all = TourRepository::find_all(&repo).await.unwrap();
        let mut ids: Vec<String> = all.into_iter().map(|t| t.id).collect();
        ids.sort();
        assert_eq!(ids, vec!["a".to_string(), "b".to_string()]);
    }

    #[tokio::test]
    async fn happening_find_all_returns_events_without_dates() {
        let repo = PostgresRepository::new(test_pool().await);
        HappeningRepository::save_batch(&repo, vec![minimal_happening("e1")])
            .await
            .unwrap();
        let all = HappeningRepository::find_all(&repo).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "e1");
        assert!(all[0].dates.is_empty());
    }

    fn embedding_row(id: &str, kind: ActivityKind, k: usize) -> ActivityEmbeddingRow {
        ActivityEmbeddingRow {
            activity_id: id.into(),
            kind,
            embedding: vec![0.5; 384],
            pca_dims: vec![0.1; k],
            features: vec![0.2; k + 4],
        }
    }

    #[tokio::test]
    async fn embeddings_upsert_round_trips_variable_pca_dims() {
        let repo = PostgresRepository::new(test_pool().await);
        let rows = vec![
            embedding_row("tour_1", ActivityKind::Hiking, 1),
            embedding_row("site_1", ActivityKind::Paragliding, 7),
        ];
        let saved = EmbeddingRepository::upsert_batch(&repo, rows)
            .await
            .unwrap();
        assert_eq!(saved, 2);
        assert_eq!(EmbeddingRepository::count(&repo).await.unwrap(), 2);

        let all = EmbeddingRepository::find_all(&repo).await.unwrap();
        let by_id: std::collections::HashMap<_, _> = all
            .into_iter()
            .map(|r| (r.activity_id.clone(), r))
            .collect();
        assert_eq!(by_id["tour_1"].kind, ActivityKind::Hiking);
        assert_eq!(by_id["tour_1"].pca_dims.len(), 1);
        assert_eq!(by_id["tour_1"].embedding.len(), 384);
        assert_eq!(by_id["site_1"].pca_dims.len(), 7);
    }

    #[tokio::test]
    async fn embeddings_upsert_overwrites_on_conflict() {
        let repo = PostgresRepository::new(test_pool().await);
        EmbeddingRepository::upsert_batch(&repo, vec![embedding_row("x", ActivityKind::Event, 2)])
            .await
            .unwrap();
        let mut updated = embedding_row("x", ActivityKind::Event, 3);
        updated.features[0] = 0.99;
        EmbeddingRepository::upsert_batch(&repo, vec![updated])
            .await
            .unwrap();

        assert_eq!(EmbeddingRepository::count(&repo).await.unwrap(), 1);
        let all = EmbeddingRepository::find_all(&repo).await.unwrap();
        assert_eq!(all[0].pca_dims.len(), 3);
        assert_eq!(all[0].features[0], 0.99);
    }

    #[tokio::test]
    async fn preference_feedback_records_and_tallies() {
        let repo = PostgresRepository::new(test_pool().await);

        PreferenceRepository::record_comparison(&repo, "a", "b")
            .await
            .unwrap();
        PreferenceRepository::record_comparison(&repo, "a", "c")
            .await
            .unwrap();
        PreferenceRepository::record_rating(&repo, "a", 4)
            .await
            .unwrap();

        assert_eq!(
            PreferenceRepository::count_comparisons(&repo).await.unwrap(),
            2
        );
        assert_eq!(PreferenceRepository::count_ratings(&repo).await.unwrap(), 1);

        let counts = PreferenceRepository::comparison_counts(&repo).await.unwrap();
        // "a" appears in both comparisons; "b" and "c" once each.
        assert_eq!(counts["a"], 2);
        assert_eq!(counts["b"], 1);
        assert_eq!(counts["c"], 1);
    }

    #[tokio::test]
    async fn load_model_parses_base_pref_and_feature_json() {
        let repo = PostgresRepository::new(test_pool().await);
        sqlx::query(
            "INSERT INTO preference_model (kind, base_pref, features) VALUES ($1, $2, $3)",
        )
        .bind("hiking")
        .bind(0.7_f64)
        .bind(sqlx::types::Json(serde_json::json!([
            { "name": "landscape", "weight": 0.4, "norm_mean": 3.0, "norm_std": 1.8 }
        ])))
        .execute(&repo.pool)
        .await
        .unwrap();

        let model = PreferenceRepository::load_model(&repo).await.unwrap();
        let hiking = model
            .iter()
            .find(|m| m.kind == ActivityKind::Hiking)
            .expect("hiking row");
        assert_eq!(hiking.base_pref, 0.7);
        assert_eq!(hiking.features.len(), 1);
        assert_eq!(hiking.features[0].name, "landscape");
        assert_eq!(hiking.features[0].weight, 0.4);
        assert_eq!(hiking.features[0].norm_std, 1.8);
    }
}
