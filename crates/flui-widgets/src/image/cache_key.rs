//! [`ImageCacheKey`] — the typed identity an async [`ImageProvider`](super::ImageProvider)
//! publishes for caching, in-flight coalescing, and the subscription an
//! [`Image`](super::Image) holds while it is mounted.

/// Identifies a decoded image for the sync decode cache, in-flight load
/// coalescing, and the subscription an async [`Image`](super::Image) holds
/// while it is mounted.
///
/// A bare `String` cannot serve this role: `AssetImage("x")` and
/// `NetworkImage("x")` must never alias the same cache slot even though their
/// path/URL text happens to match. A `dyn ImageProvider` trait object has no
/// type-plus-equality identity to lean on, so the provider namespace is part
/// of the key's identity explicitly.
///
/// `#[non_exhaustive]`: a future provider (e.g. a `MemoryImage`
/// with an async decode) adds a variant, not a breaking change
/// to existing match arms.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImageCacheKey {
    /// Keyed by the asset path — `AssetImage` (`asset-images` feature).
    Asset(String),
    /// Keyed by the URL — `NetworkImage` (`network-images` feature).
    Network(String),
}
