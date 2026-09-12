#![recursion_limit = "256"]

use anyhow::Result;
use sqlx::postgres::PgPoolOptions;
use tokio::time;

use crate::{app_state::AppState, config::AppConfig};

mod adapters;
mod app_state;
mod application;
mod config;
mod domain;
mod telemetry;
#[cfg(test)]
mod test_support;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init_telemetry()?;

    tracing::info!("Starting travelai application");

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let cfg = AppConfig::from_env()?;
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&cfg.database_url)
        .await?;
    MIGRATOR.run(&pool).await?;
    let state = AppState::new(&pool, &cfg)?;
    let job_state = state.clone();

    let tour_sync_state = state.clone();
    let embed_state = state.clone();
    let image_job_state = state.clone();
    let event_sync_repo = state.event_repo.clone();
    let event_sync_feed = state.outdoor_feed.clone();
    tokio::join!(
        async { adapters::web::server::run(state).await },
        // Planner deactivated for now: the periodic calendar job (plan → calendar
        // writes, every 8h) is disabled. Re-enable by restoring `job_state` above
        // and uncommenting this arm.
        async move {
            let mut interval = time::interval(time::Duration::from_hours(8));
            loop {
                interval.tick().await;
                if let Err(e) = application::calendar_job::run(&job_state).await {
                    tracing::error!(error = ?e, "Failed to create calendar entries");
                }
            }
        },
        async move {
            match tour_sync_state.outdoor_repo.count().await {
                Ok(0) => {
                    tracing::info!("No outdoor tours found, starting initial sync");
                    if let Err(e) = application::outdoor_sync::sync_tours(
                        tour_sync_state.outdoor_feed.as_ref(),
                        tour_sync_state.outdoor_repo.as_ref(),
                    )
                    .await
                    {
                        tracing::error!(error = ?e, "outdoor tour sync failed");
                    }
                }
                Ok(n) => tracing::info!(tours = n, "outdoor tours already present, skipping sync"),
                Err(e) => tracing::error!(error = ?e, "failed to check outdoor tour count"),
            }
        },
        async move {
            // Weekly sync's first tick is a week out; seed an empty table immediately on startup
            // (mirrors the tour block above) so a fresh deploy has events before then.
            if let Ok(0) = event_sync_repo.count().await {
                tracing::info!("No outdoor events found, starting initial sync");
                if let Err(e) = application::outdoor_sync::sync_events(
                    event_sync_feed.as_ref(),
                    event_sync_repo.as_ref(),
                )
                .await
                {
                    tracing::error!(error = ?e, "initial outdoor event sync failed");
                }
            }
            let period = time::Duration::from_secs(7 * 24 * 3600);
            let mut interval = time::interval_at(time::Instant::now() + period, period);
            loop {
                interval.tick().await;
                if let Err(e) = application::outdoor_sync::sync_events(
                    event_sync_feed.as_ref(),
                    event_sync_repo.as_ref(),
                )
                .await
                {
                    tracing::error!(error = ?e, "outdoor event sync failed");
                } else {
                    match event_sync_repo.count().await {
                        Ok(n) => tracing::info!(events = n, "outdoor events synced"),
                        Err(e) => tracing::error!(error = ?e, "failed to count events after sync"),
                    }
                }
            }
        },
        async move {
            // Load whatever preference model already exists so the planner scores
            // with it immediately on restart (no-op on a fresh DB).
            if let Err(e) = embed_state
                .preference_scorer
                .reload(
                    embed_state.preference_repo.as_ref(),
                    embed_state.embedding_repo.as_ref(),
                )
                .await
            {
                tracing::error!(error = ?e, "preference_scorer: initial reload failed");
            }

            // Embed + reduce activity features — but only after the source tables
            // have data, so we never embed an empty corpus. Both jobs are
            // resumable and idempotent: embedding checkpoints each batch, so this
            // arm picks up wherever a previous run left off after a restart
            // instead of redoing (or skipping) the whole corpus.
            let sites = embed_state.site_repo.count().await.unwrap_or(0);
            let tours = embed_state.outdoor_repo.count().await.unwrap_or(0);
            let events = embed_state.event_repo.count().await.unwrap_or(0);
            if sites == 0 || tours == 0 || events == 0 {
                tracing::info!(
                    sites,
                    tours,
                    events,
                    "activity_features: deferring embedding until all source tables have data"
                );
                return;
            }

            // Cheap checks before the expensive model load: is there any activity
            // without a text embedding, any downloaded image without one, or a
            // reduce still owed (text embedded but features not yet written)?
            let activities = match application::embed_job::collect(
                embed_state.outdoor_repo.as_ref(),
                embed_state.event_repo.as_ref(),
                embed_state.site_repo.as_ref(),
            )
            .await
            {
                Ok(a) => a.len(),
                Err(e) => {
                    tracing::error!(error = ?e, "activity_features: collect failed");
                    return;
                }
            };
            let text_done = embed_state
                .embedding_repo
                .raw_text_embeddings()
                .await
                .map(|v| v.len())
                .unwrap_or(0);
            let images_pending = embed_state
                .image_repo
                .pending_image_embeddings()
                .await
                .map(|v| v.len())
                .unwrap_or(0);
            let reduced = embed_state
                .embedding_repo
                .feature_vectors()
                .await
                .map(|v| v.len())
                .unwrap_or(0);
            let need_embed = activities > text_done || images_pending > 0;
            let need_reduce = need_embed || reduced < text_done;
            if !need_embed && !need_reduce {
                tracing::info!("activity_features: embeddings up to date, nothing to do");
                return;
            }

            // Images are fetched by the standalone image-download job, not here —
            // this pass fuses in whatever is already downloaded and is text-only
            // otherwise (a fresh deploy re-embeds once images land). The embedder
            // is constructed lazily so the one-time model download/load fails this
            // task, not app startup, and only when there is embedding to do.
            if need_embed {
                let embedder = match adapters::embedding::ort::OrtEmbedder::new(
                    &embed_state.embedding_cache_dir,
                    embed_state.embedding_batch_size,
                )
                .await
                {
                    Ok(e) => e,
                    Err(e) => {
                        tracing::error!(error = ?e, "failed to construct embedder");
                        return;
                    }
                };
                if let Err(e) = application::embed_job::run(
                    embed_state.outdoor_repo.as_ref(),
                    embed_state.event_repo.as_ref(),
                    embed_state.site_repo.as_ref(),
                    &embedder,
                    embed_state.embedding_repo.as_ref(),
                    embed_state.image_repo.as_ref(),
                    embed_state.image_store.as_ref(),
                    embed_state.embedding_batch_size,
                )
                .await
                {
                    tracing::error!(error = ?e, "activity embedding failed");
                    return;
                }
            }
            // Reduce reads the checkpointed embeddings — no embedder needed.
            if let Err(e) = application::reduce_job::run(
                embed_state.outdoor_repo.as_ref(),
                embed_state.event_repo.as_ref(),
                embed_state.site_repo.as_ref(),
                embed_state.embedding_repo.as_ref(),
                embed_state.image_repo.as_ref(),
                embed_state.preference_repo.as_ref(),
            )
            .await
            {
                tracing::error!(error = ?e, "feature reduction failed");
                return;
            }
            // Feature vectors + model scaffold changed → refresh the plan-time scorer.
            if let Err(e) = embed_state
                .preference_scorer
                .reload(
                    embed_state.preference_repo.as_ref(),
                    embed_state.embedding_repo.as_ref(),
                )
                .await
            {
                tracing::error!(error = ?e, "preference_scorer: reload after embedding failed");
            }
        },
        async move {
            // Standalone image-download job: sync image links from the source
            // tables, then fetch every image whose bytes we don't have yet
            // (`content_hash IS NULL`). Idempotent and cheap when nothing is
            // missing, so it re-runs on a fixed interval (first tick immediate) to
            // pick up images from freshly-synced tours/events. Downloading is fully
            // decoupled from embedding — the embedding pass only consumes images
            // this job has already stored.
            let period = time::Duration::from_secs(6 * 3600);
            let mut interval = time::interval(period);
            loop {
                interval.tick().await;
                match application::image_job::run(
                    image_job_state.outdoor_repo.as_ref(),
                    image_job_state.event_repo.as_ref(),
                    image_job_state.image_repo.as_ref(),
                    image_job_state.image_store.as_ref(),
                    &image_job_state.image_variant,
                    image_job_state.image_download_concurrency,
                )
                .await
                {
                    Ok(n) => tracing::info!(downloaded = n, "image_job: download run complete"),
                    Err(e) => tracing::error!(error = ?e, "image_job: download run failed"),
                }
            }
        }
    );
    Ok(())
}
