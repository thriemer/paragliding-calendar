//! Activity image domain types for the CLIP image-embedding pipeline.
//!
//! An image moves through three states, mirrored by the `activity_images` table:
//! a [`ActivityImageLink`] (URL only, from the source feed), a [`DownloadedImage`]
//! (bytes stored, addressed by `content_hash`), and — after an embedding pass —
//! an [`ImageEmbedding`] (the raw CLIP vector, refreshed on every re-embed).

use crate::domain::activities::ActivityKind;

/// One image link extracted from a source activity's gallery, before download.
/// `position` mirrors the source order (0 = primary).
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityImageLink {
    pub activity_id: String,
    pub kind: ActivityKind,
    pub position: i16,
    pub source_url: String,
}

/// A downloaded image: its content-addressed hash (locates the bytes in the
/// `ImageStore`) plus the key of the row it belongs to.
#[derive(Debug, Clone)]
pub struct DownloadedImage {
    pub activity_id: String,
    pub kind: ActivityKind,
    pub position: i16,
    pub content_hash: String,
}

/// A freshly computed CLIP vector for one stored image, ready to persist back to
/// `activity_images.embedding`.
#[derive(Debug, Clone)]
pub struct ImageEmbedding {
    pub activity_id: String,
    pub kind: ActivityKind,
    pub position: i16,
    pub embedding: Vec<f64>,
}
