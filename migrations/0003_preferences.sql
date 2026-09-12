-- Personalized preference learning (PLAN.md Phase 0b).
-- Data model only; batch pipeline and solver land in later phases.

-- One row per activity, filled across two resumable jobs. The embed job writes
-- `text_embedding` (the raw text vector) per batch as it runs, so a crash resumes
-- from whatever is already stored. The reduce job then fits per-kind PCA + z-score
-- over the whole corpus and fills `embedding` (fused text+image), `pca_dims`, and
-- `features`. `kind` is the canonical ActivityKind string; `pca_dims` is
-- variable-length (k = 1..7 per kind). All vector columns except the text
-- checkpoint are nullable while the corpus is mid-embed.
CREATE TABLE activity_embeddings (
    activity_id    TEXT NOT NULL,
    kind           TEXT NOT NULL,
    text_embedding DOUBLE PRECISION[],   -- raw text embedding; the embed job's per-batch checkpoint
    embedding      DOUBLE PRECISION[],   -- fused text+image vector; written by the reduce job
    pca_dims       DOUBLE PRECISION[],   -- per-kind PCA-reduced (k dims); written by the reduce job
    features       DOUBLE PRECISION[],   -- normalized feature vector; written by the reduce job
    PRIMARY KEY (activity_id, kind)
);

-- One row per image attached to an activity (tours/events; position 0 = primary,
-- mirroring the source gallery order). Lifecycle columns fill in stages: the link
-- is inserted first (`source_url`), `content_hash`/`content_type`/dimensions land
-- once the download job stores the bytes, and `embedding` (the raw CLIP image
-- vector) is (re)written on every embedding pass so it always matches the current
-- model. Bytes live on the filesystem, content-addressed by `content_hash`; only
-- metadata + the vector live here.
CREATE TABLE activity_images (
    activity_id   TEXT NOT NULL,
    kind          TEXT NOT NULL,
    position      SMALLINT NOT NULL DEFAULT 0,   -- 0 = primary
    source_url    TEXT NOT NULL,
    content_hash  TEXT,                          -- null until downloaded
    content_type  TEXT,
    width         INTEGER,
    height        INTEGER,
    embedding     DOUBLE PRECISION[],            -- raw CLIP image vector; refreshed each re-embed
    downloaded_at TIMESTAMPTZ,
    embedded_at   TIMESTAMPTZ,
    PRIMARY KEY (activity_id, kind, position)
);
-- Download job scans for not-yet-fetched images.
CREATE INDEX idx_activity_images_pending ON activity_images (activity_id) WHERE content_hash IS NULL;
-- Embed job scans for downloaded-but-not-yet-embedded images.
CREATE INDEX idx_activity_images_unembedded
    ON activity_images (activity_id) WHERE content_hash IS NOT NULL AND embedding IS NULL;

-- Pairwise comparisons from the Web UI. winner_id/loser_id reference activities
-- across three source tables (tours, events, sites); no single FK can enforce
-- integrity — intentionally unconstrained.
CREATE TABLE preference_comparisons (
    id          SERIAL PRIMARY KEY,
    winner_id   TEXT NOT NULL,
    loser_id    TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Post-activity 1-5 ratings. Same cross-table id caveat as comparisons.
CREATE TABLE preference_ratings (
    id          SERIAL PRIMARY KEY,
    activity_id TEXT NOT NULL,
    rating      SMALLINT NOT NULL CHECK (rating BETWEEN 1 AND 5),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One row per activity kind: learned base preference + JSONB array of feature
-- definitions bundling each learned weight with its normalizer
-- ({ "name", "weight", "norm_mean", "norm_std" }).
CREATE TABLE preference_model (
    kind       TEXT PRIMARY KEY,
    base_pref  DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    features   JSONB NOT NULL DEFAULT '[]',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
