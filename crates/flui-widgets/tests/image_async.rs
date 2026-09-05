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
use flui_types::Size;
use flui_types::painting::Image as PixelImage;
use flui_widgets::{AssetImage, Image, ImageProvider, ImageProviderError};
use flui_widgets::{Padding, SizedBox};

/// Bounded budget for a real background file-read + decode to land as an
/// observed frame — generous for a 75-byte local fixture, never open-ended.
const DECODE_BUDGET: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(2);

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// The two fixture shapes. Every discriminating pair in this file pairs one
/// of each, so `LaidOut::size` names WHICH image is on screen rather than only
/// that some image is. With both members the same size, a test asserting "the
/// new provider is showing" passes just as well when the old one never left,
/// or when the new one never arrived — which is exactly how a provider-swap
/// race went unnoticed until it failed on CI.
const OLD: (f32, f32) = (5.0, 3.0);
const NEW: (f32, f32) = (7.0, 2.0);

fn old_size() -> Size {
    size(OLD.0, OLD.1)
}

fn new_size() -> Size {
    size(NEW.0, NEW.1)
}

/// `inner` grown by `Padding::all(2.0)` on every side.
fn padded(inner: Size) -> Size {
    size(inner.width.0 + 4.0, inner.height.0 + 4.0)
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

/// An `AssetImage`-backed `Image` only ever shows the empty-box placeholder or
/// the fixture's true dimensions — never a guessed or default size — and
/// reaches the true dimensions once the bridged load lands.
///
/// The assertion is on that INVARIANT rather than on the first frame being a
/// placeholder. An earlier version asserted the placeholder on frame one, with
/// a doc-comment premise that "the eager inline poll of `resolve_async` cannot
/// synchronously complete a real background file read". CI disproved the
/// premise: on a warm page cache the 75-byte read does complete inline, and the
/// test failed with `5x3` against `0x0` — reporting correct-and-fast behaviour
/// as a defect.
///
/// A placeholder frame is permitted, not required. What is forbidden is any
/// third size, which is what "a guessed or default size" would be, and that is
/// still caught on every frame.
#[test]
fn asset_image_shows_only_the_placeholder_or_the_true_size() {
    let mut laid = lay_out(
        Image::asset(registry(), fixture("tiny-progress.png")),
        loose(1000.0),
    );

    let decoded = size(5.0, 3.0);
    let placeholder = size(0.0, 0.0);
    let mut seen = Vec::new();

    let deadline = Instant::now() + DECODE_BUDGET;
    loop {
        let observed = laid.size(laid.current_root());
        if seen.last() != Some(&observed) {
            seen.push(observed);
        }
        assert!(
            observed == placeholder || observed == decoded,
            "an in-flight image must show the empty-box placeholder or its \
             real dimensions and nothing else; the frame went through {seen:?}",
        );
        if observed == decoded {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the async load did not complete within the {DECODE_BUDGET:?} \
             budget; the frame went through {seen:?}",
        );
        laid.tick();
        std::thread::sleep(POLL_INTERVAL);
    }

    assert_eq!(
        seen.last(),
        Some(&decoded),
        "the load must land on the real dimensions"
    );
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

/// Rebuilding the SAME mounted `Image` several times while a load is in
/// flight must not spawn additional loads. The subscription is keyed on the
/// provider's CACHE KEY, not its instance identity, so a rebuild handing over
/// a freshly constructed provider for the same path is recognized as the same
/// subscription and `resolve_async` is never called again — Flutter's
/// `if (_imageStream?.key == newStream.key) return;`.
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
    // resolver must recognize the unchanged key and never resubscribe.
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
/// settled as an error — a signal `Image` gives no other externally
/// observable way to detect, since a still-loading box and an error-resolved
/// box render identically (an empty box).
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

/// Flutter's oracle `Verify Image doesn't reset its RenderImage when changing
/// providers if it has gaplessPlayback set` (`image_test.dart`, 3.44.0):
/// with [`Image::gapless_playback`] on, a provider-key change keeps the
/// previously decoded frame on screen until the new one is ready, rather than
/// flashing the placeholder.
#[test]
fn async_image_provider_swap_under_gapless_playback_retains_the_previous_frame() {
    let old_path = fixture("tiny-swap1-old.png");
    let new_path = fixture("tiny-swap1-new.png");
    let reg = registry();

    let mut laid = lay_out(
        Image::asset(Arc::clone(&reg), old_path).gapless_playback(true),
        loose(1000.0),
    );
    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == old_size()
    });

    laid.pump_widget(Image::asset(reg, new_path).gapless_playback(true));
    assert_eq!(
        laid.size(laid.current_root()),
        old_size(),
        "gapless playback must keep showing the OLD decoded frame while the \
         new provider's load is in flight, not reset to the placeholder",
    );

    // Gapless playback's actual promise is that there is NO placeholder frame
    // between the old image and the new one. Watch every frame until the new
    // image lands: each must be the old one or the new one, never the empty
    // placeholder.
    //
    // The previous version asserted the frame stayed 5x3 for 50 ticks and
    // called that "real data retention, not a race that happens to land the
    // same value". With both fixtures 5x3 it could not have detected either:
    // the new image lands well inside that window, and the assertion it made
    // was satisfied by the new image just as well as the old.
    let mut seen = vec![laid.size(laid.current_root())];
    let deadline = Instant::now() + DECODE_BUDGET;
    loop {
        laid.tick();
        let observed = laid.size(laid.current_root());
        if seen.last() != Some(&observed) {
            seen.push(observed);
        }
        assert_ne!(
            observed,
            size(0.0, 0.0),
            "gapless playback must never show the placeholder between the two \
             images; the frame went through {seen:?}",
        );
        if observed == new_size() {
            break;
        }
        assert_eq!(
            observed,
            old_size(),
            "before the new image lands the frame must be the OLD one; it \
             went through {seen:?}",
        );
        assert!(
            Instant::now() < deadline,
            "the new provider's load did not complete within the \
             {DECODE_BUDGET:?} budget; the frame went through {seen:?}",
        );
        std::thread::sleep(POLL_INTERVAL);
    }

    assert_eq!(
        seen,
        vec![old_size(), new_size()],
        "the frame must go straight from the old image to the new one, with \
         nothing in between",
    );
}

/// Flutter's oracle `Verify Image resets its RenderImage when changing
/// providers` (`image_test.dart`, 3.44.0): with the DEFAULT
/// (`gaplessPlayback: false`) policy, a provider-key change clears the
/// previously displayed image the instant it lands, showing the placeholder
/// again while the new one loads.
#[test]
fn async_image_provider_swap_clears_to_the_placeholder_by_default() {
    let old_path = fixture("tiny-swap-default-old.png");
    let new_path = fixture("tiny-swap-default-new.png");
    let reg = registry();

    let mut laid = lay_out(Image::asset(Arc::clone(&reg), old_path), loose(1000.0));
    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == old_size()
    });

    laid.pump_widget(Image::asset(reg, new_path));
    // Three distinct outcomes are possible here, and with both fixtures the
    // same size two of them were indistinguishable: the placeholder (correct),
    // the OLD frame retained (the bug this test exists to catch), or the NEW
    // frame already landed (a race -- correct behaviour observed too late).
    // Different sizes make the failure message say which one happened.
    assert_eq!(
        laid.size(laid.current_root()),
        size(0.0, 0.0),
        "Flutter's default (gaplessPlayback: false) clears to the \
         placeholder the instant the provider key changes; {} means the old \
         frame was retained and {} means the new load had already landed",
        old_size(),
        new_size(),
    );

    // ...and the NEW provider resolves: clearing is a transition, not a dead
    // end. Waiting for the new image's size rather than a size both fixtures
    // share is what makes this assert the new one arrived, instead of being
    // satisfied by the old one reappearing.
    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == new_size()
    });
}

/// Flutter's `Verify Image shows correct RenderImage when changing to an
/// already completed provider` (`image_test.dart`, 3.44.0): when the new
/// provider's decode is already resolved, the synchronous cache probe the
/// resolve takes hits immediately, so the swap shows the correct image on the
/// very frame it lands -- no placeholder gap, even under the default
/// (non-gapless) clear-on-swap policy, which clears and re-publishes within
/// the same `did_update_view`.
///
/// Both sides are pre-warmed here, so this is the cached-to-cached corner of
/// the swap matrix; the cold-to-cached corner is the sibling test below.
#[test]
fn async_image_provider_swap_between_two_already_cached_providers_shows_immediately() {
    let path_a = fixture("tiny-swap2-a.png");
    let path_b = fixture("tiny-swap2-b.png");
    let reg = registry();

    for (path, expected) in [(path_a.clone(), old_size()), (path_b.clone(), new_size())] {
        let mut warm_up = lay_out(Image::asset(Arc::clone(&reg), path), loose(1000.0));
        pump_until(&mut warm_up, |laid| {
            laid.size(laid.current_root()) == expected
        });
    }

    let mut laid = lay_out(Image::asset(Arc::clone(&reg), path_a), loose(1000.0));
    assert_eq!(
        laid.size(laid.current_root()),
        old_size(),
        "a pre-cached provider must show its real dimensions on its very \
         first frame, with no placeholder frame at all",
    );

    laid.pump_widget(Image::asset(reg, path_b));
    assert_eq!(
        laid.size(laid.current_root()),
        new_size(),
        "swapping between two already-cached providers must show the NEW \
         one's real dimensions on the same frame as the swap -- the two \
         fixtures differ in size precisely so that still showing the old \
         one is a failure here rather than an indistinguishable pass",
    );
}

/// The cold-to-cached corner of the swap matrix, with `Image` mounted as the
/// pipeline ROOT: the first provider mounts on a cache miss and resolves
/// through a real background load; the second is already cached, so the swap
/// republishes within `did_update_view` itself.
///
/// It began as a regression pin for a different, real failure: back when the
/// async path wrapped the leaf in a `FutureBuilder` only on a cache miss,
/// this swap changed the built child's TYPE (wrapped combinator -> bare
/// leaf), which replaced the ROOT render object -- and the replacement was
/// mounted but never laid out, so `LaidOut::size` panicked with "render node
/// should have box geometry after layout". `Image` no longer changes its
/// child type (it always builds the leaf and holds the subscription in its
/// own state), so this test no longer reaches that path; the root-render-
/// object replacement it used to exercise is covered directly, without going
/// through `Image`, by `child_type_swap.rs`.
#[test]
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
            laid.size(laid.current_root()) == new_size()
        });
    }

    // Path A starts COLD: a real background load, placeholder until it lands.
    let mut laid = lay_out(Image::asset(Arc::clone(&reg), path_a), loose(1000.0));
    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == old_size()
    });

    laid.pump_widget(Image::asset(reg, path_b));
    assert_eq!(
        laid.size(laid.current_root()),
        new_size(),
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

/// The same cold-to-cached swap one level down, under a `Padding` parent:
/// the parent sizes itself from the child it just laid out, so a child left
/// without committed geometry cannot produce the expected size.
///
/// Keeping both this and the root-level sibling is deliberate: the root-level
/// one alone cannot distinguish a real fix from the scenario quietly ceasing
/// to replace the root render object, and this nested one alone would miss a
/// re-root regression.
#[test]
fn async_image_provider_swap_lays_out_when_the_replacement_is_not_the_root() {
    let path_a = fixture("tiny-nested-a.png");
    let path_b = fixture("tiny-nested-b.png");
    let reg = registry();

    // Pre-warm ONLY path B, so swapping to it takes the synchronous
    // cache-probe path and yields a bare leaf where a wrapped combinator was.
    {
        let mut warm_up = lay_out(
            Padding::all(2.0).child(Image::asset(Arc::clone(&reg), path_b.clone())),
            loose(1000.0),
        );
        pump_until(&mut warm_up, |laid| {
            laid.size(laid.current_root()) == padded(new_size())
        });
    }

    let mut laid = lay_out(
        Padding::all(2.0).child(Image::asset(Arc::clone(&reg), path_a)),
        loose(1000.0),
    );
    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == padded(old_size())
    });

    laid.pump_widget(Padding::all(2.0).child(Image::asset(reg, path_b)));

    // The NEW image plus 2px of padding on every side. A child left without
    // committed geometry cannot produce this, because the padding parent sizes
    // itself from the child it just laid out -- and because the two fixtures
    // differ in size, neither can a child still showing the OLD image.
    assert_eq!(
        laid.size(laid.current_root()),
        padded(new_size()),
        "a replaced render object below the root must be laid out in the same \
         frame as the swap that created it",
    );
}

// ============================================================================
// THE SWAP MATRIX, DRIVEN BY HAND
// ============================================================================
//
// The tests above cover the swap corners a real `AssetImage` can reach on its
// own schedule (cached→cached, cold→cached). The ones below need the two
// loads' completion ORDER to be a test input rather than a race, so they run
// against a provider whose async resolution the test completes by hand. Such
// a provider never routes through `decode_cache::load_coalesced`, so nothing
// it resolves is ever written to the process-wide sync cache — which is
// exactly what keeps every one of its keys a permanent cache MISS, and each
// test's own two loads independent.

/// Shared state of one [`ControlledFuture`]: the result the test will hand
/// it, the waker to fire when that happens, and whether the future was
/// dropped (cancelled) before it ever settled.
#[derive(Default)]
struct Controlled {
    result: Option<Result<PixelImage, ImageProviderError>>,
    waker: Option<std::task::Waker>,
    dropped: bool,
    completed: bool,
}

/// A handle the test uses to settle one controlled load.
#[derive(Clone, Default)]
struct Completer {
    state: Arc<std::sync::Mutex<Controlled>>,
}

impl Completer {
    /// Hands `image` to the pending load and wakes its task, so the next
    /// `tick()` polls it to completion.
    fn complete(&self, image: PixelImage) {
        let waker = {
            let mut state = self.state.lock().expect("controlled state is not poisoned");
            state.result = Some(Ok(image));
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Settles the pending load as a failure.
    fn fail(&self) {
        let waker = {
            let mut state = self.state.lock().expect("controlled state is not poisoned");
            state.result = Some(Err(ImageProviderError::DecodeFailed {
                reason: "controlled failure".to_string(),
            }));
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Whether the load's future was dropped before settling — what
    /// cancelling an in-flight load looks like from outside.
    fn cancelled(&self) -> bool {
        let state = self.state.lock().expect("controlled state is not poisoned");
        state.dropped && !state.completed
    }
}

struct ControlledFuture {
    state: Arc<std::sync::Mutex<Controlled>>,
}

impl Future for ControlledFuture {
    type Output = Result<PixelImage, ImageProviderError>;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut state = self.state.lock().expect("controlled state is not poisoned");
        if let Some(result) = state.result.take() {
            state.completed = true;
            return std::task::Poll::Ready(result);
        }
        state.waker = Some(cx.waker().clone());
        std::task::Poll::Pending
    }
}

impl Drop for ControlledFuture {
    fn drop(&mut self) {
        self.state
            .lock()
            .expect("controlled state is not poisoned")
            .dropped = true;
    }
}

/// An async provider that resolves exactly when the test says so.
#[derive(Debug)]
struct ControlledProvider {
    key: flui_widgets::ImageCacheKey,
    state: Arc<std::sync::Mutex<Controlled>>,
}

impl std::fmt::Debug for Controlled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Controlled")
            .field("settled", &self.result.is_some())
            .field("waiting", &self.waker.is_some())
            .field("dropped", &self.dropped)
            .field("completed", &self.completed)
            .finish()
    }
}

impl ControlledProvider {
    /// A provider for `key`, plus the completer that settles its load.
    fn new(key: &str) -> (Self, Completer) {
        let completer = Completer::default();
        let provider = Self {
            key: flui_widgets::ImageCacheKey::Asset(format!("controlled://{key}")),
            state: Arc::clone(&completer.state),
        };
        (provider, completer)
    }
}

impl ImageProvider for ControlledProvider {
    fn resolve(&self) -> Result<PixelImage, ImageProviderError> {
        Err(ImageProviderError::RequiresAsyncResolve {
            provider_name: "ControlledProvider",
        })
    }

    fn resolve_async(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<PixelImage, ImageProviderError>> + Send + 'static>>
    {
        Box::pin(ControlledFuture {
            state: Arc::clone(&self.state),
        })
    }

    fn cache_key(&self) -> Option<flui_widgets::ImageCacheKey> {
        Some(self.key.clone())
    }
}

/// An opaque `w`x`h` image — only its dimensions are ever asserted on, since
/// layout size is the sole externally observable consequence of publishing a
/// frame.
fn opaque(width: u32, height: u32) -> PixelImage {
    PixelImage::from_rgba8(width, height, vec![255u8; (width * height * 4) as usize])
}

/// Miss-to-miss: both providers start cold, and the OLD one settles LAST.
///
/// Its result must be discarded rather than overwrite the frame the current
/// provider published — the classic swap bug: scroll fast, land on row 20,
/// and row 7's slow load paints over it. Two mechanisms stand between the two
/// outcomes: the swap cancels the superseded load outright (the sibling test
/// below observes that directly), and the generation the load was issued
/// under is retired, so a result that reaches the publish point anyway is
/// dropped there. This test asserts the end-to-end outcome; the generation
/// guard's own behaviour is pinned by the unit tests in
/// `flui_widgets::image::resolve`.
#[test]
fn a_retired_providers_late_completion_cannot_replace_the_current_image() {
    let (old_provider, old_completer) = ControlledProvider::new("out-of-order-old");
    let (new_provider, new_completer) = ControlledProvider::new("out-of-order-new");

    let mut laid = lay_out(Image::new(old_provider), loose(1000.0));
    assert_eq!(
        laid.size(laid.current_root()),
        size(0.0, 0.0),
        "a cold provider shows the placeholder while its load is in flight",
    );

    // Swap before the first load ever settles.
    laid.pump_widget(Image::new(new_provider));

    // The NEW provider settles first, at 8x4.
    new_completer.complete(opaque(8, 4));
    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == size(8.0, 4.0)
    });

    // Only now does the retired one settle, at a size that would be
    // unmistakable if it won.
    old_completer.complete(opaque(30, 20));
    for _ in 0..10 {
        laid.tick();
        assert_eq!(
            laid.size(laid.current_root()),
            size(8.0, 4.0),
            "a completion from the retired provider must never replace the \
             frame the current one published",
        );
    }
}

/// Swapping away from an in-flight load cancels it: the future is dropped,
/// not merely ignored. A load nobody is waiting for should stop costing
/// whatever it holds open.
#[test]
fn swapping_away_from_an_in_flight_load_cancels_it() {
    let (old_provider, old_completer) = ControlledProvider::new("swap-cancels-old");
    let (new_provider, _new_completer) = ControlledProvider::new("swap-cancels-new");

    let mut laid = lay_out(Image::new(old_provider), loose(1000.0));
    assert!(
        !old_completer.cancelled(),
        "the first load is live while its widget is the one mounted",
    );

    laid.pump_widget(Image::new(new_provider));

    assert!(
        old_completer.cancelled(),
        "the superseded load must be cancelled by the swap, not left running \
         to publish into a generation that has already been retired",
    );
}

/// Unmounting the widget cancels its load too — the `dispose` half of the
/// same rule, and the one that decides whether a completion can reach a state
/// that no longer exists.
#[test]
fn unmounting_the_widget_cancels_its_in_flight_load() {
    let (provider, completer) = ControlledProvider::new("unmount-cancels");

    let mut laid = lay_out(Image::new(provider), loose(1000.0));
    assert!(!completer.cancelled());

    // Replace the Image with something that is not an Image at all: the
    // element is unmounted, not updated.
    laid.pump_widget(SizedBox::square(7.0));
    assert_eq!(laid.size(laid.current_root()), size(7.0, 7.0));

    assert!(
        completer.cancelled(),
        "unmounting must cancel the load the widget owned",
    );

    // A completion arriving anyway must be a no-op, not a panic or a
    // resurrection of the unmounted subtree.
    completer.complete(opaque(30, 20));
    for _ in 0..5 {
        laid.tick();
        assert_eq!(
            laid.size(laid.current_root()),
            size(7.0, 7.0),
            "a completion for an unmounted widget must publish nothing",
        );
    }
}

/// Miss-to-miss under the default policy: the swap clears immediately, and
/// the frame that eventually lands is the NEW provider's.
#[test]
fn a_cold_to_cold_swap_shows_the_new_providers_frame_and_nothing_in_between() {
    let (old_provider, old_completer) = ControlledProvider::new("cold-to-cold-old");
    let (new_provider, new_completer) = ControlledProvider::new("cold-to-cold-new");

    let mut laid = lay_out(Image::new(old_provider), loose(1000.0));
    old_completer.complete(opaque(6, 6));
    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == size(6.0, 6.0)
    });

    laid.pump_widget(Image::new(new_provider));
    assert_eq!(
        laid.size(laid.current_root()),
        size(0.0, 0.0),
        "the default policy clears the old frame on the swap frame itself",
    );

    new_completer.complete(opaque(9, 3));
    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == size(9.0, 3.0)
    });
}

/// The same swap with [`Image::gapless_playback`] on holds the old frame
/// until the new one lands — and holds it across a load the widget cannot
/// resolve synchronously, which is the case a snapshot-preserving async
/// combinator alone cannot cover: the frame being held did not come from the
/// load that is now in flight.
#[test]
fn a_cold_to_cold_swap_under_gapless_playback_holds_the_old_frame_until_the_new_one_lands() {
    let (old_provider, old_completer) = ControlledProvider::new("gapless-cold-old");
    let (new_provider, new_completer) = ControlledProvider::new("gapless-cold-new");

    let mut laid = lay_out(
        Image::new(old_provider).gapless_playback(true),
        loose(1000.0),
    );
    old_completer.complete(opaque(6, 6));
    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == size(6.0, 6.0)
    });

    laid.pump_widget(Image::new(new_provider).gapless_playback(true));
    for _ in 0..10 {
        laid.tick();
        assert_eq!(
            laid.size(laid.current_root()),
            size(6.0, 6.0),
            "gapless playback holds the last decoded frame for as long as the \
             new load is in flight",
        );
    }

    new_completer.complete(opaque(9, 3));
    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == size(9.0, 3.0)
    });
}

/// A load that fails leaves the displayed frame exactly as the swap left it —
/// Flutter's `onError` records the exception and never calls `_replaceImage`.
/// Under the default policy the swap already cleared, so an error settles on
/// the placeholder; under gapless playback the held frame stays held.
#[test]
fn a_failed_load_does_not_disturb_the_frame_the_policy_already_chose() {
    for (gapless, expected) in [(false, size(0.0, 0.0)), (true, size(6.0, 6.0))] {
        let suffix = if gapless { "gapless" } else { "default" };
        let (old_provider, old_completer) = ControlledProvider::new(&format!("err-old-{suffix}"));
        let (new_provider, new_completer) = ControlledProvider::new(&format!("err-new-{suffix}"));

        let mut laid = lay_out(
            Image::new(old_provider).gapless_playback(gapless),
            loose(1000.0),
        );
        old_completer.complete(opaque(6, 6));
        pump_until(&mut laid, |laid| {
            laid.size(laid.current_root()) == size(6.0, 6.0)
        });

        laid.pump_widget(Image::new(new_provider).gapless_playback(gapless));
        new_completer.fail();

        for _ in 0..10 {
            laid.tick();
            assert_eq!(
                laid.size(laid.current_root()),
                expected,
                "a failed load must not change what a gapless_playback={gapless} \
                 swap already put on screen",
            );
        }
    }
}

/// Cached-to-miss, the last corner of the swap matrix: the frame on screen
/// came from the synchronous cache probe, and the provider replacing it has
/// to load.
///
/// This is the corner that decides where the last good frame is kept. Held in
/// an async combinator's snapshot it would not survive here, because the
/// frame being held was never that combinator's data — it came from the
/// cache. Held by the widget, it survives, and the default policy still
/// clears it on demand.
#[test]
fn a_cached_to_cold_swap_clears_by_default_and_holds_under_gapless_playback() {
    for (gapless, on_the_swap_frame) in [(false, size(0.0, 0.0)), (true, size(5.0, 3.0))] {
        let suffix = if gapless { "gapless" } else { "default" };
        let cached_path = fixture(&format!("tiny-cached-to-cold-{suffix}.png"));
        let reg = registry();

        // Warm the cache for the first path, then mount it: it renders from
        // the synchronous probe, with no load of its own in flight.
        {
            let mut warm_up = lay_out(
                Image::asset(Arc::clone(&reg), cached_path.clone()),
                loose(1000.0),
            );
            pump_until(&mut warm_up, |laid| {
                laid.size(laid.current_root()) == size(5.0, 3.0)
            });
        }
        let mut laid = lay_out(
            Image::asset(reg, cached_path).gapless_playback(gapless),
            loose(1000.0),
        );
        assert_eq!(
            laid.size(laid.current_root()),
            size(5.0, 3.0),
            "the warm path must render from the cache probe on frame one",
        );

        let (cold_provider, cold_completer) =
            ControlledProvider::new(&format!("cached-to-cold-{suffix}"));
        laid.pump_widget(Image::new(cold_provider).gapless_playback(gapless));
        assert_eq!(
            laid.size(laid.current_root()),
            on_the_swap_frame,
            "gapless_playback={gapless} decides what a cached frame does when \
             the provider replacing it has to load",
        );

        cold_completer.complete(opaque(9, 3));
        pump_until(&mut laid, |laid| {
            laid.size(laid.current_root()) == size(9.0, 3.0)
        });
    }
}
