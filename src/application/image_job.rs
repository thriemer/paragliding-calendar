//! Image download job: resolve activity image links, fetch the bytes once, and
//! store them content-addressed for re-embedding and UI serving.
//!
//! Extraction (parsing the source gallery) happens in the ingest adapter; this
//! job owns the *lifecycle*: turn each activity's `image_urls` into
//! `activity_images` rows, then download the ones we don't have yet. Downloads
//! are bounded-concurrency and idempotent (a row keeps `content_hash IS NULL`
//! until its bytes land, so a failed fetch is simply retried next run). Embedding
//! is a separate pass in [`crate::application::embed_job`].

use anyhow::{Context, Result};
use futures::stream::{self, StreamExt};
use std::io::Cursor;
use std::time::Duration;

use crate::domain::{
    activities::{ActivityKind, kind_from_category},
    image::ActivityImageLink,
    ports::{HappeningRepository, ImageRepository, ImageStore, TourRepository},
};

/// Substitute the source `{variant}` size placeholder (e.g. `300x300`). URLs
/// without the placeholder are returned unchanged.
fn resolve_variant(url_template: &str, variant: &str) -> String {
    url_template.replace("{variant}", variant)
}

/// Build the ordered link rows for one activity's gallery.
fn links_for(
    activity_id: &str,
    kind: ActivityKind,
    image_urls: &[String],
    variant: &str,
) -> Vec<ActivityImageLink> {
    image_urls
        .iter()
        .enumerate()
        .map(|(pos, url)| ActivityImageLink {
            activity_id: activity_id.to_string(),
            kind,
            position: pos as i16,
            source_url: resolve_variant(url, variant),
        })
        .collect()
}

/// Sync links for every tour/event, then download any not-yet-fetched images.
/// Returns the number of images newly downloaded this run.
pub async fn run(
    tours: &dyn TourRepository,
    events: &dyn HappeningRepository,
    images: &dyn ImageRepository,
    store: &dyn ImageStore,
    variant: &str,
    concurrency: usize,
) -> Result<usize> {
    // 1. Extract links (tours whose category doesn't map to a kind are skipped,
    //    mirroring the embedding pipeline).
    let mut links: Vec<ActivityImageLink> = Vec::new();
    for tour in tours.find_all().await? {
        if let Some(kind) = kind_from_category(&tour.category) {
            links.extend(links_for(&tour.id, kind, &tour.image_urls, variant));
        }
    }
    for happening in events.find_all().await? {
        links.extend(links_for(
            &happening.id,
            ActivityKind::Event,
            &happening.image_urls,
            variant,
        ));
    }
    let linked = images.upsert_links(&links).await?;
    tracing::info!(linked, "image_job: upserted image links");

    // 2. Download whatever isn't stored yet.
    let pending = images.pending_downloads().await?;
    if pending.is_empty() {
        tracing::info!("image_job: no pending image downloads");
        return Ok(0);
    }
    tracing::info!(pending = pending.len(), concurrency, "image_job: downloading images");

    let client = reqwest::Client::builder()
        .user_agent("travelai-image-fetcher/1.0")
        .timeout(Duration::from_secs(30))
        .build()?;

    let results: Vec<usize> = stream::iter(pending.into_iter().map(|link| {
        let client = &client;
        async move {
            match download_one(client, store, images, &link).await {
                Ok(()) => 1usize,
                Err(e) => {
                    tracing::warn!(url = %link.source_url, error = ?e, "image_job: download failed");
                    0
                }
            }
        }
    }))
    .buffer_unordered(concurrency.max(1))
    .collect()
    .await;

    let downloaded: usize = results.iter().sum();
    tracing::info!(downloaded, "image_job: images downloaded");
    Ok(downloaded)
}

/// Fetch, validate (decode header for dimensions + type), store, and record one image.
async fn download_one(
    client: &reqwest::Client,
    store: &dyn ImageStore,
    images: &dyn ImageRepository,
    link: &ActivityImageLink,
) -> Result<()> {
    let bytes = client
        .get(&link.source_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?
        .to_vec();

    // Decode only the header: validates the bytes are a real image and yields
    // dimensions + format without a full decode.
    let reader = image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .context("guessing image format")?;
    let content_type = reader.format().map(|f| f.to_mime_type().to_string());
    let (width, height) = reader.into_dimensions().context("reading image dimensions")?;

    let hash = store.put(&bytes).await?;
    images
        .mark_downloaded(
            link,
            &hash,
            content_type,
            Some(width as i32),
            Some(height as i32),
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_variant_substitutes_placeholder() {
        assert_eq!(
            resolve_variant("https://x/img/{variant}/variant.jpg", "300x300"),
            "https://x/img/300x300/variant.jpg"
        );
        // No placeholder → unchanged.
        assert_eq!(resolve_variant("https://x/a.jpg", "300x300"), "https://x/a.jpg");
    }

    #[test]
    fn links_are_positioned_in_gallery_order() {
        let urls = vec![
            "https://x/{variant}/a.jpg".to_string(),
            "https://x/{variant}/b.jpg".to_string(),
        ];
        let links = links_for("t1", ActivityKind::Hiking, &urls, "300x300");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].position, 0);
        assert_eq!(links[0].source_url, "https://x/300x300/a.jpg");
        assert_eq!(links[1].position, 1);
        assert_eq!(links[1].kind, ActivityKind::Hiking);
    }
}
