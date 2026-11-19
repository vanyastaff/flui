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
use flui_platform::{EventTime, OccludedSurface, PlatformWindow as _, WindowCapability};
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

    /// The window system shows the window again, and the host reports it seen.
    fn uncover(&self) {
        headless(&self.window).simulate_occlusion(false);
        self.runtime
            .update_window_visibility(self.runtime.presentation_id(), true);
    }

    /// The content changes: work waits on the presentation.
    fn change(&self) {
        self.runtime.mark_primary_needs_full_repaint();
    }

    /// Deliver what the runtime asks of its window for the owed frame, as a
    /// host does at the end of an owner work item.
    fn ask_owed_frames(&self) {
        for request in self.runtime.owed_frame_requests() {
            request.deliver();
        }
    }

    /// Every timed frame request the window took, oldest first.
    fn timed_frame_requests(&self) -> Vec<Option<EventTime>> {
        headless(&self.window).timed_frame_requests()
    }

    /// The runtime's clock on the event timeline.
    fn event_now(&self) -> EventTime {
        EventTime::from_nanos(
            u64::try_from(
                self.clock
                    .now()
                    .saturating_duration_since(self.clock.epoch())
                    .as_nanos(),
            )
            .expect("BUG: a test's virtual time fits the event timeline"),
        )
    }

    fn settle(&mut self, epoch: FrameEpoch, verdict: SubmitVerdict) {
        let address = self.address();
        let _ = self
            .runtime
            .settle_frame(FrameSettlement::new(address, epoch, verdict));
    }

    /// A pump `dt` later with no frame of the covered window's own: as a
    /// sibling's frame, or work due elsewhere, runs the runtime.
    fn pump_after(&mut self, dt: Duration, sink: &mut Queues) {
        self.clock.advance(dt);
        let _ = self.runtime.pump(&mut self.clock.clone(), sink);
    }

    /// The window delivers a frame now, as its host does when it serves the
    /// window's timed request.
    fn serve(&self) {
        let vsync = self.event_now();
        self.runtime.note_frame_request(
            self.runtime.presentation_id(),
            flui_platform::FrameRequest::new(vsync),
        );
    }

    /// A pump `dt` later of the frame the window delivered then ([`Self::serve`]).
    fn serve_after(&mut self, dt: Duration, sink: &mut Queues) {
        self.clock.advance(dt);
        self.serve();
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
    covering.serve_after(Duration::from_millis(1100), &mut sink);
    assert_eq!(
        sink.epochs.len(),
        2,
        "the frame of before the covering refused nothing: the owed frame is drawn"
    );
    covering.settle(sink.last(), SubmitVerdict::NotShown);
    covering.change();
    covering.serve_after(Duration::from_secs(2), &mut sink);
    assert_eq!(
        sink.epochs.len(),
        2,
        "the owed frame's own refusal ends the owing"
    );
}

/// A hold on the owed frame (here a sink holding as many frames as it may)
/// leaves it due: while the sink is full the window is asked for nothing, and
/// once the hold ends it is asked for the frame now, not after another quiet
/// period, and the frame is drawn. Fails when an attempt the hold blocks
/// starts the next quiet period.
#[test]
fn a_hold_leaves_the_owed_frame_due() {
    let mut sink = Queues::holding(1);
    let mut covering = Covering::open(OccludedSurface::Kept, &mut sink);
    let seen_frame = sink.last();
    covering.cover();
    covering.change();
    covering.ask_owed_frames();
    covering.serve_after(Duration::from_millis(1100), &mut sink);
    covering.ask_owed_frames();
    assert_eq!(sink.epochs.len(), 1, "the full sink holds the owed frame");
    assert!(
        covering.timed_frame_requests().is_empty(),
        "a full sink asks for nothing: its settlement is what frees a place"
    );
    covering.settle(seen_frame, SubmitVerdict::Presented);
    covering.ask_owed_frames();
    let now = covering.event_now();
    let asked = covering.timed_frame_requests();
    assert!(
        asked.last().copied().flatten().is_some_and(|at| at <= now),
        "the hold ended: the window is asked for the owed frame now, got {asked:?}"
    );
    covering.serve();
    assert!(
        covering.runtime.runs_frames(),
        "the window's served frame runs it"
    );
    covering.pump_after(Duration::ZERO, &mut sink);
    assert_eq!(sink.epochs.len(), 2, "the owed frame is drawn");
}

/// The owed frame is drawn only by a frame its own window delivered once its
/// instant came, the frame that serves the window's timed request: a pump past
/// the instant with no frame of its window (a sibling's frame, work due
/// elsewhere) neither draws it nor counts it as due work or a frame to run,
/// so no loop turns for it. Fails when a due owed frame is counted as due
/// work: the pump draws it without the window's frame, and a runner's dirty
/// gate keeps pumping it.
#[test]
fn the_owed_frame_is_drawn_only_by_its_windows_served_frame() {
    let mut sink = Queues::holding(2);
    let mut covering = Covering::open(OccludedSurface::Kept, &mut sink);
    covering.settle(sink.last(), SubmitVerdict::Presented);
    covering.cover();
    covering.change();
    covering.pump_after(Duration::from_millis(1100), &mut sink);
    assert_eq!(
        sink.epochs.len(),
        1,
        "no frame of its window: nothing drawn"
    );
    assert!(
        !covering.runtime.has_pending_due_work(),
        "an owed frame no frame of its window served is no due work"
    );
    assert!(
        !covering.runtime.runs_frames() && !covering.runtime.owed_frame_served(),
        "nor a frame to run"
    );
    covering.serve_after(Duration::ZERO, &mut sink);
    assert_eq!(sink.epochs.len(), 2, "its window's served frame draws it");
    assert!(
        !covering.runtime.owed_frame_served(),
        "the attempt takes the serve"
    );
}

/// The owed frame is asked of the covered window itself, at the end of its
/// quiet period, once: the runtime's wake deadline, which a host delivers to
/// every window, does not carry it, and asking again with nothing changed asks
/// nothing. The frame drawn with nothing left withdraws the request, as does
/// the window being seen again. Fails when the owed instant rides the wake
/// deadline: the window is asked for nothing and the deadline holds it.
#[test]
fn the_owed_frame_is_asked_of_its_window_and_not_of_the_wake_deadline() {
    let mut sink = Queues::holding(2);
    let mut covering = Covering::open(OccludedSurface::Kept, &mut sink);
    covering.settle(sink.last(), SubmitVerdict::Presented);
    let covered_at = covering.event_now();
    covering.cover();
    covering.change();
    covering.ask_owed_frames();
    let owed = covered_at + Duration::from_secs(1);
    assert_eq!(
        covering.timed_frame_requests(),
        [Some(owed)],
        "the window is asked for its frame at the end of the quiet period"
    );
    assert_eq!(
        covering.runtime.next_wake(),
        None,
        "the runtime's wake deadline does not carry the owed frame"
    );
    for _ in 0..5 {
        covering.pump_after(Duration::from_millis(100), &mut sink);
        covering.ask_owed_frames();
    }
    assert_eq!(
        covering.timed_frame_requests(),
        [Some(owed)],
        "turns before its instant ask nothing more"
    );

    covering.serve_after(Duration::from_millis(600), &mut sink);
    covering.ask_owed_frames();
    assert_eq!(
        sink.epochs.len(),
        2,
        "the owed frame is drawn by the frame that serves its request"
    );
    assert_eq!(
        covering.timed_frame_requests(),
        [Some(owed), None],
        "drawn with nothing left: the request is withdrawn"
    );

    covering.change();
    covering.ask_owed_frames();
    let asked = covering.timed_frame_requests();
    assert!(
        matches!(asked[..], [_, None, Some(next)] if next > owed),
        "the next change is owed at the next quiet period's end: {asked:?}"
    );
    covering.uncover();
    covering.ask_owed_frames();
    assert_eq!(
        covering.timed_frame_requests().last(),
        Some(&None),
        "the window seen again: the request is withdrawn"
    );
}

/// An owner that runs work, and commits the install it holds when its
/// delivery asks it to.
#[derive(Default)]
struct WorkOnly {
    install: std::cell::RefCell<
        Option<(
            flui_runtime::owner::OwnerHost,
            flui_runtime::owner::PreparedInstall,
        )>,
    >,
}

impl flui_runtime::owner::OwnerEffects for WorkOnly {
    fn finish_install(
        &self,
        _: PresentationAddress,
        _: flui_runtime::owner::InitializationOutcome,
        _: flui_runtime::owner::RecoveryState,
    ) {
    }
    fn runtime_lifecycle(
        &self,
        _: flui_foundation::UiRuntimeId,
        _: flui_scheduler::AppLifecycleState,
    ) {
    }
    fn runtimes_stopped(&self, _: flui_runtime::owner::RecoveryState) {}
    fn frame(&self, _: PresentationAddress, _: &mut UiRuntime) {}
    fn commit_install(
        &self,
        token: flui_runtime::owner::InstallToken,
        _: flui_runtime::owner::RecoveryState,
    ) -> Option<flui_runtime::owner::InstallInitialization> {
        let (owner, prepared) = self.install.borrow_mut().take().expect("a pending install");
        let address = owner
            .publication(prepared)
            .expect("no runtime is checked out at the commit")
            .commit();
        assert_eq!(address, token.address());
        Some(flui_runtime::owner::InstallInitialization {
            address,
            observations: Vec::new(),
            lifecycle: None,
        })
    }
    fn retire_host(&self, _: &[PresentationAddress], _: flui_runtime::owner::RecoveryState) {}
    fn cancel_install(
        &self,
        _: flui_runtime::owner::InstallToken,
        _: flui_runtime::owner::RecoveryState,
    ) {
        panic!("no pending install");
    }
    fn resize_surface(&self, _: PresentationAddress, _: flui_foundation::geometry::WindowMetrics) {}
    fn retire_presentation(&self, _: PresentationAddress, _: Option<PresentationAddress>) {}
    fn after_turn(&self, _: flui_runtime::owner::RecoveryState) {}
    fn request_continuation(&self) -> bool {
        true
    }
}

/// A runner's owner delivery asks the covered window for its owed frame at
/// the end of the work item that left it owed, with the runtime back in the
/// owner, and asks nothing more for the same instant at the next item. Fails
/// when the owner delivery delivers no owed frame request: the window is
/// asked for nothing, and no deadline carries the frame.
#[test]
fn an_owner_work_item_asks_the_covered_window_for_its_owed_frame() {
    let mut sink = Queues::holding(2);
    let mut covering = Covering::open(OccludedSurface::Kept, &mut sink);
    covering.settle(sink.last(), SubmitVerdict::Presented);
    let owed = covering.event_now() + Duration::from_secs(1);
    covering.cover();
    let Covering {
        runtime, window, ..
    } = covering;
    let owner = flui_runtime::owner::OwnerHost::new();
    let address = owner
        .publication(owner.prepare_runtime(runtime))
        .expect("publish")
        .commit();
    let dispatcher = owner
        .presentation_dispatcher(address)
        .expect("a published presentation");
    // Each work item changes the covered window's content.
    let change = || {
        dispatcher
            .test_callback(
                Box::new(|runtime: &UiRuntime| runtime.mark_primary_needs_full_repaint()),
                &WorkOnly::default(),
            )
            .expect("the owner runs the work");
    };
    change();
    assert_eq!(
        headless(&window).timed_frame_requests(),
        [Some(owed)],
        "the work item that left the frame owed asks the window for it"
    );
    change();
    assert_eq!(
        headless(&window).timed_frame_requests(),
        [Some(owed)],
        "the next work item asks nothing for the same instant"
    );
}

/// A window installed covered, its content changed before the install,
/// owes its frame from the start: the installation asks its window for it,
/// with no later work item to do so. Fails when the install's delivery drops
/// the owed frame requests it took: the window is asked for nothing until
/// some other work reaches its runtime.
#[test]
fn a_window_installed_covered_is_asked_for_its_owed_frame_by_the_install() {
    let mut sink = Queues::holding(2);
    let mut covering = Covering::open(OccludedSurface::Kept, &mut sink);
    covering.settle(sink.last(), SubmitVerdict::Presented);
    let owed = covering.event_now() + Duration::from_secs(1);
    covering.cover();
    covering.change();
    let Covering {
        runtime, window, ..
    } = covering;
    let owner = flui_runtime::owner::OwnerHost::new();
    let mut prepared = owner.prepare_runtime(runtime);
    let token = prepared.take_delivery().expect("one delivery token");
    let effects = WorkOnly::default();
    *effects.install.borrow_mut() = Some((owner.clone(), prepared));
    let _ = owner
        .queue_install(token, &effects)
        .expect("the owner admits the install");
    assert!(
        effects.install.borrow().is_none(),
        "the install was committed"
    );
    assert_eq!(
        headless(&window).timed_frame_requests(),
        [Some(owed)],
        "the installed covered window is asked for its owed frame"
    );
}
