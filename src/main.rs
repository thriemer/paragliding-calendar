use std::env;

use anyhow::Result;
use tokio::time;

use crate::app_state::AppState;

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

    let db_path = env::var("XDG_DATA_HOME")
        .ok()
        .or(env::var("CACHE_DIRECTORY").ok())
        .expect("Cache environment variable not set.");
    let db = fjall::Database::builder(&db_path).open()?;
    let state = AppState::new(&db)?;

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
