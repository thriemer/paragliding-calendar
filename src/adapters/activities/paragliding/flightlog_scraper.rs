use chrono::{TimeZone, Utc};
use reqwest::Client;
use std::fs;
use std::time::Duration;

use crate::domain::paragliding::flight::Track;

const BASE_URL: &str = "https://flightlog.org/fl.html";
const START_ID: i64 = 942736;
const DELAY_MILLIS: u64 = 500;

pub async fn scrape(output_dir: &str) {
    println!("Starting flightlog.org KML scraper...");

    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(Duration::from_secs(30))
        .cookie_store(true)
        .build()
        .expect("Failed to create HTTP client");

    println!("\n[i] Visiting main page to establish session...");
    let _ = client.get("https://flightlog.org/").send();

    println!("[i] Starting scan from id {} (decrementing)\n", START_ID);

    let mut current_id = START_ID;
    if let Ok(entries) = fs::read_dir(output_dir) {
        let mut min_id = None;
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".kml") {
                    if let Ok(id) = name.trim_end_matches(".kml").parse::<i64>() {
                        min_id = Some(min_id.map_or(id, |m: i64| m.min(id)));
                    }
                }
            }
        }
        if let Some(m) = min_id {
            current_id = m - 1;
            println!(
                "[i] Resuming from id {} (found existing files)",
                current_id + 1
            );
        }
    }
    let mut valid = 0;

    let mut current_date = Utc::now();
    let stop = Utc.with_ymd_and_hms(2020, 1, 1, 1, 1, 1).unwrap();

    let mut stop_date_count = 0;

    while stop_date_count < 4 {
        let trip_id = current_id;

        let url = format!("{}?rqtid=19&trip_id={}", BASE_URL, trip_id);

        match client.get(&url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    if let Ok(text) = response.text().await {
                        if let Ok(track) = Track::from_kml(&text) {
                            if current_date < stop {
                                stop_date_count += 1;
                            } else {
                                stop_date_count = 0;
                            }
                            current_date = track.points.get(0).unwrap().time.clone();
                            let filename = format!("{}/{}.kml", output_dir, trip_id);
                            fs::write(&filename, &text).expect("Failed to write KML file");
                            println!("Saved trip_id={} -> {}", trip_id, filename);
                            valid += 1;
                            std::thread::sleep(Duration::from_millis(DELAY_MILLIS));
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[x] Error fetching trip_id={}: {}", trip_id, e);
            }
        }

        current_id -= 1;

        println!("[i] Sleeping {}s...", DELAY_MILLIS);
        std::thread::sleep(Duration::from_millis(DELAY_MILLIS));
    }

    println!(
        "\n[✓] Done! Downloaded {} KML files to {}",
        valid, output_dir
    );
}
