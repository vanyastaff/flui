//! [`ImageCacheKey`] — the typed identity an async [`ImageProvider`](super::ImageProvider)
//! publishes for the subscription an [`Image`](super::Image) holds while it is
//! mounted.

use std::any::{Any, TypeId};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::{rc::Rc, sync::Arc};

/// Identifies the image an async provider resolves, for the subscription an
/// [`Image`](super::Image) holds while it is mounted: a rebuild whose provider
/// has an equal key resolves nothing again.
///
/// Keys are typed, never labels. `AssetImage` keys by its asset key and
/// `NetworkImage` by its URL, and an asset key never equals a URL with the
/// same text; the image cache and the asset bundle they load through are the
/// ones the mounted `Image` acquired, which do not change while it is
/// mounted, so the key need not name them. `MemoryImage` keys by its bytes
/// and `FileImage` by its
/// path, and neither equals the other or an asset or network source. A
/// custom provider keys by a value of its own type through
/// [`custom`](Self::custom); values of different types are never equal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageCacheKey(Repr);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Repr {
    /// `AssetImage`: its asset key as written.
    #[cfg(feature = "asset-images")]
    Asset(Arc<str>),
    /// `NetworkImage`: its URL.
    #[cfg(feature = "asset-images")]
    Network(Arc<str>),
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
        K: Hash + Eq + fmt::Debug + 'static,
    {
        Self(Repr::Custom(CustomKey(Rc::new(key))))
    }

    /// The key of encoded `bytes` whose hash is `hash`.
    pub(super) fn memory(bytes: Arc<[u8]>, hash: u64) -> Self {
        Self(Repr::Memory(MemoryKey { bytes, hash }))
    }

    /// The key of the file at `path`.
    pub(super) fn file(path: std::path::PathBuf) -> Self {
        Self(Repr::File(path))
    }

    /// The key of the asset `key` names, as written.
    #[cfg(feature = "asset-images")]
    pub(super) fn asset(key: Arc<str>) -> Self {
        Self(Repr::Asset(key))
    }

    /// The key of the image at `url`, with whatever client the
    /// [`Image`](super::Image) holding it acquired: one `Image`'s client does
    /// not change while it is mounted, so the key need not name it, while the
    /// cache keeps two clients' images apart.
    #[cfg(feature = "asset-images")]
    pub(super) fn network(url: Arc<str>) -> Self {
        Self(Repr::Network(url))
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
struct CustomKey(Rc<dyn DynKey>);

trait DynKey: fmt::Debug {
    fn as_any(&self) -> &dyn Any;
    fn dyn_eq(&self, other: &dyn DynKey) -> bool;
    fn dyn_hash(&self, state: &mut dyn Hasher);
}

impl<K> DynKey for K
where
    K: Hash + Eq + fmt::Debug + 'static,
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
