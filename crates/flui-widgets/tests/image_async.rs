//! Async dispatch tests for the `Image` widget's `AssetImage` provider
//! (`asset-images` feature): the decode-cache probe, the placeholder →
//! decoded transition, remount/rebuild identity, and in-flight coalescing.
//!
//! # Fixture isolation
//!
//! `flui_widgets::image::decode_cache`'s sync cache and pending-load map are
//! process-wide statics (mirroring Flutter's singleton `ImageCache`).
//! `nextest` runs every test in this binary as OS threads within ONE process,
//! so two tests racing on the SAME asset path would observe each other's
//! cache entries. Each test below therefore loads its own dedicated fixture
//! copy (`tiny-progress.png`, `tiny-remount.png`, …) — same 75-byte 5×3 PNG
//! bytes as `tests/fixtures/tiny.png`, but a distinct path, hence a distinct
//! `ImageCacheKey`.
#![cfg(feature = "asset-images")]

mod common;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use common::{lay_out, loose, size};
use flui_assets::AssetRegistry;
use flui_types::painting::Image as PixelImage;
use flui_widgets::{AssetImage, Image, ImageProvider, ImageProviderError};

/// Bounded budget for a real background file-read + decode to land as an
/// observed frame — generous for a 75-byte local fixture, never open-ended.
const DECODE_BUDGET: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(2);

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

fn registry() -> Arc<AssetRegistry> {
    Arc::new(AssetRegistry::default())
}

/// Pumps frames (driving the local scheduler's async step each time) until
/// `check` returns `true` or [`DECODE_BUDGET`] elapses — then panics loudly,
/// never silently passing on a stuck load. `check` runs against `laid` inside
/// the loop.
fn pump_until(laid: &mut common::LaidOut, mut check: impl FnMut(&mut common::LaidOut) -> bool) {
    let deadline = Instant::now() + DECODE_BUDGET;
    loop {
        laid.tick();
        if check(laid) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the async load did not complete within the {DECODE_BUDGET:?} budget -- \
             the background bridge task is stuck or was never scheduled",
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Item 2: an `AssetImage`-backed `Image` shows the empty-box placeholder on
/// the first frame (the eager inline poll of `resolve_async` cannot
/// synchronously complete a real background file read), then decodes to the
/// fixture's true 5×3 dimensions once the bridged load lands as a scheduled
/// rebuild.
#[test]
fn asset_image_placeholder_then_decodes_across_pumped_frames() {
    let mut laid = lay_out(
        Image::asset(registry(), fixture("tiny-progress.png")),
        loose(1000.0),
    );

    assert_eq!(
        laid.size(laid.current_root()),
        size(0.0, 0.0),
        "the first frame must show the empty-box placeholder while the \
         background load is in flight, not a guessed or default size",
    );

    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == size(5.0, 3.0)
    });
}

/// Item 3: unmounting and remounting an `Image` with the SAME cache key
/// after the decode has already completed and been cached must decode
/// IMMEDIATELY — no placeholder frame at all.
#[test]
fn asset_image_remount_hits_the_decode_cache_with_no_placeholder_frame() {
    let path = fixture("tiny-remount.png");

    // Warm the cache: mount once, wait for the real decode, then drop
    // (unmount) this tree entirely.
    {
        let mut warm_up = lay_out(Image::asset(registry(), path.clone()), loose(1000.0));
        pump_until(&mut warm_up, |laid| {
            laid.size(laid.current_root()) == size(5.0, 3.0)
        });
    }

    // Remount: a brand-new tree, same key. The decode cache is process-wide,
    // so this must be a synchronous hit on the very first frame.
    let remounted = lay_out(Image::asset(registry(), path), loose(1000.0));
    assert_eq!(
        remounted.size(remounted.root()),
        size(5.0, 3.0),
        "a remount with a warm cache entry must decode on frame one, with no \
         placeholder frame in between",
    );
}

/// A test double that counts calls to [`ImageProvider::resolve_async`] while
/// delegating everything else to a real [`AssetImage`] — proves how many
/// times `Image`'s async dispatch actually invoked the provider's factory,
/// independent of how many times the parent `Image` view itself rebuilt.
#[derive(Debug)]
struct CountingAssetImage {
    inner: AssetImage,
    resolve_async_calls: Arc<AtomicUsize>,
}

impl ImageProvider for CountingAssetImage {
    fn resolve(&self) -> Result<PixelImage, ImageProviderError> {
        self.inner.resolve()
    }

    fn resolve_async(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static>>
    {
        self.resolve_async_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.resolve_async()
    }

    fn cache_key(&self) -> Option<flui_widgets::ImageCacheKey> {
        self.inner.cache_key()
    }
}

/// Item 4: rebuilding the SAME mounted `Image` (same provider key) several
/// times while a load is in flight must not spawn additional loads —
/// `FutureBuilder`'s key-based dedup means the factory (and therefore
/// `resolve_async`) is invoked exactly once per subscription, regardless of
/// how many times the parent `Image` view is rebuilt in between.
#[test]
fn asset_image_rebuild_spawns_exactly_one_load() {
    let path = fixture("tiny-rebuild.png");
    let calls = Arc::new(AtomicUsize::new(0));

    let make_widget = || {
        Image::new(CountingAssetImage {
            inner: AssetImage::new(registry(), path.clone()),
            resolve_async_calls: Arc::clone(&calls),
        })
    };

    let mut laid = lay_out(make_widget(), loose(1000.0));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the initial mount subscribes once"
    );

    // Several rebuilds with a fresh `Image`/`CountingAssetImage` instance
    // each time, but the SAME cache key (same registry + path) -- the
    // FutureBuilder underneath must recognize the unchanged key and never
    // resubscribe.
    for _ in 0..5 {
        laid.pump_widget(make_widget());
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "5 rebuilds with an unchanged cache key must not spawn additional loads",
    );

    // Let the real load complete too, and confirm settling doesn't spawn one
    // either.
    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == size(5.0, 3.0)
    });
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "completion must not trigger a second load",
    );
}

/// Item 5: two `Image` widgets mounted together with the SAME provider key
/// both decode correctly through the shared decode cache / in-flight
/// coalescing path (`image::decode_cache::load_coalesced`).
///
/// The "exactly one underlying load" guarantee itself is proven
/// deterministically at the white-box level by
/// `image::decode_cache::tests::load_coalesced_shares_one_load_across_concurrent_callers`
/// (which has crate-internal access to count `start` invocations directly) —
/// nothing at this integration-test boundary can observe the load count
/// externally, since `decode_cache` is a private module. This test instead
/// proves the public, end-to-end consequence: both widgets converge on the
/// correct decoded image via the shared cache.
#[test]
fn two_images_same_key_both_decode_through_the_shared_cache() {
    use flui_widgets::Column;
    use flui_widgets::column;

    let path = fixture("tiny-coalesce.png");
    let reg = registry();

    let mut laid = lay_out(
        Column::new(column![
            Image::asset(Arc::clone(&reg), path.clone()),
            Image::asset(reg, path),
        ]),
        loose(1000.0),
    );

    pump_until(&mut laid, |laid| {
        let root = laid.current_root();
        laid.render_node_count() >= 2
            && laid.size(laid.child(root, 0)) == size(5.0, 3.0)
            && laid.size(laid.child(root, 1)) == size(5.0, 3.0)
    });
}
