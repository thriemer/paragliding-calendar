use crate::paragliding::dhv::load_sites;

mod models;
mod paragliding;
mod weather;

fn main() {
    let location = weather::geocode("Gornau/Erz").unwrap();
    let weather = weather::get_forecast(location[0].clone()).unwrap();

    load_sites("dhvgelaende_dhvxml_de.xml");
}
