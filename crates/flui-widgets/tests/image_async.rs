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
use flui_foundation::geometry::Size;
use flui_painting::paint::Image as PixelImage;
use flui_widgets::SizedBox;
use flui_widgets::{AssetImage, Image, ImageProvider, ImageProviderError};

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
const OLD: (f64, f64) = (5.0, 3.0);
const NEW: (f64, f64) = (7.0, 2.0);

fn old_size() -> Size {
    size(OLD.0, OLD.1)
}

fn new_size() -> Size {
    size(NEW.0, NEW.1)
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
    // NOT `== old_size()`. The contract is "no placeholder between the two
    // images", and a new image that is already decoded by the time the swap
    // returns satisfies it by showing the NEW frame — demanding the old one
    // here asserts that the load is still in flight, which is a property of
    // the machine, not of gapless playback. It failed on CI exactly that way
    // (left 7x2, the new image; right 5x3, the old), on a pull request that
    // changed only a Markdown file.
    //
    // The loop below is what actually pins the promise, and it covers this
    // frame too — this assertion is the first sample, held to the same rule.
    let first = laid.size(laid.current_root());
    assert_ne!(
        first,
        size(0.0, 0.0),
        "gapless playback must not flash the placeholder on the swap frame",
    );
    assert!(
        first == old_size() || first == new_size(),
        "the swap frame must be one of the two images, got {first:?}",
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
    let mut seen = vec![first];
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

    // Two shapes satisfy the promise, not one. `[old, new]` is the usual
    // run; `[new]` is the swap frame already showing the decoded new image,
    // which the first-sample check above deliberately permits — demanding
    // that the old frame was OBSERVED before the new one asserts the decode
    // was still in flight at the first sample, a property of the runner's
    // scheduling, not of gapless playback. It is the same premise the
    // first-sample check above no longer makes, left standing here at its
    // mirror site, and it failed on CI the same way: the sequence was `[7x2]`
    // alone, the new image, against an expected `[5x3, 7x2]`.
    //
    // Not redundant with the loop: the one shape the loop lets through that
    // this line rejects is `[new, old, new]` — the old frame resurfacing after
    // the new one landed — which the loop cannot see because its `== old`
    // check does not know `new` was already on screen at the first sample.
    assert!(
        seen == [old_size(), new_size()] || seen == [new_size()],
        "the frame must go from the old image to the new one with nothing in \
         between, or already show the new one; it went through {seen:?}",
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
    // frame already landed (also correct — the clear happened and the new
    // decode landed inside the same frame). Different sizes make the failure
    // message say which one happened. Only the retained OLD frame is the bug;
    // demanding the placeholder here would assert the new decode was still
    // in flight at the swap frame, a property of the runner, not of the
    // policy — the gapless sibling above failed on CI on exactly that
    // premise. The deterministic clear-on-swap pin is
    // `a_cached_to_cold_swap_clears_by_default_and_holds_under_gapless_playback`
    // below, where the completion is a test input and the placeholder frame
    // is therefore a guaranteed state.
    let swap_frame = laid.size(laid.current_root());
    assert_ne!(
        swap_frame,
        old_size(),
        "Flutter's default (gaplessPlayback: false) clears to the \
         placeholder the instant the provider key changes; the old frame was \
         retained instead",
    );
    assert!(
        swap_frame == size(0.0, 0.0) || swap_frame == new_size(),
        "the swap frame must be the placeholder or the already-landed new \
         image, got {swap_frame:?}",
    );

    // ...and the NEW provider resolves: clearing is a transition, not a dead
    // end. Waiting for the new image's size rather than a size both fixtures
    // share is what makes this assert the new one arrived, instead of being
    // satisfied by the old one reappearing.
    pump_until(&mut laid, |laid| {
        laid.size(laid.current_root()) == new_size()
    });
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
    PixelImage::from_rgba8(
        width,
        height,
        vec![255u8; width as usize * height as usize * 4],
    )
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
