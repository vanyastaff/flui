//! [`ImageCacheKey`] — the typed identity an async [`ImageProvider`](super::ImageProvider)
//! publishes for the subscription an [`Image`](super::Image) holds while it is
//! mounted.

use std::any::{Any, TypeId};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Identifies the image an async provider resolves, for the subscription an
/// [`Image`](super::Image) holds while it is mounted: a rebuild whose provider
/// has an equal key resolves nothing again.
///
/// Keys are typed, never labels. `AssetImage` and `NetworkImage` key by their
/// asset registry and source, so equal paths in two registries are two images,
/// and an asset path never equals a URL with the same text. `MemoryImage` keys
/// by its bytes and `FileImage` by its path, and neither equals the other or
/// a registry source. A custom provider keys
/// by a value of its own type through [`custom`](Self::custom); values of
/// different types are never equal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageCacheKey(Repr);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Repr {
    /// `AssetImage` and `NetworkImage`: a source within one registry.
    #[cfg(feature = "asset-images")]
    Registry(RegistryImageKey),
    /// `MemoryImage`: its encoded bytes.
    Memory(MemoryKey),
    /// `FileImage`: its path.
    File(std::path::PathBuf),
    Custom(CustomKey),
}

impl ImageCacheKey {
    /// A key for a custom provider, equal to another only when that one was
    /// made from an equal value of the same type `K`.
    pub fn custom<K>(key: K) -> Self
    where
        K: Hash + Eq + fmt::Debug + Send + Sync + 'static,
    {
        Self(Repr::Custom(CustomKey(Arc::new(key))))
    }

    /// The key of encoded `bytes` whose hash is `hash`.
    pub(super) fn memory(bytes: Arc<[u8]>, hash: u64) -> Self {
        Self(Repr::Memory(MemoryKey { bytes, hash }))
    }

    /// The key of the file at `path`.
    pub(super) fn file(path: std::path::PathBuf) -> Self {
        Self(Repr::File(path))
    }

    /// The key of `source` loaded through `registry`.
    #[cfg(feature = "asset-images")]
    pub(super) fn registry(
        registry: &Arc<flui_assets::AssetRegistry>,
        source: flui_assets::ImageSource,
    ) -> Self {
        Self(Repr::Registry(RegistryImageKey {
            registry: Arc::downgrade(registry),
            source,
        }))
    }
}

/// A source within one asset registry.
///
/// The registry is compared by allocation. The key keeps a weak reference to
/// it, so the address cannot be reused by another registry while the key is
/// retained, without keeping the registry or its background runtime alive.
#[cfg(feature = "asset-images")]
#[derive(Debug, Clone)]
struct RegistryImageKey {
    registry: std::sync::Weak<flui_assets::AssetRegistry>,
    source: flui_assets::ImageSource,
}

#[cfg(feature = "asset-images")]
impl PartialEq for RegistryImageKey {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source && self.registry.ptr_eq(&other.registry)
    }
}

#[cfg(feature = "asset-images")]
impl Eq for RegistryImageKey {}

#[cfg(feature = "asset-images")]
impl Hash for RegistryImageKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.registry.as_ptr().hash(state);
        self.source.hash(state);
    }
}

/// Encoded bytes, compared by allocation first, then by hash and length,
/// then byte for byte: equal bytes in two allocations are one image.
#[derive(Clone)]
struct MemoryKey {
    bytes: Arc<[u8]>,
    hash: u64,
}

impl PartialEq for MemoryKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.bytes, &other.bytes)
            || (self.hash == other.hash
                && self.bytes.len() == other.bytes.len()
                && self.bytes == other.bytes)
    }
}

impl Eq for MemoryKey {}

impl Hash for MemoryKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

impl fmt::Debug for MemoryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryKey")
            .field("len", &self.bytes.len())
            .field("hash", &self.hash)
            .finish()
    }
}

/// A custom provider's key value, compared by type and then by value.
#[derive(Clone)]
struct CustomKey(Arc<dyn DynKey>);

trait DynKey: fmt::Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn dyn_eq(&self, other: &dyn DynKey) -> bool;
    fn dyn_hash(&self, state: &mut dyn Hasher);
}

impl<K> DynKey for K
where
    K: Hash + Eq + fmt::Debug + Send + Sync + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn dyn_eq(&self, other: &dyn DynKey) -> bool {
        other
            .as_any()
            .downcast_ref::<K>()
            .is_some_and(|other| other == self)
    }

    fn dyn_hash(&self, mut state: &mut dyn Hasher) {
        TypeId::of::<K>().hash(&mut state);
        self.hash(&mut state);
    }
}

impl PartialEq for CustomKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.dyn_eq(&*other.0)
    }
}

impl Eq for CustomKey {}

impl Hash for CustomKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.dyn_hash(state);
    }
}

impl fmt::Debug for CustomKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
