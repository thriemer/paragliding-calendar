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
        }
    );
    Ok(())
}
