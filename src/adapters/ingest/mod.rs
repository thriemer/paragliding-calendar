pub mod dhv;
pub mod kml;
pub mod outdooractive_event;
pub mod outdooractive_feed;
pub mod outdooractive_tour;

/// Canonical outdoor-active detail page for a tour/event id. `/de/r/<id>` 301-redirects to the
/// full localized page and works for both tours and events. The external URL scheme lives here,
/// in the adapter that owns the outdoor-active wire format — not in the domain.
pub fn detail_url(id: &str) -> String {
    format!("https://www.outdooractive.com/de/r/{id}")
}
