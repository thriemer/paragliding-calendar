use anyhow::Result;

use crate::domain::ports::{EventRepository, OutdoorFeed, OutdoorTourRepository};

pub async fn sync_tours(feed: &dyn OutdoorFeed, repo: &dyn OutdoorTourRepository) -> Result<usize> {
    let tours = feed.fetch_tours().await?;
    let saved = repo.save_batch(tours).await?;
    tracing::info!(saved, "saved tours to database");
    Ok(saved)
}

pub async fn sync_events(feed: &dyn OutdoorFeed, repo: &dyn EventRepository) -> Result<usize> {
    let events = feed.fetch_events().await?;
    let saved = repo.save_batch(events).await?;
    tracing::info!(saved, "saved events to database");
    Ok(saved)
}
