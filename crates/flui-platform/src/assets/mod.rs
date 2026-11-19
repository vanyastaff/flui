//! An application's assets as a capability of the host (ADR-0384).
//!
//! `flui build` copies the files `[assets] directories` declares into the
//! platform's package, and the host reads them back by [`AssetKey`] through
//! its [`AssetSource`]: a directory beside the executable or in an Apple
//! bundle ([`DirectoryAssets`]), the APK's assets on Android, the page's
//! files on the Web. A widget acquires [`AssetBundleCapability`] in a
//! lifecycle hook and gets an [`AssetBundle`], FLUI's form of Flutter's
//! `rootBundle`: the composition root makes one source per UI runtime, and
//! every handle it provides shares it.
//!
//! The handle is `Send`: a read runs on a background lane, and the future it
//! returns needs no async runtime of the caller's.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Weak};

use crate::capability::Capability;

#[cfg(not(target_arch = "wasm32"))]
mod directory;
mod key;

#[cfg(not(target_arch = "wasm32"))]
pub use directory::DirectoryAssets;
pub use key::{AssetKey, AssetKeyError, MAX_ASSET_KEY_LEN};

/// What an [`AssetSource`] read resolves to: a `Send` future of `T` or an
/// [`AssetLoadError`].
pub type AssetFuture<T> = Pin<Box<dyn Future<Output = Result<T, AssetLoadError>> + Send + 'static>>;

/// Reads an application's assets: a host's package, or a test's own.
///
/// A host implements it; widgets and services reach it through
/// [`AssetBundle`].
pub trait AssetSource: Send + Sync + 'static {
    /// The bytes of the asset `key`, at most `limit` of them.
    ///
    /// The future needs no async runtime of the caller's: a source over
    /// files reads one chunk per poll on whatever lane polls it, and yields
    /// between chunks, so dropping the future stops the read at the next
    /// one. A source checks the asset's length against `limit` before it
    /// allocates and again as it reads, opens only a regular file, and
    /// answers [`AssetLoadError::NotFound`] for an asset its package does
    /// not hold.
    fn load(&self, key: &AssetKey, limit: u64) -> AssetFuture<Vec<u8>>;
}

/// The marker of the asset bundle capability (ADR-0152): its handle is
/// [`AssetBundle`].
#[derive(Debug)]
#[non_exhaustive]
pub enum AssetBundleCapability {}

impl Capability for AssetBundleCapability {
    type Handle = AssetBundle;
    const NAME: &'static str = "asset_bundle";
}

/// A widget's or a service's way to the application's assets.
///
/// Cheap to clone; clones share the source.
#[derive(Clone)]
pub struct AssetBundle {
    source: Arc<dyn AssetSource>,
}

impl AssetBundle {
    /// A handle over `source`.
    #[must_use]
    pub fn new(source: Arc<dyn AssetSource>) -> Self {
        Self { source }
    }

    /// Which source this handle reads: equal for every clone of it, and
    /// never equal to another source's, a dropped one included.
    #[must_use]
    pub fn authority(&self) -> AssetAuthority {
        AssetAuthority(Arc::downgrade(&self.source))
    }

    /// The bytes of `key`, at most `limit` of them; see
    /// [`AssetSource::load`].
    pub fn load(&self, key: &AssetKey, limit: u64) -> AssetFuture<Vec<u8>> {
        self.source.load(key, limit)
    }

    /// The text of `key`, at most `limit` bytes of UTF-8: Flutter's
    /// `loadString`.
    pub fn load_string(&self, key: &AssetKey, limit: u64) -> AssetFuture<String> {
        let bytes = self.source.load(key, limit);
        let key = key.clone();
        Box::pin(async move {
            String::from_utf8(bytes.await?).map_err(|_| AssetLoadError::NotUtf8 { key })
        })
    }
}

impl fmt::Debug for AssetBundle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AssetBundle")
            .field("authority", &self.authority())
            .finish_non_exhaustive()
    }
}

/// The identity of the source an [`AssetBundle`] reads, which a cache keys
/// its results by.
///
/// Compared by the source's allocation, which the authority keeps reserved:
/// one whose source is gone never matches a later source's, even one
/// allocated at the same address. It keeps the allocation but not the
/// source alive.
#[derive(Clone)]
pub struct AssetAuthority(Weak<dyn AssetSource>);

impl PartialEq for AssetAuthority {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::addr_eq(self.0.as_ptr(), other.0.as_ptr())
    }
}

impl Eq for AssetAuthority {}

impl std::hash::Hash for AssetAuthority {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.as_ptr().cast::<()>().hash(state);
    }
}

impl fmt::Debug for AssetAuthority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AssetAuthority")
            .field(&self.0.as_ptr().cast::<()>())
            .finish()
    }
}

/// Why an asset could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AssetLoadError {
    /// The package holds no asset under the key.
    #[error("no asset {key}")]
    NotFound {
        /// The key read.
        key: AssetKey,
    },
    /// The key names something other than a regular file: a directory, a
    /// FIFO, a device.
    #[error("the asset {key} is not a regular file")]
    NotAFile {
        /// The key read.
        key: AssetKey,
    },
    /// The asset is longer than the read's limit; its bytes past the limit
    /// were not read.
    #[error("the asset {key} is over the {limit}-byte limit")]
    TooLarge {
        /// The key read.
        key: AssetKey,
        /// The limit the read set.
        limit: u64,
    },
    /// [`AssetBundle::load_string`] read bytes that are not UTF-8.
    #[error("the asset {key} is not UTF-8")]
    NotUtf8 {
        /// The key read.
        key: AssetKey,
    },
    /// The platform's reader failed.
    #[error("reading the asset {key} failed: {source}")]
    Io {
        /// The key read.
        key: AssetKey,
        /// What the platform reported.
        #[source]
        source: std::io::Error,
    },
}

impl AssetLoadError {
    /// The key whose read failed.
    #[must_use]
    pub fn key(&self) -> &AssetKey {
        match self {
            Self::NotFound { key }
            | Self::NotAFile { key }
            | Self::TooLarge { key, .. }
            | Self::NotUtf8 { key }
            | Self::Io { key, .. } => key,
        }
    }

    /// The error a reader reports for `error` while reading `key`: a
    /// missing file or a missing directory on its path is
    /// [`NotFound`](Self::NotFound), anything else [`Io`](Self::Io).
    #[must_use]
    pub fn from_io(key: &AssetKey, error: std::io::Error) -> Self {
        let kind = error.kind();
        if kind == std::io::ErrorKind::NotFound || kind == std::io::ErrorKind::NotADirectory {
            Self::NotFound { key: key.clone() }
        } else {
            Self::Io {
                key: key.clone(),
                source: error,
            }
        }
    }
}
