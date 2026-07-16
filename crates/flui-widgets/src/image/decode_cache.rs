//! Process-wide decoded-image cache and in-flight load coalescing.
//!
//! Mirrors Flutter's `PaintingBinding.instance.imageCache`
//! (`painting/image_cache.dart`, 3.44.0): a small, count-bounded cache of
//! already-decoded images (`_cache`) plus a `_pendingImages`-shaped map so two
//! widgets requesting the same image share one load.
//!
//! # Why not `flui-assets`' own cache?
//!
//! `flui_assets::AssetCache` (moka-backed) has a hardcoded 5-minute
//! time-to-live and 1-minute time-to-idle — sensible for a byte-loader cache
//! that re-fetches cheaply on expiry, wrong for a decoded-image cache a UI
//! layer wants to hold onto for as long as it is actually displayed, however
//! long that is. `flui-assets`' registry stays the byte/asset loader only;
//! this module is the count-bounded, non-expiring cache a UI layer probes
//! synchronously before deciding whether to spawn a load at all — Flutter's
//! `ImageCache.putIfAbsent` does exactly this synchronous check.
//!
//! # Coalescing
//!
//! [`load_coalesced`] de-duplicates concurrent callers for the same
//! [`ImageCacheKey`]: the second caller for a key already loading receives
//! the SAME shared future rather than starting a second load. This is what
//! makes two `Image` widgets mounted with the same provider key issue exactly
//! one load between them (test: `two_images_same_key_share_one_load` in
//! `tests/image.rs`).

use std::collections::HashMap;
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::LazyLock;

use flui_types::painting::Image as PixelImage;
use futures_util::FutureExt;
use futures_util::future::Shared;
use parking_lot::Mutex;

use super::cache_key::ImageCacheKey;
use super::provider::ImageProviderError;

/// Default number of decoded images the cache retains.
///
/// A small, conservative bound (Flutter's own default `maximumSize` is 1000,
/// but flui has no eviction-pressure telemetry yet to justify matching it —
/// revisit alongside `docs/ROADMAP.md`'s deferred `evict`/`clearLiveImages`
/// API). Callers who need to bypass the cache entirely can pre-decode and use
/// [`DirectImageProvider`](super::DirectImageProvider) instead.
const DEFAULT_CAPACITY: usize = 100;

type PendingLoad =
    Shared<Pin<Box<dyn Future<Output = Result<PixelImage, ImageProviderError>> + Send>>>;

struct DecodedImageCache {
    entries: Mutex<lru::LruCache<ImageCacheKey, PixelImage>>,
    pending: Mutex<HashMap<ImageCacheKey, PendingLoad>>,
}

impl DecodedImageCache {
    fn new(capacity: usize) -> Self {
        let capacity = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::MIN);
        Self {
            entries: Mutex::new(lru::LruCache::new(capacity)),
            pending: Mutex::new(HashMap::new()),
        }
    }
}

static CACHE: LazyLock<DecodedImageCache> =
    LazyLock::new(|| DecodedImageCache::new(DEFAULT_CAPACITY));

/// Returns the cached decoded image for `key`, if present — the synchronous
/// probe [`Image`](crate::Image) makes before spawning an async load
/// (`ImageCache.putIfAbsent`'s synchronous fast path).
pub(crate) fn cached(key: &ImageCacheKey) -> Option<PixelImage> {
    CACHE.entries.lock().get(key).cloned()
}

/// Loads `key` via `start` (invoked at most once per in-flight key),
/// coalescing concurrent callers onto the same load and caching the result on
/// success.
///
/// A second caller for a key already loading receives the same underlying
/// future (cloned cheaply via [`Shared`]) rather than invoking `start` again
/// — Flutter's `_pendingImages` de-duplication. The decoded image is written
/// to the sync cache before the future resolves, so a [`cached`] probe made
/// immediately after any awaiter observes completion already sees the hit.
pub(crate) fn load_coalesced<F>(
    key: ImageCacheKey,
    start: impl FnOnce() -> F + Send + 'static,
) -> impl Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static
where
    F: Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static,
{
    let mut pending = CACHE.pending.lock();
    if let Some(existing) = pending.get(&key) {
        return existing.clone();
    }

    let cache_key_for_success = key.clone();
    let pending_key_for_cleanup = key.clone();
    let boxed: Pin<Box<dyn Future<Output = Result<PixelImage, ImageProviderError>> + Send>> =
        Box::pin(async move {
            let outcome = start().await;
            if let Ok(image) = &outcome {
                CACHE
                    .entries
                    .lock()
                    .put(cache_key_for_success, image.clone());
            }
            CACHE.pending.lock().remove(&pending_key_for_cleanup);
            outcome
        });
    let shared = boxed.shared();
    pending.insert(key, shared.clone());
    shared
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn solid(width: u32, height: u32) -> PixelImage {
        PixelImage::from_rgba8(width, height, vec![0u8; (width * height * 4) as usize])
    }

    /// A cold key (never inserted by any test) is a guaranteed miss.
    fn fresh_key(name: &str) -> ImageCacheKey {
        ImageCacheKey::Asset(format!("decode-cache-test-{name}"))
    }

    #[test]
    fn cached_returns_none_for_an_unknown_key() {
        assert_eq!(cached(&fresh_key("unknown")), None);
    }

    #[tokio::test]
    async fn load_coalesced_populates_the_sync_cache_on_success() {
        let key = fresh_key("populates-cache");
        let image = solid(3, 3);
        let expected = image.clone();

        load_coalesced(key.clone(), move || async move { Ok(image) })
            .await
            .expect("the load succeeds");

        assert_eq!(cached(&key), Some(expected));
    }

    #[tokio::test]
    async fn load_coalesced_does_not_populate_the_cache_on_failure() {
        let key = fresh_key("failure-not-cached");

        let result = load_coalesced(key.clone(), || async {
            Err(ImageProviderError::DecodeFailed {
                reason: "synthetic failure".to_string(),
            })
        })
        .await;

        assert!(result.is_err());
        assert_eq!(cached(&key), None);
    }

    /// Two concurrent callers for the same key must invoke `start` exactly
    /// once between them, and both must observe the same decoded image.
    #[tokio::test]
    async fn load_coalesced_shares_one_load_across_concurrent_callers() {
        let key = fresh_key("coalesced-concurrent");
        let start_calls = Arc::new(AtomicUsize::new(0));

        let make_start = |counter: Arc<AtomicUsize>, image: PixelImage| {
            move || {
                counter.fetch_add(1, Ordering::SeqCst);
                async move { Ok(image) }
            }
        };

        let first = load_coalesced(
            key.clone(),
            make_start(Arc::clone(&start_calls), solid(2, 2)),
        );
        let second = load_coalesced(
            key.clone(),
            make_start(Arc::clone(&start_calls), solid(2, 2)),
        );

        let (first_result, second_result) = tokio::join!(first, second);

        assert_eq!(
            start_calls.load(Ordering::SeqCst),
            1,
            "two concurrent subscribers for the same key must share ONE load",
        );
        assert_eq!(first_result.unwrap(), second_result.unwrap());
    }

    /// A load for one key must never coalesce with a load for a different
    /// key, even when the key TEXT is otherwise identical across the
    /// `Asset`/`Network` namespace.
    #[tokio::test]
    async fn load_coalesced_does_not_share_loads_across_different_keys() {
        let start_calls = Arc::new(AtomicUsize::new(0));
        let make_start = |counter: Arc<AtomicUsize>, image: PixelImage| {
            move || {
                counter.fetch_add(1, Ordering::SeqCst);
                async move { Ok(image) }
            }
        };

        let asset = load_coalesced(
            ImageCacheKey::Asset("same-text.png".to_string()),
            make_start(Arc::clone(&start_calls), solid(1, 1)),
        );
        let network = load_coalesced(
            ImageCacheKey::Network("same-text.png".to_string()),
            make_start(Arc::clone(&start_calls), solid(1, 1)),
        );

        let (asset_result, network_result) = tokio::join!(asset, network);
        assert!(asset_result.is_ok());
        assert!(network_result.is_ok());

        assert_eq!(
            start_calls.load(Ordering::SeqCst),
            2,
            "Asset(\"same-text.png\") and Network(\"same-text.png\") must load \
             independently, never coalesced together",
        );
    }
}
