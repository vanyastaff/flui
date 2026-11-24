//! A capture's frame sinks: registered until their handle drops, delivered every frame under
//! containment, and a capture's identity its own.
#![cfg(feature = "profiling")]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use flui_devtools::{Capture, CaptureSubscription, CapturedFrame};
use parking_lot::Mutex;
use tracing::Dispatch;
use tracing::field::{Field, Visit};
use tracing_subscriber::Registry;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::reload;

use crate::profiler::{capturing, frame};

/// Sets its flag when dropped: a sink that owns one says when its closure is destroyed.
struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// A sink that counts the frames it sees.
fn counting(count: &Arc<AtomicUsize>) -> impl Fn(&Capture, &CapturedFrame) + Send + Sync + use<> {
    let count = Arc::clone(count);
    move |_, _| {
        count.fetch_add(1, Ordering::SeqCst);
    }
}

fn one_frame(capture: &Capture) {
    capturing(capture, || frame(None, Duration::ZERO));
}

/// Dropping a sink's handle removes that sink and destroys its closure; a
/// sink registered beside it keeps seeing every frame.
///
/// Fails when the handle leaves its sink registered, or removes another sink.
#[test]
fn ending_a_sink_handle_removes_exactly_that_sink() {
    let capture = Capture::new();
    let (removed, kept) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let destroyed = Arc::new(AtomicBool::new(false));
    let removed_sink = {
        let count = counting(&removed);
        let flag = DropFlag(Arc::clone(&destroyed));
        capture.on_frame(move |capture, frame| {
            let _owned = &flag;
            count(capture, frame);
        })
    };
    let _kept_sink = capture.on_frame(counting(&kept));

    one_frame(&capture);
    assert!(!destroyed.load(Ordering::SeqCst), "the sink is live");
    drop(removed_sink);
    assert!(
        destroyed.load(Ordering::SeqCst),
        "the removed sink's closure is destroyed with its handle"
    );
    one_frame(&capture);

    assert_eq!(
        removed.load(Ordering::SeqCst),
        1,
        "the removed sink saw only the frame before"
    );
    assert_eq!(kept.load(Ordering::SeqCst), 2, "the other sink saw both");
}

/// The `ERROR` events a subscriber saw: their target and message.
#[derive(Clone, Default)]
struct ErrorEvents(Arc<Mutex<Vec<(String, String)>>>);

struct MessageText(String);

impl Visit for MessageText {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl<S: tracing::Subscriber> Layer<S> for ErrorEvents {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        if *event.metadata().level() == tracing::Level::ERROR {
            let mut message = MessageText(String::new());
            event.record(&mut message);
            self.0
                .lock()
                .push((event.metadata().target().to_owned(), message.0));
        }
    }
}

/// A sink that panics takes the frame from none of the sinks after it, the capture keeps
/// working for the next frame, and each frame's first panic is reported as one error event on
/// `flui.devtools` rather than raised.
///
/// Fails when the first panic stops delivery, unwinds out of the layer, goes unreported, or
/// when a later panic of the same frame is reported in its place.
#[test]
fn a_panicking_sink_is_reported_once_per_frame_and_the_others_still_run() {
    let capture = Capture::new();
    let seen = Arc::new(AtomicUsize::new(0));
    let _first = capture.on_frame(|_, _| panic!("the first sink that fails"));
    let _second = capture.on_frame(|_, _| panic!("the second sink that fails"));
    let _counting = capture.on_frame(counting(&seen));
    let errors = ErrorEvents::default();
    flui_testing::log_capture::disarm_interest_cache();
    let subscriber = tracing_subscriber::registry()
        .with(capture.layer())
        .with(errors.clone());

    tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
        frame(None, Duration::ZERO);
        frame(None, Duration::ZERO);
    });

    assert_eq!(
        seen.load(Ordering::SeqCst),
        2,
        "the last sink saw both frames"
    );
    assert_eq!(capture.frames().len(), 2);
    let errors = errors.0.lock().clone();
    assert_eq!(errors.len(), 2, "one report per frame: {errors:?}");
    for (target, message) in &errors {
        assert_eq!(target, "flui.devtools");
        assert!(
            message.contains("the first sink that fails")
                && !message.contains("the second sink that fails"),
            "the report names the first panic: {message}"
        );
    }
}

/// A slot for a sink's handle that a sink itself can empty.
type HandleSlot = Arc<Mutex<Option<CaptureSubscription>>>;

/// A sink that removes itself, and one that removes a later sink, during a delivery: nothing
/// deadlocks, that delivery still reaches every sink it started with, and the removed sinks
/// are not called on the next frame.
///
/// Fails when a removal waits on the lock a delivery holds, or changes the running delivery.
#[test]
fn a_sink_removed_during_a_delivery_finishes_that_delivery_and_misses_the_next() {
    let capture = Capture::new();
    let calls: Arc<Mutex<Vec<&'static str>>> = Arc::default();
    let (own, later): (HandleSlot, HandleSlot) = (Arc::default(), Arc::default());
    let sink = |name: &'static str, removes: Option<HandleSlot>| {
        let calls = Arc::clone(&calls);
        move |_: &Capture, _: &CapturedFrame| {
            calls.lock().push(name);
            if let Some(slot) = &removes {
                let handle = slot.lock().take();
                drop(handle);
            }
        }
    };
    *own.lock() = Some(capture.on_frame(sink("removes itself", Some(Arc::clone(&own)))));
    let _removes_later = capture.on_frame(sink("removes the next", Some(Arc::clone(&later))));
    *later.lock() = Some(capture.on_frame(sink("removed by the previous", None)));
    let _last = capture.on_frame(sink("last", None));

    one_frame(&capture);
    assert_eq!(
        *calls.lock(),
        [
            "removes itself",
            "removes the next",
            "removed by the previous",
            "last"
        ],
        "the delivery that removed them still ran every sink"
    );
    calls.lock().clear();
    one_frame(&capture);
    assert_eq!(*calls.lock(), ["removes the next", "last"]);
}

type CaptureFilter = tracing_subscriber::filter::Filtered<
    flui_devtools::CaptureLayer,
    flui_devtools::FrameSpanFilter,
    Registry,
>;

/// A capture created after another was dropped does not read what the dropped one left in a
/// span still open: a frame span opened under the first capture's layer, which closes after
/// that layer was replaced by a new capture's, is no frame of the new capture.
///
/// The new capture is created right after the first one is freed, so an allocator that reuses
/// the block hands it the dropped capture's address. Fails when a capture's state in a span is
/// keyed by that address instead of a minted identity.
#[test]
fn a_new_capture_does_not_read_a_dropped_captures_state() {
    let first = Capture::new();
    let (layer, swap) = reload::Layer::new(first.layer::<Registry>());
    flui_testing::log_capture::disarm_interest_cache();
    let dispatch = Dispatch::new(tracing_subscriber::registry().with(layer));
    let replace = |with: &mut CaptureFilter| {
        swap.modify(|installed| std::mem::swap(installed.inner_mut(), with.inner_mut()))
            .expect("the subscriber is alive");
    };

    let second = tracing::dispatcher::with_default(&dispatch, || {
        let opened = tracing::debug_span!(target: "flui.frame", "frame").entered();
        // The first capture's layer leaves the registry and the capture is freed.
        let stand_in = Capture::new();
        let mut taken = stand_in.layer::<Registry>();
        replace(&mut taken);
        drop((taken, first));
        let second = Capture::new();
        let mut installed = second.layer::<Registry>();
        replace(&mut installed);
        drop(opened);
        assert!(
            second.frames().is_empty(),
            "the frame the dropped capture opened is not the new capture's: {:?}",
            second.frames()
        );
        frame(None, Duration::ZERO);
        second
    });

    assert_eq!(second.frames().len(), 1, "the new capture's own frame");
}
