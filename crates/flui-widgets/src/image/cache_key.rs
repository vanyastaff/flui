//! [`ImageCacheKey`] — the typed identity an async [`ImageProvider`](super::ImageProvider)
//! publishes for caching, in-flight coalescing, and the subscription an
//! [`Image`](super::Image) holds while it is mounted.

/// Identifies a decoded image for the sync decode cache, in-flight load
/// coalescing, and the subscription an async [`Image`](super::Image) holds
/// while it is mounted.
///
/// A bare `String` cannot serve this role: `AssetImage("x")` and
/// `NetworkImage("x")` must never alias the same cache slot even though their
/// path/URL text happens to match. Flutter's own `ImageProvider` avoids this
/// collision via `runtimeType` plus the provider's own `==` — Rust has no
/// analogue for that on a `dyn ImageProvider` trait object, so the provider
/// namespace becomes part of the key's identity explicitly instead.
///
/// `#[non_exhaustive]`: a future provider (e.g. a `dart:ui`-style
/// `MemoryImage` with an async decode) adds a variant, not a breaking change
/// to existing match arms.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImageCacheKey {
    /// Keyed by the asset path — `AssetImage` (`asset-images` feature).
    Asset(String),
    /// Keyed by the URL — `NetworkImage` (`network-images` feature).
    Network(String),
}
