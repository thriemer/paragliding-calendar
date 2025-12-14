use crate::paragliding::dhv::load_sites;
use crate::models::{Location, ParaglidingSite};
use haversine::{distance, Location as HaversineLocation, Units};

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
    }
}
