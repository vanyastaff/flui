//! The frame a covered window owes its surface, against the runtime's own frame
//! flags and a sink that settles frames after they were handed over, as a
//! threaded raster side does (ADR-0357).

use std::num::NonZeroUsize;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use flui_foundation::{FrameEpoch, ManualClock, MonotonicClock as _, PresentationAddress};
use flui_native::{HeadlessHostWindow, HostWindow};
use flui_platform::{OccludedSurface, PlatformWindow as _, WindowCapability};
use flui_runtime::presentation::PresentationWindow;
use flui_runtime::sink::{FrameSettlement, FrameSink, SubmitVerdict};
use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};

/// A sink that queues every frame and keeps each frame's epoch, holding at most
/// `limit` frames of a presentation unsettled.
struct Queues {
    epochs: Vec<FrameEpoch>,
    limit: NonZeroUsize,
}

impl Queues {
    fn holding(limit: usize) -> Self {
        Self {
            epochs: Vec::new(),
            limit: NonZeroUsize::new(limit).expect("BUG: a sink holds at least one frame"),
        }
    }

    fn last(&self) -> FrameEpoch {
        *self.epochs.last().expect("a frame was submitted")
    }
}

impl FrameSink for Queues {
    fn surface_size(&mut self) -> (u32, u32) {
        (400, 400)
    }

    fn in_flight_limit(&mut self) -> NonZeroUsize {
        self.limit
    }

    fn submit(&mut self, epoch: FrameEpoch, _: flui_layer::ScenePublication) -> SubmitVerdict {
        self.epochs.push(epoch);
        SubmitVerdict::Queued
    }
}

/// A runtime on `clock` showing a headless window that tells a covering, its
/// surface `surface`, its first frame submitted to `sink` and left unsettled.
struct Covering {
    runtime: UiRuntime,
    window: Rc<dyn HostWindow>,
    clock: ManualClock,
}

impl Covering {
    fn open(surface: OccludedSurface, sink: &mut Queues) -> Self {
        let clock = ManualClock::new();
        let window = flui_native::headless_platform()
            .open_window(
                flui_platform::WindowOptions::default(),
                flui_testing::window_handler::RecordingWindowHandler::new(),
            )
            .expect("headless window");
        let headless = headless(&window);
        let mut parts = headless.state().capabilities.into_parts();
        parts.reports_occlusion = WindowCapability::Native;
        headless.simulate_capabilities(
            parts
                .into_capabilities()
                .publish()
                .expect("BUG: a test publishes far fewer than u64::MAX capabilities"),
        );
        let accessibility = window.accessibility();
        let mut runtime = UiRuntime::try_new(
            PresentationWindow::new(Rc::clone(&window) as _, accessibility)
                .with_occluded_surface(surface),
            1.0,
            RuntimeHostServices::new(
                Arc::new(|| {}),
                Arc::new(AtomicBool::new(false)),
                &flui_painting::FontCollection::new(),
                flui_scheduler::ClockSource::Manual(clock.clone()),
            ),
        )
        .expect("manual-clock runtime");
        runtime
            .attach_root_widget_with_size(&flui_widgets::SizedBox::square(10.0), 400.0, 400.0)
            .expect("mounted root");
        let _ = runtime.pump(&mut clock.clone(), sink);
        assert_eq!(sink.epochs.len(), 1, "the first frame is submitted");
        Self {
            runtime,
            window,
            clock,
        }
    }

    fn address(&self) -> PresentationAddress {
        PresentationAddress {
            ui_runtime_id: self.runtime.id(),
            presentation_id: self.runtime.presentation_id(),
        }
    }

    /// The window system covers the window, and the host reports it unseen.
    fn cover(&self) {
        headless(&self.window).simulate_occlusion(true);
        self.runtime
            .update_window_visibility(self.runtime.presentation_id(), false);
    }

    /// The content changes: work waits on the presentation.
    fn change(&self) {
        self.runtime.mark_primary_needs_full_repaint();
    }

    fn settle(&mut self, epoch: FrameEpoch, verdict: SubmitVerdict) {
        let address = self.address();
        let _ = self
            .runtime
            .settle_frame(FrameSettlement::new(address, epoch, verdict));
    }

    fn pump_after(&mut self, dt: Duration, sink: &mut Queues) {
        self.clock.advance(dt);
        let _ = self.runtime.pump(&mut self.clock.clone(), sink);
    }
}

fn headless(window: &Rc<dyn HostWindow>) -> &HeadlessHostWindow {
    window
        .as_any()
        .downcast_ref::<HeadlessHostWindow>()
        .expect("a headless window")
}

/// A window that cannot be seen asks for no frame: a redraw requested of it
/// leaves the runtime's redraw flag down, so no host takes it for a frame
/// owed now. Fails when a hidden presentation's redraw request raises the
/// flag, which every runner and the headless host read as a frame to run.
#[test]
fn a_hidden_window_raises_no_redraw_flag() {
    let mut sink = Queues::holding(2);
    let mut covering = Covering::open(OccludedSurface::Discarded, &mut sink);
    covering.settle(sink.last(), SubmitVerdict::Presented);
    covering.cover();
    covering.change();
    assert!(
        !covering.runtime.needs_redraw(),
        "a hidden window's redraw request asks the host for no frame"
    );
    assert!(
        !covering.runtime.runs_frames(),
        "with no window seen and none owing a frame, a wake runs none"
    );
}

/// A frame drawn while the window was seen and settled unshown after the
/// covering is not the owed frame: the owed frame is still drawn once quiet.
/// Its own refusal ends the owing. Fails when any frame settling unshown while
/// the window is unseen refuses the owed frame.
#[test]
fn an_owed_frame_is_refused_by_its_own_settlement_only() {
    let mut sink = Queues::holding(2);
    let mut covering = Covering::open(OccludedSurface::Kept, &mut sink);
    let seen_frame = sink.last();
    covering.cover();
    covering.settle(seen_frame, SubmitVerdict::NotShown);
    covering.change();
    covering.pump_after(Duration::from_millis(1100), &mut sink);
    assert_eq!(
        sink.epochs.len(),
        2,
        "the frame of before the covering refused nothing: the owed frame is drawn"
    );
    covering.settle(sink.last(), SubmitVerdict::NotShown);
    covering.change();
    covering.pump_after(Duration::from_secs(2), &mut sink);
    assert_eq!(
        sink.epochs.len(),
        2,
        "the owed frame's own refusal ends the owing"
    );
}

/// A hold on the owed frame (here a sink holding as many frames as it may)
/// leaves it due: once the hold ends, the runtime asks for a wake now, not
/// after another quiet period, and the frame is drawn. Fails when an attempt
/// the hold blocks starts the next quiet period.
#[test]
fn a_hold_leaves_the_owed_frame_due() {
    let mut sink = Queues::holding(1);
    let mut covering = Covering::open(OccludedSurface::Kept, &mut sink);
    let seen_frame = sink.last();
    covering.cover();
    covering.change();
    covering.pump_after(Duration::from_millis(1100), &mut sink);
    assert_eq!(sink.epochs.len(), 1, "the full sink holds the owed frame");
    covering.settle(seen_frame, SubmitVerdict::Presented);
    let now = covering.clock.now();
    assert!(
        covering.runtime.next_wake().is_some_and(|wake| wake <= now),
        "the hold ended: the owed frame is due now, got {:?}",
        covering
            .runtime
            .next_wake()
            .map(|wake| wake.saturating_duration_since(now))
    );
    assert!(covering.runtime.runs_frames(), "and the wake runs it");
    covering.pump_after(Duration::ZERO, &mut sink);
    assert_eq!(sink.epochs.len(), 2, "the owed frame is drawn");
}
