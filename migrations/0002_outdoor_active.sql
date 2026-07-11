CREATE TABLE outdoor_tours (
    id               TEXT PRIMARY KEY,
    title            TEXT NOT NULL,
    category         TEXT NOT NULL,
    location         GEOMETRY(Point, 4326) NOT NULL,
    location_name    TEXT NOT NULL DEFAULT '',
    description      TEXT NOT NULL DEFAULT '',
    duration_minutes INTEGER NOT NULL DEFAULT 0,
    length_meters    INTEGER NOT NULL DEFAULT 0,
    ascent_meters    INTEGER NOT NULL DEFAULT 0,
    descent_meters   INTEGER NOT NULL DEFAULT 0,
    difficulty       SMALLINT NOT NULL DEFAULT 0,
    stamina          SMALLINT NOT NULL DEFAULT 0,
    landscape        SMALLINT NOT NULL DEFAULT 0,
    experience       SMALLINT NOT NULL DEFAULT 0,
    is_loop          BOOLEAN NOT NULL DEFAULT false,
    season_bitmask   SMALLINT NOT NULL DEFAULT 0,
    source_url       TEXT NOT NULL DEFAULT '',
    image_urls       TEXT[] NOT NULL DEFAULT '{}',
    raw_json         JSONB NOT NULL
);
CREATE INDEX idx_outdoor_tours_location ON outdoor_tours USING GIST (location);
CREATE INDEX idx_outdoor_tours_category ON outdoor_tours (category);

CREATE TABLE outdooractive_events (
    id                TEXT PRIMARY KEY,
    title             TEXT NOT NULL,
    location          GEOMETRY(Point, 4326),
    category_id       TEXT,
    category_title    TEXT,
    category_keys     TEXT[] NOT NULL DEFAULT '{}',
    description_short TEXT,
    description_long  TEXT,
    homepage          TEXT,
    address           JSONB,
    organizer         TEXT,
    schedule_rules    JSONB,
    data              JSONB NOT NULL,
    source_url        TEXT NOT NULL DEFAULT '',
    image_urls        TEXT[] NOT NULL DEFAULT '{}',
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_oe_location ON outdooractive_events USING GIST (location);
CREATE INDEX idx_oe_category_keys ON outdooractive_events USING GIN (category_keys);

CREATE TABLE outdooractive_event_dates (
    id          SERIAL PRIMARY KEY,
    event_id    TEXT NOT NULL REFERENCES outdooractive_events(id) ON DELETE CASCADE,
    time_from   TIMESTAMPTZ NOT NULL,
    time_to     TIMESTAMPTZ NOT NULL,
    date_text   TEXT
);
CREATE INDEX idx_oed_event ON outdooractive_event_dates (event_id);
CREATE INDEX idx_oed_time  ON outdooractive_event_dates (time_from, time_to);
