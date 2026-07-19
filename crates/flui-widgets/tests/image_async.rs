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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

/// An `AssetImage`-backed `Image` shows the empty-box placeholder on the
/// first frame (the eager inline poll of `resolve_async` cannot
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

/// Unmounting and remounting an `Image` with the SAME cache key after the
/// decode has already completed and been cached must decode IMMEDIATELY —
/// no placeholder frame at all.
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

/// Rebuilding the SAME mounted `Image` (same provider key) several times
/// while a load is in flight must not spawn additional loads —
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

/// Two `Image` widgets mounted together with the SAME provider key both
/// decode correctly through the shared decode cache / in-flight coalescing
/// path (`image::decode_cache::load_coalesced`).
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

/// A test double that observes when [`ImageProvider::resolve_async`]'s
/// returned future actually settles (`Ready`, whichever way), and whether it
/// settled as an error — a signal the `Image`/`FutureBuilder` pipeline gives
/// no other externally-observable way to detect, since an unresolved
/// (`Waiting`) box and an error-resolved (`Done` + error) box render
/// identically (an empty box).
#[derive(Debug)]
struct SettleObservingProvider {
    inner: AssetImage,
    settled: Arc<AtomicBool>,
    settled_as_error: Arc<AtomicBool>,
}

impl ImageProvider for SettleObservingProvider {
    fn resolve(&self) -> Result<PixelImage, ImageProviderError> {
        self.inner.resolve()
    }

    fn resolve_async(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static>>
    {
        let inner_future = self.inner.resolve_async();
        let settled = Arc::clone(&self.settled);
        let settled_as_error = Arc::clone(&self.settled_as_error);
        Box::pin(async move {
            let result = inner_future.await;
            settled_as_error.store(result.is_err(), Ordering::SeqCst);
            settled.store(true, Ordering::SeqCst);
            result
        })
    }

    fn cache_key(&self) -> Option<flui_widgets::ImageCacheKey> {
        self.inner.cache_key()
    }
}

/// An `AssetImage` pointed at a path that will never exist must settle on
/// the empty box within a bounded number of frames — not hang forever
/// waiting on a load that never completes, and not silently keep showing the
/// `Waiting` placeholder as if nothing happened. The error arm genuinely ran
/// (observed via [`SettleObservingProvider`], not inferred from the render
/// size alone, since `Waiting` and `Done`-with-error both render as an empty
/// box).
#[test]
fn asset_image_missing_path_settles_on_the_empty_box_not_a_hang() {
    let settled = Arc::new(AtomicBool::new(false));
    let settled_as_error = Arc::new(AtomicBool::new(false));

    let provider = SettleObservingProvider {
        inner: AssetImage::new(
            registry(),
            "flui-widgets-test-image-async-this-path-never-exists.png",
        ),
        settled: Arc::clone(&settled),
        settled_as_error: Arc::clone(&settled_as_error),
    };

    let mut laid = lay_out(Image::new(provider), loose(1000.0));

    assert_eq!(
        laid.size(laid.current_root()),
        size(0.0, 0.0),
        "the first frame must show the empty-box placeholder while the \
         (doomed) load is in flight",
    );

    pump_until(&mut laid, |_laid| settled.load(Ordering::SeqCst));

    assert!(
        settled_as_error.load(Ordering::SeqCst),
        "a load against a path that never exists must settle as an error, \
         not silently succeed",
    );
    assert_eq!(
        laid.size(laid.current_root()),
        size(0.0, 0.0),
        "an error must settle on the empty box permanently, not hang and \
         not show a phantom decoded size",
    );
}

/// Flutter's oracle `Verify Image resets its RenderImage when changing
/// providers` (`image_test.dart`, 3.44.0) expects the DEFAULT
/// (`gaplessPlayback: false`) behavior to clear the previously displayed
/// image the instant the provider key changes, showing the placeholder
/// again while the new one loads.
///
/// FLUI's `Image` does NOT do this — and investigating why is a genuine,
/// previously-undocumented finding, not a simple missing-parameter gap.
/// `Image`'s async dispatch is built on the generic
/// [`FutureBuilder`](flui_widgets::FutureBuilder) primitive
/// (`crates/flui-view/src/element/future_builder.rs`), and that primitive's
/// `did_update_view` intentionally PRESERVES the old snapshot data across a
/// key change (`"preserving the old data/error"`, ported faithfully from
/// Dart's own generic `FutureBuilder.didUpdateWidget` — see
/// `future_builder_key_change_preserves_old_data_and_ignores_initial_data`
/// in that same file). That is *correct* for the generic combinator; but it
/// means `Image`, having no policy of its own layered on top, inherits
/// "always preserve the old frame" — i.e. it behaves as Flutter's
/// `gaplessPlayback: true` UNCONDITIONALLY, with no way to opt into the
/// oracle's default reset. See `docs/ROADMAP.md` Cross.H for the full
/// writeup and the `#[ignore]`d test below pinning the oracle's actual
/// expectation.
#[test]
fn async_image_provider_swap_retains_the_previous_frame_until_the_new_one_decodes() {
    let old_path = fixture("tiny-swap1-old.png");
    let new_path = fixture("tiny-swap1-new.png");
    let reg = registry();

    let mut laid = lay_out(Image::asset(Arc::clone(&reg), old_path), loose(1000.0));
    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == size(5.0, 3.0)
    });

    laid.pump_widget(Image::asset(reg, new_path));
    assert_eq!(
        laid.size(laid.current_root()),
        size(5.0, 3.0),
        "FLUI's Image inherits FutureBuilder's data-preserving update \
         semantics: swapping to a provider with a different cache key must \
         keep showing the OLD decoded frame while the new one loads, not \
         reset to the placeholder -- this is real, verified current \
         behavior (a documented divergence from Flutter's default \
         gaplessPlayback:false, not a bug in this test)",
    );

    // Watch a short window while the new path's load is genuinely in
    // flight: the frame must stay 5x3 continuously, never dipping to the
    // empty placeholder in between -- proving this is real data retention,
    // not a race that happens to land the same value on the one frame
    // already asserted above.
    for _ in 0..50 {
        laid.tick();
        assert_eq!(
            laid.size(laid.current_root()),
            size(5.0, 3.0),
            "the displayed frame must never drop to the empty placeholder \
             while the new provider's load is in flight",
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Pins Flutter's ACTUAL oracle expectation from `Verify Image resets its
/// RenderImage when changing providers` (`image_test.dart`, 3.44.0): a
/// provider-key change should clear to the placeholder immediately, per the
/// test directly above this one currently does not (and, per that test's
/// doc, cannot without `Image` growing its own reset-unless-gapless policy
/// on top of `FutureBuilder`). Un-ignore once that policy lands — see
/// `docs/ROADMAP.md` Cross.H.
#[test]
#[ignore = "Image has no reset-on-key-change policy of its own; it inherits \
            FutureBuilder's data-preserving semantics unconditionally -- see \
            docs/ROADMAP.md Cross.H"]
fn async_image_provider_swap_should_clear_to_the_placeholder_like_flutters_default() {
    let old_path = fixture("tiny-swap1-old.png");
    let new_path = fixture("tiny-swap1-new.png");
    let reg = registry();

    let mut laid = lay_out(Image::asset(Arc::clone(&reg), old_path), loose(1000.0));
    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == size(5.0, 3.0)
    });

    laid.pump_widget(Image::asset(reg, new_path));
    assert_eq!(
        laid.size(laid.current_root()),
        size(0.0, 0.0),
        "Flutter's default (gaplessPlayback: false) clears to the \
         placeholder the instant the provider key changes",
    );
}

/// Mirrors the spirit of Flutter's `Verify Image shows correct RenderImage
/// when changing to an already completed provider` (`image_test.dart`,
/// 3.44.0): when BOTH providers' decodes are already resolved and cached
/// before the swap, `build_dispatch`'s synchronous `decode_cache::cached`
/// probe hits for the new key immediately, so the swap shows the correct
/// image on the very same frame it lands -- no placeholder gap.
///
/// This pre-warms path A too (not just path B, unlike the literal oracle)
/// so BOTH sides of the swap take the "already cached" `build_dispatch`
/// branch (a bare `RawImage`, no `FutureBuilder` wrapper) from their very
/// first frame. That narrowing is deliberate, not a shortcut: starting path
/// A COLD (`FutureBuilder`-wrapped while loading, matching the literal
/// oracle) and swapping to an already-cached path B mid-flight reproducibly
/// panics in this harness with "render node should have box geometry after
/// layout" -- a real, separate reproducible failure: a `StatelessView`
/// that is itself the pipeline root, whose built child changes from a
/// wrapped-combinator type to a differently-typed bare leaf within the same
/// build pass, has its ROOT render object replaced -- the new node mounts
/// but never receives a layout pass. Whether the cause is general View
/// reconciliation or the root-swap re-root path is NOT yet isolated (the
/// reproducer only covers the root-as-swap-subtree case) -- see
/// `docs/ROADMAP.md` Cross.H. It is pinned by the `#[ignore]`d regression
/// test below and filed there rather than chased here; out of scope for a
/// test-porting pass.
#[test]
fn async_image_provider_swap_between_two_already_cached_providers_shows_immediately() {
    let path_a = fixture("tiny-swap2-a.png");
    let path_b = fixture("tiny-swap2-b.png");
    let reg = registry();

    for path in [path_a.clone(), path_b.clone()] {
        let mut warm_up = lay_out(Image::asset(Arc::clone(&reg), path), loose(1000.0));
        pump_until(&mut warm_up, |laid| {
            laid.size(laid.current_root()) == size(5.0, 3.0)
        });
    }

    let mut laid = lay_out(Image::asset(Arc::clone(&reg), path_a), loose(1000.0));
    assert_eq!(
        laid.size(laid.current_root()),
        size(5.0, 3.0),
        "a pre-cached provider must show its real dimensions on its very \
         first frame, with no placeholder frame at all",
    );

    laid.pump_widget(Image::asset(reg, path_b));
    assert_eq!(
        laid.size(laid.current_root()),
        size(5.0, 3.0),
        "swapping between two already-cached providers must show the new \
         one's real dimensions on the same frame as the swap",
    );
}

/// Regression pin for a real, reproducible failure surfaced while porting
/// the swap-to-already-cached-provider case above: when the FIRST provider
/// mounts COLD (cache miss, so `Image` builds a `FutureBuilder`-wrapped
/// `RawImage` while it loads) and is THEN swapped, after resolving, to a
/// DIFFERENT provider whose decode is already cached (so `Image` builds a
/// bare `RawImage` directly, no wrapper) -- with `Image` mounted as the
/// pipeline root -- the ROOT render object is replaced: the new node is
/// mounted (`render_node_count` stays 1, `current_root` resolves to it,
/// its generation bumped confirming a real remount) but never receives a
/// layout pass -- `LaidOut::size` panics with "render node should have box
/// geometry after layout" on the very frame of the swap, and an additional
/// `tick()` afterward does not recover it either (proven by hand; not a
/// timing fluke). This is not test misuse: the same `pump_widget`/
/// `swap_root_view` primitive lays out every swap that REUSES the root
/// render object -- only replacing it trips this.
///
/// The cause is NOT isolated between two candidates this reproducer cannot
/// separate: (a) a general `flui-view`/`flui-rendering` reconciliation gap
/// (a replaced child's fresh render object not marked needs-layout on
/// creation), or (b) a root-swap re-root gap (`swap_root_view` never
/// re-establishes the pipeline's `root_id`/root constraints when the root
/// render object's identity changes). Because this mounts `Image` as the
/// pipeline root, it exercises only (b)'s trigger; a production tree roots
/// the pipeline at a stable `RenderView`, where (b) cannot occur. Filed to
/// `docs/ROADMAP.md` Cross.H; isolate by re-running under a stable parent
/// before asserting a layer. Un-ignore once a root-render-object identity
/// change across a swap reliably lays the new node out.
#[test]
#[ignore = "reproducible failure (cause not isolated -- general \
            reconciliation vs the root-swap re-root path): replacing the \
            pipeline-root render object across a StatelessView child \
            type-change (wrapped combinator -> bare leaf) leaves the new \
            node unlaid-out -- panics rather than fails; see \
            docs/ROADMAP.md Cross.H"]
fn async_image_provider_swap_from_a_cold_stream_to_an_already_cached_provider_lays_out() {
    let path_a = fixture("tiny-swap2-a.png");
    let path_b = fixture("tiny-swap2-b.png");
    let reg = registry();

    // Pre-warm ONLY path B.
    {
        let mut warm_up = lay_out(
            Image::asset(Arc::clone(&reg), path_b.clone()),
            loose(1000.0),
        );
        pump_until(&mut warm_up, |laid| {
            laid.size(laid.current_root()) == size(5.0, 3.0)
        });
    }

    // Path A starts COLD: FutureBuilder-wrapped while it decodes.
    let mut laid = lay_out(Image::asset(Arc::clone(&reg), path_a), loose(1000.0));
    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == size(5.0, 3.0)
    });

    laid.pump_widget(Image::asset(reg, path_b));
    assert_eq!(
        laid.size(laid.current_root()),
        size(5.0, 3.0),
        "swapping from a cold-then-resolved stream to an already-cached \
         provider must still lay out the new render object on the same \
         frame, not leave it permanently without committed geometry",
    );
}

/// An async image's forced `width` reserves that width during the
/// placeholder frame too, not just once decoded -- `RawImage::
/// create_render_object` calls `render.set_width` unconditionally, even
/// when `image` is still `None`. With intrinsic size `Size::ZERO` (no image
/// yet) the aspect source is degenerate, so `RenderImage::compute_size`
/// falls back to `folded.smallest()`: the forced width axis is tight at 40,
/// the unconstrained height axis reports its minimum (0). This has no direct
/// `image_test.dart` counterpart (Flutter's placeholder-sizing story runs
/// through a different code path, `_ImageState`'s synchronous `ImageStream`
/// attach), but proves a real, previously-unexercised FLUI behavior: a
/// forced dimension is not silently dropped while a load is in flight.
#[test]
fn async_image_with_forced_width_reserves_that_width_during_the_placeholder_frame() {
    let path = fixture("tiny-forced-width.png");
    let mut laid = lay_out(Image::asset(registry(), path).width(40.0), loose(1000.0));

    assert_eq!(
        laid.size(laid.current_root()),
        size(40.0, 0.0),
        "the forced width must be honored even on the placeholder frame, \
         before any image has decoded -- a dropped forced width here would \
         silently collapse layout to 0x0 for one frame",
    );

    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == size(40.0, 24.0)
    });
}
