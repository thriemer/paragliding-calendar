pub mod events;
pub mod paragliding;
pub mod tours;

pub use events::EventActivitySource;
pub use paragliding::ParaglidingActivitySource;
pub use tours::TourActivitySource;

/// Append a source URL to a description `body`, separated by a blank line so Google Calendar
/// auto-links the bare URL. Stands alone if the body is empty.
fn with_source_url(body: &str, url: &str) -> String {
    if body.is_empty() {
        url.to_string()
    } else {
        format!("{body}\n\n{url}")
    }
}

#[cfg(test)]
mod tests {
    use super::with_source_url;

    #[test]
    fn appends_url_after_blank_line() {
        assert_eq!(
            with_source_url("A hike", "https://www.outdooractive.com/de/r/801692621"),
            "A hike\n\nhttps://www.outdooractive.com/de/r/801692621"
        );
    }

    #[test]
    fn empty_body_yields_bare_url() {
        assert_eq!(
            with_source_url("", "https://www.outdooractive.com/de/r/38026594"),
            "https://www.outdooractive.com/de/r/38026594"
        );
    }
}
