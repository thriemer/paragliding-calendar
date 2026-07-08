CREATE EXTENSION IF NOT EXISTS postgis;

CREATE TABLE paragliding_sites (
    name                    TEXT PRIMARY KEY,
    country                 TEXT,
    data_source             TEXT NOT NULL,
    parking_location        GEOMETRY(Point, 4326),
    parking_name            TEXT,
    parking_country         TEXT,
    mute_alerts             BOOLEAN,
    rating                  SMALLINT,
    preferred_weather_model TEXT
);

CREATE TABLE paragliding_launches (
    id                      SERIAL PRIMARY KEY,
    site_name               TEXT NOT NULL REFERENCES paragliding_sites(name) ON DELETE CASCADE,
    site_type               TEXT NOT NULL CHECK (site_type IN ('Hang', 'Winch')),
    location                GEOMETRY(Point, 4326) NOT NULL,
    name                    TEXT NOT NULL,
    country                 TEXT NOT NULL,
    direction_degrees_start DOUBLE PRECISION NOT NULL,
    direction_degrees_stop  DOUBLE PRECISION NOT NULL,
    elevation               DOUBLE PRECISION NOT NULL
);
CREATE INDEX idx_launches_location ON paragliding_launches USING GIST (location);
CREATE INDEX idx_launches_site ON paragliding_launches (site_name);

CREATE TABLE paragliding_landings (
    id          SERIAL PRIMARY KEY,
    site_name   TEXT NOT NULL REFERENCES paragliding_sites(name) ON DELETE CASCADE,
    location    GEOMETRY(Point, 4326) NOT NULL,
    name        TEXT NOT NULL,
    country     TEXT NOT NULL,
    elevation   DOUBLE PRECISION NOT NULL
);
CREATE INDEX idx_landings_location ON paragliding_landings USING GIST (location);
CREATE INDEX idx_landings_site ON paragliding_landings (site_name);

CREATE TABLE user_settings (
    id                      INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    location_name           TEXT NOT NULL,
    location                GEOMETRY(Point, 4326) NOT NULL,
    search_radius_km        DOUBLE PRECISION NOT NULL,
    calendar_name           TEXT NOT NULL,
    minimum_flyable_hours   INTEGER NOT NULL,
    excluded_calendar_names TEXT[] NOT NULL DEFAULT '{}'
);

CREATE TABLE cache (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    expires_at BIGINT NOT NULL
);
CREATE INDEX idx_cache_expires ON cache (expires_at);
