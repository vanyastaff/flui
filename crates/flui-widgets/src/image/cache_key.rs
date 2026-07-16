//! [`ImageCacheKey`] — the typed identity an async [`ImageProvider`](super::ImageProvider)
//! publishes for caching, in-flight coalescing, and the
//! [`FutureBuilder`](crate::FutureBuilder) subscription key.

/// Identifies a decoded image for the sync decode cache, in-flight load
/// coalescing, and the [`FutureBuilder`](crate::FutureBuilder) key an async
/// [`Image`](super::Image) subscribes with.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_and_network_keys_with_the_same_text_are_not_equal() {
        let asset = ImageCacheKey::Asset("shared.png".to_string());
        let network = ImageCacheKey::Network("shared.png".to_string());

        assert_ne!(
            asset, network,
            "the provider namespace must be part of the key's identity, not \
             just the path/URL text",
        );
    }

    #[test]
    fn equal_keys_hash_equal() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(ImageCacheKey::Asset("a.png".to_string()));
        assert!(set.contains(&ImageCacheKey::Asset("a.png".to_string())));
        assert!(!set.contains(&ImageCacheKey::Network("a.png".to_string())));
    }
}
