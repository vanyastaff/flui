//! Process-wide decoded-image cache and in-flight load coalescing.
//!
//! A small, count-bounded cache of already-decoded images plus a map of
//! in-flight loads so two widgets requesting the same image share one load.
//!
//! # Why not `flui-assets`' own cache?
//!
//! `flui_assets::AssetCache` (moka-backed) has a hardcoded 5-minute
//! time-to-live and 1-minute time-to-idle — sensible for a byte-loader cache
//! that re-fetches cheaply on expiry, wrong for a decoded-image cache a UI
//! layer wants to hold onto for as long as it is actually displayed, however
//! long that is. `flui-assets`' registry stays the byte/asset loader only;
//! this module is the count-bounded, non-expiring cache a UI layer probes
//! synchronously before deciding whether to spawn a load at all.
//!
//! # Coalescing
//!
//! [`load_coalesced`] de-duplicates concurrent callers for the same
//! [`ImageCacheKey`]: the second caller for a key already loading receives
//! the SAME shared future rather than starting a second load. This is what
//! makes two `Image` widgets mounted with the same provider key issue exactly
//! one load between them.
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
//! which nobody does for an abandoned load. [`CoalescedLoad`] guards against
//! this by removing the pending entry when the last subscriber detaches, not
//! only on completion: the map's own reference does not count as a subscriber, and the
//! LAST outstanding [`CoalescedLoad`] handle removes the entry on `Drop`,
//! whether or not the load ever finished.

use std::collections::HashMap;
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use std::task::{Context, Poll};

use flui_painting::paint::Image as PixelImage;
use futures_util::FutureExt;
use futures_util::future::{Either, Shared};
use parking_lot::Mutex;

use super::cache_key::ImageCacheKey;
use super::provider::ImageProviderError;

/// Default number of decoded images the cache retains.
///
/// A small, conservative bound (there is no eviction-pressure telemetry yet to
/// justify a larger one —
/// revisit alongside `docs/ROADMAP.md`'s deferred cache eviction API). Callers who need to bypass the cache entirely can pre-decode and use
/// [`DirectImageProvider`](super::DirectImageProvider) instead.
const DEFAULT_CAPACITY: NonZeroUsize =
    NonZeroUsize::new(100).expect("BUG: decoded image cache capacity is nonzero");

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
    fn new(capacity: NonZeroUsize) -> Self {
        Self {
            entries: Mutex::new(lru::LruCache::new(capacity)),
            pending: Mutex::new(HashMap::new()),
        }
    }

    fn cached(&self, key: &ImageCacheKey) -> Option<PixelImage> {
        self.entries.lock().get(key).cloned()
    }

    fn insert(&self, key: ImageCacheKey, image: PixelImage) {
        let retired = {
            let mut entries = self.entries.lock();
            entries.push(key, image)
        };
        // `put` destroys capacity evictions inside the operation. `push` hands
        // back ownership, so the last pixel buffer is freed outside the lock.
        drop(retired);
    }
}

static CACHE: LazyLock<DecodedImageCache> =
    LazyLock::new(|| DecodedImageCache::new(DEFAULT_CAPACITY));

/// Returns the cached decoded image for `key`, if present — the synchronous
/// probe [`Image`](crate::Image) makes before spawning an async load.
pub(crate) fn cached(key: &ImageCacheKey) -> Option<PixelImage> {
    CACHE.cached(key)
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
/// again. The decoded image is
/// written to the sync cache before the future resolves, so a [`cached`]
/// probe made immediately after any awaiter observes completion already sees
/// the hit. Admission probes completed entries again, so a completion after
/// the caller's earlier cache miss does not cause another load. Hits refresh
/// LRU recency without invoking `start`.
/// An abandoned load (every subscriber dropped before completion) is
/// removed from the pending map immediately — see the module doc.
pub(crate) fn load_coalesced<F>(
    key: ImageCacheKey,
    start: impl FnOnce() -> F + Send + 'static,
) -> impl Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static
where
    F: Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static,
{
    let mut pending = CACHE.pending.lock();
    if let Some(image) = CACHE.cached(&key) {
        // The unused start closure may own host state. Retire it only after
        // releasing the admission guard, on both cached and pending hits.
        drop(pending);
        return Either::Left(std::future::ready(Ok(image)));
    }
    if let Some(slot) = pending.get(&key) {
        slot.live_subscribers.fetch_add(1, Ordering::AcqRel);
        let load = CoalescedLoad {
            key,
            future: slot.future.clone(),
            live_subscribers: Arc::clone(&slot.live_subscribers),
        };
        drop(pending);
        return Either::Right(load);
    }

    let cache_key_for_success = key.clone();
    let boxed: Pin<Box<dyn Future<Output = Result<PixelImage, ImageProviderError>> + Send>> =
        Box::pin(async move {
            let outcome = start().await;
            if let Ok(image) = &outcome {
                CACHE.insert(cache_key_for_success, image.clone());
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

    let load = CoalescedLoad {
        key,
        future: shared,
        live_subscribers,
    };
    drop(pending);
    Either::Right(load)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A local cache is the seam for count/recency policy; global production
    /// capacity is not part of the public image-provider contract.
    #[test]
    fn decoded_cache_promotes_hits_and_preserves_displayed_pixels_after_eviction() {
        let cache = DecodedImageCache::new(NonZeroUsize::new(2).expect("nonzero test capacity"));
        let a = fresh_key("recent");
        let b = fresh_key("evicted");
        let c = fresh_key("inserted");
        cache.insert(a.clone(), solid(1, 1));
        cache.insert(b.clone(), solid(2, 1));
        let displayed = cache.cached(&b).expect("display holds decoded image");
        let displayed_pixels = displayed.data().to_vec();
        cache.cached(&a).expect("using the older image promotes it");
        cache.insert(c.clone(), solid(3, 1));
        assert!(cache.cached(&b).is_none(), "least recent image is evicted");
        assert_eq!(cache.cached(&a).expect("recent image survives").width(), 1);
        assert_eq!(cache.cached(&c).expect("new image is cached").width(), 3);
        assert_eq!((displayed.width(), displayed.height()), (2, 1));
        assert_eq!(
            displayed.data(),
            displayed_pixels,
            "eviction does not retire displayed pixels"
        );
        cache.insert(a.clone(), solid(4, 1));
        assert_eq!(cache.cached(&a).expect("replacement is cached").width(), 4);
        assert!(
            cache.cached(&c).is_some(),
            "replacement does not evict another image"
        );
    }

    /// The production cache is intentionally process-wide, while Rust's unit
    /// test harness runs this module in parallel. Give every cache test a clean
    /// transaction so capacity assertions measure their own entries rather
    /// than whichever sibling happened to populate the global LRU first.
    static TEST_CACHE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    async fn isolated_cache() -> tokio::sync::MutexGuard<'static, ()> {
        let guard = TEST_CACHE_LOCK.lock().await;
        CACHE.entries.lock().clear();
        let _prev = std::mem::take(&mut *CACHE.pending.lock());
        guard
    }

    fn solid(width: u32, height: u32) -> PixelImage {
        PixelImage::from_rgba8(
            width,
            height,
            vec![0u8; width as usize * height as usize * 4],
        )
    }

    /// A cold key (never inserted by any test) is a guaranteed miss.
    fn fresh_key(name: &str) -> ImageCacheKey {
        ImageCacheKey::Asset(format!("decode-cache-test-{name}"))
    }

    /// Abandoning the only subscriber to a load BEFORE it completes (the
    /// `Image` widget unmounts, the key is never requested again) must
    /// remove the pending entry immediately — not leave it pinned in the map
    /// forever waiting for a completion nobody will ever observe. The entry
    /// must go when the last subscriber detaches, not only on completion.
    async fn abandoning_the_only_subscriber_before_completion_removes_the_pending_entry() {
        let _cache = isolated_cache().await;
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

    /// Two concurrent callers for the same key must invoke `start` exactly
    /// once between them, and both must observe the same decoded image.
    async fn load_coalesced_shares_one_load_across_concurrent_callers() {
        let _cache = isolated_cache().await;
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

    async fn unused_load_captures_can_reenter_on_cache_and_pending_hits() {
        struct ReenterOnDrop {
            provider: super::super::AssetImage,
            retired: Arc<AtomicUsize>,
        }
        impl Drop for ReenterOnDrop {
            fn drop(&mut self) {
                use super::super::ImageProvider;
                assert!(
                    CACHE.pending.try_lock().is_some(),
                    "admission is unlocked before retirement"
                );
                assert!(
                    CACHE.entries.try_lock().is_some(),
                    "entries are unlocked before retirement"
                );
                drop(self.provider.resolve_async());
                self.retired.fetch_add(1, Ordering::SeqCst);
            }
        }

        let _cache = isolated_cache().await;
        for completed in [false, true] {
            let path = format!("decode-cache-reentry-{completed}");
            let key = ImageCacheKey::Asset(path.clone());
            let mut first = Box::pin(load_coalesced(key.clone(), || async { Ok(solid(2, 1)) }));
            if completed {
                first.as_mut().await.expect("first load completes");
            }
            let retired = Arc::new(AtomicUsize::new(0));
            let probe = ReenterOnDrop {
                provider: super::super::AssetImage::new(
                    Arc::new(flui_assets::AssetRegistry::default()),
                    path,
                ),
                retired: Arc::clone(&retired),
            };
            let second = load_coalesced(key, move || {
                drop(probe);
                async { Ok(solid(9, 1)) }
            });
            assert_eq!(
                retired.load(Ordering::SeqCst),
                1,
                "unused capture is released at admission"
            );
            let shared = second.await.expect("existing decode remains deliverable");
            assert_eq!(shared.width(), 2, "hit must not invoke the new loader");
            if !completed {
                first.await.expect("original subscriber still completes");
            }
        }
    }

    /// Coalescing contracts in one runtime: abandonment releases pending work,
    /// concurrent callers share one load, and unused captures can reenter.
    #[tokio::test]
    async fn decode_cache_coalescing_contracts() {
        abandoning_the_only_subscriber_before_completion_removes_the_pending_entry().await;
        load_coalesced_shares_one_load_across_concurrent_callers().await;
        unused_load_captures_can_reenter_on_cache_and_pending_hits().await;
    }
}
