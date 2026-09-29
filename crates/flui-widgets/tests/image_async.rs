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
use std::time::{Duration, Instant};

use common::{lay_out, loose, size};
use flui_painting::paint::Image as PixelImage;
use flui_widgets::SizedBox;
use flui_widgets::{Image, ImageProvider, ImageProviderError};

/// Bounded budget for a real background file-read + decode to land as an
/// observed frame — generous for a 75-byte local fixture, never open-ended.
const DECODE_BUDGET: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(2);

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

/// Unmounting the widget cancels its load too — the `dispose` half of the
/// same rule, and the one that decides whether a completion can reach a state
/// that no longer exists.
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

/// Asynchronous image loads are owned by the widget that started them: a retired
/// provider's late completion is dropped, and unmounting cancels the load in flight.
#[test]
fn async_image_loads_are_owned_by_their_widget() {
    a_retired_providers_late_completion_cannot_replace_the_current_image();
    unmounting_the_widget_cancels_its_in_flight_load();
}
