use anyhow::Result;
use tokio::time;

use crate::{app_state::AppState, config::AppConfig};

mod adapters;
mod app_state;
mod application;
mod config;
mod domain;
mod telemetry;
mod web;

#[tokio::main]
async fn main() -> Result<()> {
    telemetry::init_telemetry()?;

    tracing::info!("Starting travelai application");

    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let cfg = AppConfig::from_env()?;
    let db = fjall::Database::builder(&cfg.db_path).open()?;
    let state = AppState::new(&db, &cfg)?;

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
