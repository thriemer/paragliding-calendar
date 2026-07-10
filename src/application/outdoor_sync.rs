use anyhow::Result;

use crate::domain::ports::{HappeningRepository, CatalogFeed, TourRepository};

pub async fn sync_tours(feed: &dyn CatalogFeed, repo: &dyn TourRepository) -> Result<usize> {
    let tours = feed.fetch_tours().await?;
    let saved = repo.save_batch(tours).await?;
    tracing::info!(saved, "saved tours to database");
    Ok(saved)
}

pub async fn sync_events(feed: &dyn CatalogFeed, repo: &dyn HappeningRepository) -> Result<usize> {
    let events = feed.fetch_happenings().await?;
    let saved = repo.save_batch(events).await?;
    tracing::info!(saved, "saved events to database");
    Ok(saved)
}
