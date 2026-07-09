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
mod web;

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
    let event_sync_repo = state.event_repo.clone();
    tokio::join!(
        async { web::run(state).await },
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
                    if let Err(e) = application::outdoor_sync::sync_tours(tour_sync_state.outdoor_repo.as_ref()).await {
                        tracing::error!(error = ?e, "outdoor tour sync failed");
                    }
                }
                Ok(n) => tracing::info!(tours = n, "outdoor tours already present, skipping sync"),
                Err(e) => tracing::error!(error = ?e, "failed to check outdoor tour count"),
            }
        },
        async move {
            let period = time::Duration::from_secs(7 * 24 * 3600);
            let mut interval = time::interval_at(time::Instant::now() + period, period);
            loop {
                interval.tick().await;
                if let Err(e) =
                    application::outdoor_sync::sync_events(event_sync_repo.as_ref()).await
                {
                    tracing::error!(error = ?e, "outdoor event sync failed");
                } else {
                    match event_sync_repo.count().await {
                        Ok(n) => tracing::info!(events = n, "outdoor events synced"),
                        Err(e) => tracing::error!(error = ?e, "failed to count events after sync"),
                    }
                }
            }
        }
    );
    Ok(())
}
