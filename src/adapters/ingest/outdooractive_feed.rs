//! Downloads the opentourism zip bundles and parses them into domain tours/events. Owns all the
//! HTTP + zip + filesystem I/O so the application layer stays pure.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::StatusCode;
use tokio::io::AsyncWriteExt;

use crate::adapters::ingest::{outdooractive_event, outdooractive_tour};
use crate::domain::{tour::Tour, happening::Happening, ports::CatalogFeed};

const TOUR_ZIP_URL: &str = "https://www.opentourism.net/zip/outdooractive_opentourism_tour.zip";
const EVENT_ZIP_URL: &str = "https://www.opentourism.net/zip/outdooractive_opentourism_event.zip";
const MAX_RETRIES: u32 = 3;

pub struct OutdoorActiveFeed;

impl OutdoorActiveFeed {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CatalogFeed for OutdoorActiveFeed {
    async fn fetch_tours(&self) -> Result<Vec<Tour>> {
        let path = temp_path("travelai_outdoor_tours.zip");
        tracing::info!("Downloading outdoor tour data");
        download_to_file(TOUR_ZIP_URL, &path).await?;

        let parse_path = path.clone();
        let tours = tokio::task::spawn_blocking(move || {
            parse_zip(&parse_path, outdooractive_tour::parse_tour)
        })
        .await??;

        let _ = std::fs::remove_file(&path);
        Ok(tours)
    }

    async fn fetch_happenings(&self) -> Result<Vec<Happening>> {
        let path = temp_path("travelai_outdoor_events.zip");
        tracing::info!("Downloading outdoor event data");
        download_to_file(EVENT_ZIP_URL, &path).await?;

        let parse_path = path.clone();
        let events = tokio::task::spawn_blocking(move || {
            parse_zip(&parse_path, outdooractive_event::parse_event)
        })
        .await??;

        let _ = std::fs::remove_file(&path);
        Ok(events)
    }
}

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

async fn download_to_file(url: &str, path: &Path) -> Result<()> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?;

    for attempt in 0..MAX_RETRIES {
        match try_download(&client, url, path).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                if attempt + 1 < MAX_RETRIES {
                    let delay = 5u64 * 3u64.pow(attempt); // 5s, 15s, 45s
                    tracing::warn!(attempt = attempt + 1, delay_secs = delay, error = %e, "download failed, retrying");
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                } else {
                    return Err(e).context(format!("download failed after {MAX_RETRIES} attempts"));
                }
            }
        }
    }
    unreachable!()
}

async fn try_download(client: &reqwest::Client, url: &str, path: &Path) -> Result<()> {
    let existing_len = tokio::fs::metadata(path).await.map(|m| m.len()).unwrap_or(0);

    let mut request = client.get(url);
    if existing_len > 0 {
        tracing::info!(bytes = existing_len, "resuming download");
        request = request.header("Range", format!("bytes={existing_len}-"));
    }

    let response = request.send().await?;
    let status = response.status();
    if status.is_client_error() && existing_len > 0 {
        tracing::warn!(%status, "client error with partial download, removing cached file");
        let _ = tokio::fs::remove_file(path).await;
    }
    let response = response.error_for_status()?;

    // `base` is the byte count already kept on disk: the existing bytes on a real 206 resume, else 0
    // because we truncated and start over. Progress counters count up from it.
    let (mut file, base) = match response.status() {
        StatusCode::PARTIAL_CONTENT => (
            tokio::fs::OpenOptions::new().append(true).open(path).await?,
            existing_len,
        ),
        _ => {
            if existing_len > 0 {
                tracing::info!("server ignored range request, restarting download");
            }
            (tokio::fs::File::create(path).await?, 0)
        }
    };

    let mut downloaded = base;
    let total = response.content_length().map(|cl| cl + base);
    let mut response = response;

    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        if let Some(total) = total
            && downloaded % (50 * 1024 * 1024) < chunk.len() as u64
        {
            tracing::info!(mb_downloaded = downloaded / (1024 * 1024), mb_total = total / (1024 * 1024), "download progress");
        }
    }

    file.flush().await?;
    tracing::info!(bytes = downloaded, "download complete");
    Ok(())
}

/// Read every `_de.json` entry out of a downloaded zip and parse it. Blocking (zip + `std::fs`), so
/// callers run it inside `spawn_blocking` to keep it off the async runtime.
fn parse_zip<T>(path: &Path, parse: impl Fn(&str) -> Result<T>) -> Result<Vec<T>> {
    let file = std::fs::File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    let mut out = Vec::new();
    let mut errors = 0u32;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if !name.ends_with("_de.json") {
            continue;
        }
        let mut json = String::new();
        entry.read_to_string(&mut json)?;
        match parse(&json) {
            Ok(v) => out.push(v),
            Err(e) => {
                errors += 1;
                if errors <= 5 {
                    tracing::warn!(file = %name, error = %e, "failed to parse entry");
                }
            }
        }
    }
    if errors > 5 {
        tracing::warn!(total_errors = errors, "additional parse errors suppressed");
    }
    tracing::info!(parsed = out.len(), errors, "parsed entries from zip");
    Ok(out)
}
