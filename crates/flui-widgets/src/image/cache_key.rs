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
    /// Keyed by registry ownership and URL — `NetworkImage`.
    #[cfg(feature = "network-images")]
    Network(NetworkImageKey),
}

/// Opaque identity of an HTTP image within its owning asset registry.
///
/// Obtain this key through [`super::ImageProvider::cache_key`]. Cloned providers
/// sharing a registry share identity; different registries do not, even at the
/// same URL. The key keeps a weak allocation reference, preventing address reuse
/// while it is retained without retaining the registry or its background runtime.
#[cfg(feature = "network-images")]
#[derive(Debug, Clone)]
pub struct NetworkImageKey {
    registry: std::sync::Weak<flui_assets::AssetRegistry>,
    url: String,
}

#[cfg(feature = "network-images")]
impl NetworkImageKey {
    pub(super) fn new(registry: &std::sync::Arc<flui_assets::AssetRegistry>, url: String) -> Self {
        Self {
            registry: std::sync::Arc::downgrade(registry),
            url,
        }
    }
}

#[cfg(feature = "network-images")]
impl PartialEq for NetworkImageKey {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url && self.registry.ptr_eq(&other.registry)
    }
}

#[cfg(feature = "network-images")]
impl Eq for NetworkImageKey {}

#[cfg(feature = "network-images")]
impl std::hash::Hash for NetworkImageKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.registry.as_ptr(), state);
        std::hash::Hash::hash(&self.url, state);
    }
}
