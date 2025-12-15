use crate::paragliding::dhv::load_sites;
use crate::paragliding::site_evaluator::evaluate_site;
use crate::models::{Location, ParaglidingSite};
use crate::models::weather::WeatherForecast;
use haversine::{distance, Location as HaversineLocation, Units};
use chrono::{Utc, Duration};

mod models;
mod paragliding;
mod weather;

fn calculate_distance(from: &Location, to: &Location) -> f64 {
    let from_haversine = HaversineLocation {
        latitude: from.latitude,
        longitude: from.longitude,
    };
    let to_haversine = HaversineLocation {
        latitude: to.latitude,
        longitude: to.longitude,
    };
    distance(from_haversine, to_haversine, Units::Kilometers)
}

fn filter_forecast_for_two_days(mut forecast: WeatherForecast) -> WeatherForecast {
    let now = Utc::now();
    let tomorrow = now + Duration::days(1);
    let day_after = now + Duration::days(2);
    
    forecast.forecast.retain(|weather_data| {
        let date = weather_data.timestamp.date_naive();
        date == now.date_naive() || date == tomorrow.date_naive() || date == day_after.date_naive()
    });
    
    forecast
}

fn find_sites_within_radius(center: &Location, radius_km: f64, sites: &[ParaglidingSite]) -> Vec<(ParaglidingSite, f64)> {
    let mut results = Vec::new();
    
    for site in sites {
        // Find the closest launch to the center point
        let mut min_distance = f64::INFINITY;
        
        for launch in &site.launches {
            let distance = calculate_distance(center, &launch.location);
            if distance < min_distance {
                min_distance = distance;
            }
        }
        
        // Include site if any launch is within radius
        if min_distance <= radius_km {
            results.push((site.clone(), min_distance));
        }
    }
    
    // Sort by distance (closest first)
    results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    results
}

fn main() {
    let location = weather::geocode("Gornau/Erz").unwrap();
    let _weather = weather::get_forecast(location[0].clone()).unwrap();

    let sites = load_sites("dhvgelaende_dhvxml_de.xml");
    
    // Search for sites within 50km of the location
    let search_center = &location[0];
    let radius_km = 50.0;
    let nearby_sites = find_sites_within_radius(search_center, radius_km, &sites);
    
    println!("Found {} paragliding sites within {}km of {}:", 
             nearby_sites.len(), radius_km, search_center.name);
    
    for (site, distance) in nearby_sites.iter().take(10) {
        println!("  - {} ({:.1}km away) - {} launches", 
                 site.name, distance, site.launches.len());
        
        // Get weather forecast for the site's first launch location
        if let Some(launch) = site.launches.first() {
            match weather::get_forecast(launch.location.clone()) {
                Ok(forecast) => {
                    let filtered_forecast = filter_forecast_for_two_days(forecast);
                    let evaluation = evaluate_site(site, &filtered_forecast);
                    
                    // Display results for the first two days
                    for (i, daily_summary) in evaluation.daily_summaries.iter().take(2).enumerate() {
                        let day_name = if i == 0 { "Today" } else { "Tomorrow" };
                        println!("    {}: {}/100 - {} flyable hours", 
                                day_name, daily_summary.overall_score, daily_summary.total_flyable_hours);
                    }
                }
                Err(_) => {
                    println!("    Weather unavailable");
                }
            }
        } else {
            println!("    No launch locations available");
        }
        println!();
    }
}
