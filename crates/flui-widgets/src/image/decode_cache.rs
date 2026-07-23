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
//! one load between them (test:
//! `two_images_same_key_both_decode_through_the_shared_cache` in
//! `tests/image_async.rs`).
//!
//! # Abandoned loads do not leak
//!
//! A [`Shared`] future alone is not enough: if the map held a permanent
//! strong clone, a load whose only subscriber unmounts before completion
//! (its `Image` widget removed from the tree, and the key never requested
//! again) would pin that entry — and everything its `start` closure
//! captured, e.g. an `Arc<AssetRegistry>` and its background runtime —
//! in the map forever, because the in-future cleanup that removes a
//! completed entry only runs if something polls the future to completion,
//! which nobody does for an abandoned load. Flutter's `ImageCache` guards
//! against exactly this by removing `_pendingImages[key]` when the last
//! listener detaches, not only on completion. [`CoalescedLoad`] reproduces
//! that: the map's own reference does not count as a subscriber, and the
//! LAST outstanding [`CoalescedLoad`] handle removes the entry on `Drop`,
//! whether or not the load ever finished.

use std::collections::HashMap;
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use std::task::{Context, Poll};

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

type SharedLoad =
    Shared<Pin<Box<dyn Future<Output = Result<PixelImage, ImageProviderError>> + Send>>>;

/// A `pending` map slot: the shared future plus a count of outstanding
/// [`CoalescedLoad`] handles subscribed to it. The map's own clone of
/// `future` does not itself count as a subscriber.
struct PendingSlot {
    future: SharedLoad,
    live_subscribers: Arc<AtomicUsize>,
}

struct DecodedImageCache {
    entries: Mutex<lru::LruCache<ImageCacheKey, PixelImage>>,
    pending: Mutex<HashMap<ImageCacheKey, PendingSlot>>,
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

/// A handle to an in-flight (or already-resolved) coalesced load.
///
/// Polling delegates to the underlying [`Shared`] clone. On `Drop`, if this
/// was the LAST live handle for `key` — regardless of whether the load ever
/// completed — the `pending` map entry is removed. See the module doc,
/// "Abandoned loads do not leak".
struct CoalescedLoad {
    key: ImageCacheKey,
    future: SharedLoad,
    live_subscribers: Arc<AtomicUsize>,
}

impl Future for CoalescedLoad {
    type Output = Result<PixelImage, ImageProviderError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.future).poll(cx)
    }
}

impl Drop for CoalescedLoad {
    fn drop(&mut self) {
        // Decrement and remove-if-last under the SAME lock `load_coalesced`
        // takes to increment-or-insert: without that, a decrement to zero
        // here could race a concurrent `load_coalesced` call that just
        // found (and is about to revive) this same slot, deleting an entry
        // a brand-new subscriber just claimed.
        let mut pending = CACHE.pending.lock();
        if self.live_subscribers.fetch_sub(1, Ordering::AcqRel) == 1 {
            pending.remove(&self.key);
        }
    }
}

/// Loads `key` via `start` (invoked at most once per in-flight key),
/// coalescing concurrent callers onto the same load and caching the result on
/// success.
///
/// A second caller for a key already loading receives a handle to the same
/// underlying load (a cheap [`Shared`] clone) rather than invoking `start`
/// again — Flutter's `_pendingImages` de-duplication. The decoded image is
/// written to the sync cache before the future resolves, so a [`cached`]
/// probe made immediately after any awaiter observes completion already sees
/// the hit. An abandoned load (every subscriber dropped before completion) is
/// removed from the pending map immediately — see the module doc.
pub(crate) fn load_coalesced<F>(
    key: ImageCacheKey,
    start: impl FnOnce() -> F + Send + 'static,
) -> impl Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static
where
    F: Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static,
{
    let mut pending = CACHE.pending.lock();
    if let Some(slot) = pending.get(&key) {
        slot.live_subscribers.fetch_add(1, Ordering::AcqRel);
        return CoalescedLoad {
            key,
            future: slot.future.clone(),
            live_subscribers: Arc::clone(&slot.live_subscribers),
        };
    }

    let cache_key_for_success = key.clone();
    let boxed: Pin<Box<dyn Future<Output = Result<PixelImage, ImageProviderError>> + Send>> =
        Box::pin(async move {
            let outcome = start().await;
            if let Ok(image) = &outcome {
                CACHE
                    .entries
                    .lock()
                    .put(cache_key_for_success, image.clone());
            }
            outcome
        });
    let shared = boxed.shared();
    let live_subscribers = Arc::new(AtomicUsize::new(1));
    pending.insert(
        key.clone(),
        PendingSlot {
            future: shared.clone(),
            live_subscribers: Arc::clone(&live_subscribers),
        },
    );

    CoalescedLoad {
        key,
        future: shared,
        live_subscribers,
    }
}

#[cfg(test)]
mod tests {
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
        assert!(
            !CACHE.pending.lock().contains_key(&key),
            "a completed load's sole subscriber dropping (right after `.await` \
             returns Ready) must remove the pending entry",
        );
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
        assert!(!CACHE.pending.lock().contains_key(&key));
    }

    /// Abandoning the only subscriber to a load BEFORE it completes (the
    /// `Image` widget unmounts, the key is never requested again) must
    /// remove the pending entry immediately — not leave it pinned in the map
    /// forever waiting for a completion nobody will ever observe. This is
    /// the leak Flutter's `ImageCache` avoids by removing `_pendingImages`
    /// entries when the last listener detaches, not only on completion.
    #[test]
    fn abandoning_the_only_subscriber_before_completion_removes_the_pending_entry() {
        let key = fresh_key("abandoned-before-completion");
        let (_release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();

        let mut future = Box::pin(load_coalesced(key.clone(), move || async move {
            // Never resolves within this test: `_release_tx` is held (never
            // sent to, dropped at test end), so `.await` would be Pending
            // forever if this were ever actually driven to completion -- it
            // isn't, `future` is dropped first below. `Poll::Pending` from a
            // genuine unresolved `.await`, not a blocking call, is what lets
            // the single manual poll below return without hanging the
            // (synchronous, non-tokio) test thread.
            let _ = release_rx.await;
            Ok(solid(1, 1))
        }));

        // Poll once so the load has genuinely started (proving it is really
        // in flight, not merely constructed) before abandoning it.
        let waker = std::task::Waker::noop();
        let mut cx = Context::from_waker(waker);
        assert!(matches!(future.as_mut().poll(&mut cx), Poll::Pending));

        assert!(
            CACHE.pending.lock().contains_key(&key),
            "the entry must be registered while the sole subscriber is in flight",
        );

        drop(future); // abandon: the only subscriber goes away before completion.

        assert!(
            !CACHE.pending.lock().contains_key(&key),
            "abandoning the last subscriber before completion must remove the \
             pending entry immediately -- otherwise it, and everything the \
             load captured, is pinned in the map forever",
        );
    }

    /// While at least one OTHER subscriber remains, dropping one of several
    /// concurrent subscribers must NOT remove the entry — only the LAST one
    /// leaving does.
    #[tokio::test]
    async fn dropping_one_of_two_subscribers_keeps_the_entry_alive_for_the_other() {
        let key = fresh_key("one-of-two-abandoned");
        let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        // `Image`'s `PartialEq` is `Arc::ptr_eq` (documented: dimensions plus
        // same backing buffer, not pixel-by-pixel), so the expectation must
        // be a clone of the SAME instance the closure resolves to, not a
        // separately-constructed `solid(1, 1)`.
        let resolved_image = solid(1, 1);
        let expected_image = resolved_image.clone();

        let first = Box::pin(load_coalesced(key.clone(), move || async move {
            let _ = release_rx.await;
            Ok(resolved_image)
        }));
        let second = load_coalesced(key.clone(), || async {
            unreachable!("coalesced -- the second subscriber must never invoke start")
        });

        drop(first); // one of two subscribers leaves early.

        assert!(
            CACHE.pending.lock().contains_key(&key),
            "the entry must survive while a second subscriber is still live",
        );

        release_tx
            .send(())
            .expect("the loader task is still awaiting release");
        let result = second.await;
        assert_eq!(result.unwrap(), expected_image);
        assert!(!CACHE.pending.lock().contains_key(&key));
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

    /// Flutter's `ImageCache.maximumSize` bounds the decoded-image cache the
    /// same way (`painting/image_cache.dart`); the oracle exercises pressure
    /// via `imageCache.maximumSize = 0` in "Same image provider in multiple
    /// parts of the tree, no cache room left" (`image_test.dart`, 3.44.0).
    /// That oracle test also asserts a SEPARATE `liveImageCount`/
    /// `statusForKey`/`keepAlive` tier this cache does not have -- FLUI's
    /// `DecodedImageCache` is the LRU `entries` tier alone, with no
    /// still-displayed-but-evicted "live" tracking (see `docs/ROADMAP.md`
    /// Cross.H). This test exercises the LRU half only: filling one entry
    /// past [`DEFAULT_CAPACITY`] must evict the least-recently-used entry,
    /// not silently grow the cache unbounded.
    #[tokio::test]
    async fn cache_entries_beyond_capacity_evict_the_least_recently_used() {
        let first_key = fresh_key("evict-pressure-0");
        load_coalesced(first_key.clone(), || async { Ok(solid(1, 1)) })
            .await
            .expect("the first load succeeds");

        // Fill DEFAULT_CAPACITY more distinct entries -- the LRU is now
        // asked to hold DEFAULT_CAPACITY + 1 total, one past its bound.
        for i in 1..=DEFAULT_CAPACITY {
            let key = fresh_key(&format!("evict-pressure-{i}"));
            load_coalesced(key, || async { Ok(solid(1, 1)) })
                .await
                .expect("each synthetic load succeeds");
        }

        assert_eq!(
            cached(&first_key),
            None,
            "inserting {DEFAULT_CAPACITY} more entries past the \
             {DEFAULT_CAPACITY}-entry capacity must evict the \
             least-recently-used (the very first) entry",
        );
    }

    /// Boundary sibling to the eviction test above: filling EXACTLY
    /// [`DEFAULT_CAPACITY`] distinct entries (no overflow) must retain all
    /// of them -- proves the eviction above is a genuine capacity boundary,
    /// not an off-by-one that starts evicting a step early.
    #[tokio::test]
    async fn cache_retains_exactly_capacity_entries_without_evicting() {
        let first_key = fresh_key("at-capacity-0");
        load_coalesced(first_key.clone(), || async { Ok(solid(1, 1)) })
            .await
            .expect("the first load succeeds");

        for i in 1..DEFAULT_CAPACITY {
            let key = fresh_key(&format!("at-capacity-{i}"));
            load_coalesced(key, || async { Ok(solid(1, 1)) })
                .await
                .expect("each synthetic load succeeds");
        }

        assert!(
            cached(&first_key).is_some(),
            "exactly {DEFAULT_CAPACITY} distinct entries must all remain \
             cached, with no eviction at the capacity boundary itself",
        );
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
