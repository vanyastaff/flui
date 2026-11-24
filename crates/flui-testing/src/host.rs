//! A headless host for a [`UiRuntime`]: the product frame transaction
//! (`UiRuntime::pump`, ADR-0083 §1) on a manual clock, a window double and a
//! sink that keeps what the frame composited.
//!
//! [`HeadlessHost`] is to a UI runtime what a runner is on screen: it owns the
//! window the UI runtime presents into, the sink a frame is submitted through and
//! the clock the frame reads, and it drives frames and input through the
//! UI runtime's own entry points. Nothing here re-implements a phase of the frame:
//! applying commands, begin frame, the pipeline, end frame and the text-store
//! commit anchor all run inside `UiRuntime::pump`.
//!
//! One [`ManualClock`] drives the whole ui_runtime: the UI runtime reads it as its
//! [`ClockSource`] (frame-time origin, gesture-arena deadlines, the
//! presentation's `FrameClock`), and [`HeadlessHost::pump`] hands a clone to
//! the pump as the frame's timestamp. A test advances time only through
//! [`HeadlessHost::pump`] or [`HeadlessHost::clock`].
//!
//! The same clock is the timeline of the host's [`Scenario`]: the scripted
//! clipboard, primary selection and any plugin built on
//! [`HeadlessHost::with_scenario`] answer on it, and each pump applies the
//! scenario's changes due at its instant before the frame's turn, so a
//! change scheduled at 3 s settles in the first pump at or after 3 s.
//!
//! # Failures
//!
//! A UI runtime contains a panic that escapes a frame segment, and a pipeline
//! error, as a dropped frame (ADR-0048) and reports it to its frame-failure
//! handler; on screen the process survives. Under test that containment would
//! hide the failure, so [`HeadlessHost::pump`] raises the first dropped-frame
//! report of the pump as a panic once the pump has returned, carrying the
//! report's text (the UI runtime retains it verbatim here). A later panic that
//! unwinds out of the same pump does not replace it: the first failure stays
//! authoritative. A dropped-frame report the UI runtime makes between pumps is
//! raised by the next pump, before it frames. A lifecycle panic the tree recovered from (an `ErrorView`
//! substitution) is not raised: the frame it happened in completed.

use std::cell::RefCell;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use flui_foundation::panic::FirstPanic;
use flui_foundation::{ManualClock, PresentationId};
use flui_platform::capability::scripted::{AlreadyDriven, Scenario};
use flui_platform::text::store::TextStoreHost;
use flui_platform::{PlatformWindow, WindowIdMint};

use flui_runtime::frame_failure::{
    FailureDisposition, FrameFailureDetail, FrameFailureHandler, FrameFailureKind,
    FrameFailureReport,
};
use flui_runtime::owner_turn::{CloseRequestRouter, QuitRequest, SessionEnd};
use flui_runtime::presentation::PresentationWindow;
use flui_runtime::pump::FrameOutcome;

use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};
use flui_scheduler::{ClockSource, PostFrameHandle};
use flui_semantics::platform::PlatformAccessibility;
use flui_view::__runtime::{FlushHost, FlushWrite, OwnerNotify};
use flui_view::CloseReason;
use parking_lot::Mutex;

use crate::text_store_host::RecordingTextStoreHost;

mod accessibility;
mod clock;
mod dev_agent;
mod errors;
mod input;
mod os_drag;
mod registrations;
mod scripted_built_ins;
mod session_end;
mod settling;
mod sink;
mod text_input;
mod text_sizing;
mod turn;
mod window;
mod window_control;
mod window_lifecycle;

use accessibility::HeadlessAccessibility;
pub use clock::HostClock;
pub use dev_agent::HeadlessDevAgent;
pub use errors::{NotSettled, WindowSystemMismatch};
pub use os_drag::{HeadlessOsDrags, StartedOsDrag};
// The kit's doubles this host's API hands out, beside it (ADR-0264 §4):
// `HeadlessHost::http` returns a `ScriptedHttp`, scripted with
// `ScriptedResponse`, and `HeadlessWindow::haptic_actuator` is a `FakeHaptics`.
pub use flui_platform::testing::{FakeHaptics, ScriptedHttp, ScriptedResponse};
pub(crate) use registrations::AppRegistrations;
use registrations::{Registry, compose_registry};
use scripted_built_ins::ScriptedBuiltIns;
pub use session_end::SESSION_END_BUDGET;
use session_end::session_end_support_of;
pub use sink::{HeadlessSink, UnshownFrame};
pub use text_sizing::ScriptedTextSizing;
pub use window::HeadlessWindow;
use window_control::{install_window_control, register_close_requests};

/// A [`UiRuntime`] hosted headlessly, driven frame by frame on a manual clock.
/// See the [module docs](self).
#[doc(alias = "HeadlessRuntime")]
pub struct HeadlessHost {
    /// The UI runtime, until the host's last window closes: a real host
    /// retires its UI runtime then, and so does this one
    /// ([`close_presentation`](Self::close_presentation)).
    ui_runtime: Option<UiRuntime>,
    clock: ManualClock,
    sink: HeadlessSink,
    /// The window the UI runtime was built on, kept after it closed so a
    /// test still reads what its double recorded.
    primary: HostWindow,
    scripted: ScriptedBuiltIns,
    /// The windows open, in opening order: the primary first while it is.
    open: Vec<HostWindow>,
    /// Issues the id of every window the host opens, the primary first,
    /// never one twice: a window opened after another closed has an id of
    /// its own.
    window_ids: WindowIdMint,
    /// The surfaces embedded in the primary window, in minting order.
    embedded: Vec<flui_interaction::ids::InputSurface>,
    /// Dropped-frame reports not yet raised, as the text a raised failure
    /// carries: those of the pump in progress, and any the UI runtime made
    /// between pumps, which the next pump raises before it frames.
    failures: Rc<RefCell<Vec<String>>>,
    /// The presentation report the next pump's frame requests carry
    /// ([`Self::report_presented`]).
    presented: Option<flui_platform::FramePresented>,
    /// The flush registry's host side, when the host was given storage: what
    /// the widgets' documents publish, written as a runner writes it.
    flush: Option<FlushHost>,
    /// The registry's writes waiting for the next pump to run them, as a
    /// runner's IO pool runs them.
    // `Arc<Mutex>`: the writer closure `FlushHost::new` takes is `Send + Sync`.
    flush_writes: Arc<Mutex<Vec<FlushWrite>>>,
    /// The closes the windows' widgets asked for, which the next turn
    /// carries out.
    close_asks: CloseAsks,
    /// The UI runtime's id, kept after it retired: half of every window's
    /// address.
    ui_runtime_id: flui_foundation::UiRuntimeId,
    /// Every open window's close handler and guard, which every close, quit
    /// and session-end question is put to, as a runner's.
    close_requests: Rc<CloseRequestRouter>,
    /// The closes the window's platform lets the application refuse
    /// ([`close_vetoes`](flui_runtime::owner_turn::close_vetoes)).
    close_vetoes: Vec<CloseReason>,
    /// Where a quit the application or the user asked for stands.
    quit: QuitRequest,
    /// A quit no window holds any more, which the turn carries out.
    quit_owed: bool,
    /// The session end being answered: the query's request and the windows
    /// that refused it.
    session_end: Rc<SessionEnd>,
    /// Whether a phone's session end answered `Block` and holds the
    /// background until its writes end.
    background_hold: bool,
    /// How many times the application let the platform's session end go on:
    /// a phone's background hold whose writes ended, or the last block a
    /// query's request held.
    session_end_releases: Rc<std::cell::Cell<usize>>,
    /// The scripted text-sizing provider a test installed
    /// ([`Self::script_text_sizing`]), which takes the runtime's frontiers
    /// after each pump.
    text_sizing: Option<Rc<std::cell::RefCell<text_sizing::ScriptedTextSizingState>>>,
}

impl std::fmt::Debug for HeadlessHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadlessHost")
            .field("ui_runtime", &self.ui_runtime)
            .field("window", &self.primary.window)
            .field("open", &self.open.len())
            .field("sink", &self.sink)
            .finish_non_exhaustive()
    }
}

/// The storage a headless host gives its UI runtime: the host's own, and
/// the storage the application provides as the
/// [`StorageCapability`](flui_view::persist::StorageCapability) in its
/// place, if any. The host's flush writes each document's bytes to the
/// storage that document acquired.
#[derive(Clone)]
pub(crate) struct KitStorage {
    pub(crate) host: Arc<dyn flui_platform::Storage>,
    pub(crate) provided: Option<Arc<dyn flui_platform::Storage>>,
}

impl HeadlessHost {
    /// A UI runtime over `window`, whose surface has the window's size.
    ///
    /// # Panics
    ///
    /// If the UI runtime's interaction lane cannot be created (its identity space
    /// is exhausted).
    #[must_use]
    pub fn new(window: HeadlessWindow) -> Self {
        Self::with_storage(window, None)
    }

    /// [`Self::new`], its widgets acquiring platform capabilities from
    /// `registry` alone (ADR-0152): the host's [`clipboard`](Self::clipboard)
    /// reaches them only if `registry` provides it.
    ///
    /// # Panics
    ///
    /// As [`Self::new`].
    #[must_use]
    pub fn with_capabilities(
        window: HeadlessWindow,
        registry: flui_runtime::capability::CapabilityRegistry,
    ) -> Self {
        Self::assemble(window, None, Registry::Given(registry), None)
    }

    /// [`Self::new`], its widgets acquiring what `plugin` registers beside
    /// the host's built-ins (the [`clipboard`](Self::clipboard), and the
    /// [`primary_selection`](Self::primary_selection) where the window's
    /// platform has one): how a package tests its plugin on a headless host
    /// (ADR-0152).
    ///
    /// # Panics
    ///
    /// When `plugin` registers a capability twice, or under the name of
    /// another; and as [`Self::new`].
    #[must_use]
    pub fn with_plugin(
        window: HeadlessWindow,
        plugin: &dyn flui_platform::capability::Plugin,
    ) -> Self {
        Self::with_scenario(window, &Scenario::new(), plugin)
    }

    /// [`Self::with_plugin`] on `scenario`: the host drives the scenario's
    /// clock (frames, deadlines and scheduled changes on one timeline), and
    /// its built-ins (the clipboard, the primary selection and the HTTP
    /// client, each where it is registered) are on it, so a
    /// plugin whose doubles are on `scenario` answers on the same timeline as
    /// everything else (ADR-0154 §10). A scenario drives one host at a time:
    /// time moves only through that host.
    ///
    /// # Panics
    ///
    /// When another live host already drives `scenario`
    /// ([`Self::try_with_scenario`] returns that refusal instead); and as
    /// [`Self::with_plugin`].
    #[must_use]
    pub fn with_scenario(
        window: HeadlessWindow,
        scenario: &Scenario,
        plugin: &dyn flui_platform::capability::Plugin,
    ) -> Self {
        Self::try_with_scenario(window, scenario, plugin)
            .unwrap_or_else(|refused| panic!("with_scenario: {refused}: one host per scenario"))
    }

    /// [`Self::with_scenario`], refusing instead of panicking when another
    /// live host already drives `scenario`.
    ///
    /// # Errors
    ///
    /// [`AlreadyDriven`] while another host drives `scenario`.
    ///
    /// # Panics
    ///
    /// As [`Self::with_plugin`].
    pub fn try_with_scenario(
        window: HeadlessWindow,
        scenario: &Scenario,
        plugin: &dyn flui_platform::capability::Plugin,
    ) -> Result<Self, AlreadyDriven> {
        let scripted = ScriptedBuiltIns::on(scenario.drive()?);
        let registry = compose_registry(
            &scripted,
            window.window_system(),
            &window.assets,
            None,
            &AppRegistrations::new(),
            Some(plugin),
        );
        Ok(Self::assemble(
            window,
            None,
            Registry::Given(registry),
            Some(scripted),
        ))
    }

    /// [`Self::new`], its widgets acquiring `storage` as
    /// the [`StorageCapability`](flui_view::persist::StorageCapability).
    pub(crate) fn with_storage(window: HeadlessWindow, storage: Option<KitStorage>) -> Self {
        Self::with_application(window, storage, AppRegistrations::default())
    }

    /// [`Self::with_storage`], with what the application registers over the
    /// built-ins: its overrides and its plugins, as a runner builds the
    /// registry of an `Application` (ADR-0152).
    ///
    /// # Panics
    ///
    /// When the application's registrations conflict, as a run fails before
    /// its platform starts; and as [`Self::new`].
    pub(crate) fn with_application(
        window: HeadlessWindow,
        storage: Option<KitStorage>,
        application: AppRegistrations,
    ) -> Self {
        Self::assemble(window, storage, Registry::Composed(application), None)
    }

    fn assemble(
        mut window: HeadlessWindow,
        storage: Option<KitStorage>,
        registry: Registry,
        scripted: Option<ScriptedBuiltIns>,
    ) -> Self {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the window size came from u32 pixel counts"
        )]
        let surface = (window.size.width() as u32, window.size.height() as u32);
        let text_store_host = window
            .text_store_host
            .map(RecordingTextStoreHost::with_selection_ui);
        let gesture_preset = window.gesture_preset.clone();
        let platform = window.platform;
        let window_system = window.window_system();
        let close_handler = window.close_request_handler.take();
        let assets = window.assets.clone();
        let window_ids = WindowIdMint::new();
        window.id = window_ids
            .next()
            .expect("BUG: a fresh mint issues the primary window's id");
        let window = Rc::new(window);
        let accessibility = Arc::new(HeadlessAccessibility::default());
        let scripted = scripted.unwrap_or_else(|| {
            ScriptedBuiltIns::on(
                Scenario::new()
                    .drive()
                    .expect("BUG: a scenario made here has no other driver"),
            )
        });
        let clock = scripted.clock.driver.clock().clone();
        // A collection of its own: the ui_runtime owns a `TextContext` over it
        // (ADR-0092 §3), exactly as a hosted ui_runtime does. Deliberately
        // bundled-only, unlike the app's host-fed one
        // (`FontCollection::with_host_fonts`), so text measures the same on
        // every host a test runs on.
        let fonts = flui_painting::FontCollection::new();
        // The wake sets the redraw flag, as a runner's frame wake does, so the
        // runtime's demand for a next frame is observable here.
        let needs_redraw = Arc::new(AtomicBool::new(false));
        let wake_flag = Arc::clone(&needs_redraw);
        let flush_writes = Arc::new(Mutex::new(Vec::new()));
        let flush = storage.as_ref().map(|_| {
            let queued = Arc::clone(&flush_writes);
            FlushHost::new(
                Box::new(move |write| {
                    queued.lock().push(write);
                    Ok(())
                }),
                // Every pump settles the registry, so a signal has nothing to wake.
                OwnerNotify::new(|_| Ok(())),
            )
        });
        let mut host = RuntimeHostServices::new(
            Arc::new(move || wake_flag.store(true, Ordering::Relaxed)),
            needs_redraw,
            &fonts,
            ClockSource::Manual(clock.clone()),
        );
        let registry = match registry {
            Registry::Given(registry) => registry,
            Registry::Composed(application) => compose_registry(
                &scripted,
                window_system,
                &assets,
                storage.as_ref(),
                &application,
                None,
            ),
        };
        host = host.with_capabilities(registry);
        if let Some(flush) = &flush {
            host = host.with_flush_registry(flush.registry());
        }
        let close_vetoes = flui_runtime::owner_turn::close_vetoes(
            window_system,
            Some(session_end_support_of(window_system)),
        );
        // The host gathers its half of what keeps the application from being
        // idle and answers the agent's settled waits after every turn.
        host = host.with_close_vetoes(&close_vetoes).composes_host_half();
        let ui_runtime = UiRuntime::try_new(
            PresentationWindow::new(
                Rc::clone(&window) as Rc<dyn PlatformWindow>,
                Some(Arc::clone(&accessibility) as Arc<dyn PlatformAccessibility>),
            )
            .with_text_store_host(
                text_store_host
                    .clone()
                    .map(|host| host as Rc<dyn TextStoreHost>),
            )
            .with_gesture_profiles(gesture_preset)
            .with_occluded_surface(window.occluded_surface)
            .with_platform(platform),
            1.0,
            host,
        )
        .expect("BUG: interaction lane identity exhausted");
        let close_asks = CloseAsks::default();
        install_window_control(
            &ui_runtime,
            ui_runtime.presentation_id(),
            &window,
            &close_asks,
        );
        let close_requests = Rc::new(CloseRequestRouter::new());
        register_close_requests(
            &close_requests,
            &close_vetoes,
            &ui_runtime,
            ui_runtime.presentation_id(),
            &window,
            close_handler,
        );
        let failures = Rc::new(RefCell::new(Vec::new()));
        ui_runtime.set_frame_failure_detail(FrameFailureDetail::Verbatim);
        let recorded = Rc::clone(&failures);
        ui_runtime.set_frame_failure_handler(Some(FrameFailureHandler::new(move |report| {
            if let Some(text) = dropped_frame_text(report) {
                recorded.borrow_mut().push(text);
            }
        })));
        let primary = HostWindow {
            id: ui_runtime.presentation_id(),
            window,
            accessibility,
            text_store_host,
        };
        Self {
            ui_runtime_id: ui_runtime.id(),
            ui_runtime: Some(ui_runtime),
            clock,
            sink: HeadlessSink::new(surface.0, surface.1),
            open: vec![primary.clone()],
            primary,
            scripted,
            window_ids,
            embedded: Vec::new(),
            failures,
            presented: None,
            flush,
            flush_writes,
            close_asks,
            close_requests,
            close_vetoes,
            quit: QuitRequest::new(),
            quit_owed: false,
            session_end: Rc::default(),
            background_hold: false,
            session_end_releases: Rc::default(),
            text_sizing: None,
        }
    }

    /// [`ui_runtime`](Self::ui_runtime), to change.
    ///
    /// # Panics
    ///
    /// Once the host's last window closed, which retired it.
    fn ui_runtime_mut(&mut self) -> &mut UiRuntime {
        self.ui_runtime
            .as_mut()
            .expect("a host whose last window closed has retired its UI runtime; `try_ui_runtime` answers None then")
    }

    /// Whether the host's last window closed, retiring its UI runtime as a
    /// real host does: what the UI runtime still held to answer ended as
    /// [`OwnerRetired`](flui_platform::capability::OwnerRetired), and a
    /// [`pump`](Self::pump) moves only time, the scenario, the writes and
    /// the background lanes, as [`time_to_next_frame`](Self::time_to_next_frame)
    /// and [`pump_until_settled`](Self::pump_until_settled) wait for. An entry
    /// that reaches the UI runtime (input, semantics, a window's attachment)
    /// panics on a retired host.
    #[must_use]
    pub fn is_retired(&self) -> bool {
        self.ui_runtime.is_none()
    }

    /// Report that the primary window's frame of `frame` reached the screen
    /// at `at`, on a display refreshing every `refresh` where it names a
    /// period, as a host's platform does (DXGI statistics,
    /// `wp_presentation_time`): the report names that frame
    /// ([`FramePresented::frame`](flui_platform::FramePresented::frame)), and
    /// the next [`pump`](Self::pump)'s frame request of the primary window
    /// carries it
    /// ([`FrameRequest::presented`](flui_platform::FrameRequest::presented)),
    /// which its frame clock keeps. Its time is on the event timeline input
    /// and frames are stamped on: the time since the [`clock`](Self::clock)
    /// started. A later report before that pump replaces it.
    ///
    /// Returns whether the report was made. As a real host reports only a
    /// frame it presented, a frame the sink did not present recently
    /// ([`HeadlessSink::last_presented`]) is not reported, and neither is any
    /// frame of a primary window whose capabilities say it reports no
    /// presentation (made without
    /// [`HeadlessWindow::with_presentation_reports`]).
    pub fn report_presented(
        &mut self,
        frame: flui_foundation::FrameEpoch,
        at: flui_platform::EventTime,
        refresh: Option<flui_platform::RefreshPeriod>,
    ) -> bool {
        if !self.primary.window.reports_presentation
            || !self.sink.presented(frame)
            || self.is_closed(self.primary_window())
        {
            return false;
        }
        let address = flui_foundation::PresentationAddress {
            ui_runtime_id: self.ui_runtime().id(),
            presentation_id: self.primary.id,
        };
        // The double draws on one surface, its first.
        let stamp = flui_foundation::FrameStamp::new(
            address,
            frame,
            flui_foundation::SurfaceGeneration::ZERO.next(),
        );
        let mut presented = flui_platform::FramePresented::new(stamp, at);
        if let Some(refresh) = refresh {
            presented = presented.refresh(refresh);
        }
        self.presented = Some(presented);
        true
    }

    /// Hand this UI runtime's window to `agent`'s hook, as a runner hands it a
    /// window it has installed. The window's first semantics tree is built
    /// by the next [`pump`](Self::pump); dropping the UI runtime closes the window,
    /// and the hook's handle to it answers `gone` from then on.
    ///
    /// Does nothing when the hook is not attached.
    #[must_use]
    pub fn with_dev_agent(self, agent: &mut HeadlessDevAgent) -> Self {
        agent.host.publish(self.ui_runtime(), self.primary.id);
        self
    }

    /// Hand `window`, which this host [opened](Self::open_window), to `agent`'s hook, as a
    /// runner hands each window it installs; closing it ([`Self::request_close`]) closes the
    /// hook's handle to it.
    ///
    /// Does nothing when the hook is not attached.
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host.
    pub fn serve_window(&self, window: HeadlessWindowId, agent: &mut HeadlessDevAgent) {
        let presentation = self.open_window_of(window).id;
        agent.host.publish(self.ui_runtime(), presentation);
    }

    /// Attach `view` as the UI runtime's root widget, the root view sized to the
    /// window. Like a `flui-app` runner, the host first wraps it in
    /// [`flui_widgets::app_root`] (Tab, Shift+Tab, the clipboard chords), and
    /// the UI runtime then wraps that in its root scopes (`GestureArenaScope`,
    /// `VsyncScope`, `FocusTreeRoot`, `MediaQuery`); the first
    /// [`pump`](Self::pump) builds, lays out and paints it.
    ///
    /// # Errors
    ///
    /// [`flui_view::__runtime::AttachError`] if a root is already attached.
    pub fn attach<V>(&self, view: &V) -> Result<(), flui_view::__runtime::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        self.attach_to(self.primary_window(), view)
    }

    /// Advance the clock by `dt`, then run one frame through
    /// `UiRuntime::pump` at the new instant.
    ///
    /// The clock moves before the frame starts, once: a caller that catches
    /// a raised failure and pumps again with `Duration::ZERO` retries at the
    /// same instant.
    ///
    /// # Panics
    ///
    /// With a dropped-frame report the UI runtime made since the last pump, before
    /// this one frames or moves the clock; with the first dropped-frame
    /// report of this pump (a segment panic or a pipeline error the UI runtime
    /// contained), after the pump returned; or with a panic that unwound out
    /// of the pump when nothing was reported before it; after a frame that
    /// failed nothing, when a window's semantics publication found an author
    /// error (an attribute its node's role does not support, or in debug
    /// builds siblings sharing an identifier), in every open window at once;
    /// a frame that failed otherwise drops its findings with it. See the
    /// [module docs](self#failures).
    pub fn pump(&mut self, dt: Duration) -> FrameOutcome {
        let pending = std::mem::take(&mut *self.failures.borrow_mut());
        if let Some(failure) = pending.into_iter().next() {
            panic!("{failure}");
        }
        // The scenario's changes due at the new instant apply before the
        // turn, so their answers settle in this pump.
        self.advance(dt);
        self.settle_lifecycle();
        // Background work runs before the frame, so an answer it posts
        // settles in this pump.
        self.scripted.background.drain_unless_held();
        if self.is_retired() {
            // The last window closed in an earlier turn or in this one's
            // settle: there is nothing left to frame.
            return FrameOutcome::default();
        }
        // A frame or mode a widget set reaches the UI runtime before the frame, as a
        // window system's state notice does. Walked by position rather than
        // over a collected list, so an idle pump asks nothing of the heap.
        let mut position = 0;
        while let Some(presentation) = self.presentation_at(position) {
            position += 1;
            if self
                .window_of(HeadlessWindowId(presentation))
                .take_unobserved()
            {
                self.observe(presentation, flui_runtime::owner::WindowObservation::State);
            }
        }
        // Each open presentation's texture frames ride the frames its scene
        // publishes, as each window's ride its raster host. Refreshed in place
        // by position, so an idle pump asks nothing of the heap.
        self.track_textures();
        // The frame is delivered as `HostInput::Frame` would be: every
        // window's vsync is the clock's reading, on the event timeline, so a
        // recording sees the frame where its deadlines fire.
        let vsync = self.event_now();
        let mut request = flui_platform::FrameRequest::new(vsync);
        if let Some(presented) = self.presented.take() {
            request = request.presented(presented);
        }
        // As a window system does, a window that cannot be seen is asked for no
        // frame, unless its timed frame request came due: this frame serves that
        // request, once, and only that frame of its own window serves the frame
        // a covered window owes its surface (ADR-0389).
        for window in &self.open {
            let served = window.window.serve_timed_frame(vsync);
            if served || window.window.state().is_seen() {
                self.ui_runtime().note_frame_request(window.id, request);
            }
        }
        let mut frame_clock = self.clock.clone();
        let Self {
            ui_runtime: Some(ui_runtime),
            sink,
            ..
        } = self
        else {
            unreachable!("BUG: a host that is not retired holds its UI runtime");
        };
        // The pump's own steps and the answers after them unwind into one
        // fold, in the order they ran, so the failures settle before the
        // first is resumed; a dropped-frame report the runtime contained is
        // raised ahead of both.
        let mut first_panic = FirstPanic::new();
        // As a runner's wake gate: a runtime whose frames are off (no window
        // seen) takes a background turn, unless a covered window owes its
        // surface a frame.
        let outcome = first_panic.contain(|| {
            if ui_runtime.runs_frames() {
                ui_runtime.pump(&mut frame_clock, sink)
            } else {
                ui_runtime.pump_background();
                FrameOutcome::default()
            }
        });
        // A frame whose lineage the pump renewed (a hot reload) takes its
        // presentation's texture updates now, from the registry its new
        // lineage names.
        self.track_textures();
        self.sink.claim_textures();
        let first_failure = {
            let mut failures = self.failures.borrow_mut();
            let first = failures.first().cloned();
            failures.clear();
            first
        };
        // As a runner's owner delivery does at the end of each work item: the
        // text-sizing frontiers the frame asked for go to the provider, then
        // each covered window is asked for the frame it owes.
        self.serve_text_sizing();
        self.ask_owed_frames();
        // Once the turn's own steps are done, the list they leave answers the
        // agent's settled waits, on every outcome. A reply's waker is
        // contained by the reply itself; the rest of an answer pass can still
        // unwind (the wake it asks for frames a parked wait counts, a
        // subscriber of its events), and that panic stays behind the frame's
        // own failure.
        first_panic.contain(|| self.answer_settled());
        // Taken on every outcome: a failed frame's findings are its own, and
        // the failure that is raised already fails the test.
        let semantics_findings = self.take_semantics_findings();
        if let Some(failure) = first_failure {
            if let Some(payload) = first_panic.into_payload() {
                tracing::error!(
                    first_failure = %failure,
                    "a panic unwound out of the pump or its answers after a contained frame \
                     failure; the contained failure is raised and the later panic is discarded"
                );
                // Destroyed under containment (ADR-0281).
                flui_foundation::panic::discard_payload(payload);
            }
            panic!("{failure}");
        }
        first_panic.resume();
        let outcome = outcome.expect("BUG: a pump that unwound nothing returned its outcome");
        assert!(
            semantics_findings.is_empty(),
            "the frame's semantics publication found author errors, by presentation: \
             {semantics_findings:?}"
        );
        outcome
    }

    /// Takes every open window's semantics findings from the frame that just
    /// ran ([`crate::take_semantics_findings`]), each beside its presentation;
    /// none once the last window closed and the UI runtime retired.
    /// Every pump takes them, whatever its outcome, so a finding is never
    /// reported against a later frame.
    fn take_semantics_findings(
        &self,
    ) -> Vec<(
        flui_foundation::PresentationId,
        Vec<flui_semantics::SemanticsDiagnostic>,
    )> {
        let Some(ui_runtime) = self.try_ui_runtime() else {
            return Vec::new();
        };
        self.open
            .iter()
            .filter_map(|window| {
                let pipeline = ui_runtime
                    .presentation_widgets_for_test(window.id)
                    .pipeline_owner()?;
                let findings = crate::take_semantics_findings(&pipeline);
                (!findings.is_empty()).then_some((window.id, findings))
            })
            .collect::<Vec<_>>()
    }

    /// Leave the next `frames` frames unshown for the reason `how` names, as
    /// a real surface does: those frames render, their tree stays
    /// uncommitted, and the UI runtime recovers as `how` says. Replaces what
    /// an earlier call left to come; `frames` of 0 shows every frame again.
    pub fn leave_unshown(&mut self, frames: u32, how: UnshownFrame) {
        self.sink.unshown = NonZeroU32::new(frames).map(|frames| (frames, how));
    }

    /// Run `f` inside the UI runtime's owner scope (its interaction lane, global
    /// key registry and post-frame lane), as every UI runtime entry point does.
    ///
    /// Crate-private, as is [`ui_runtime`](Self::ui_runtime): the UI runtime's frame entry
    /// points are the host's (ADR-0083 §2), and `flui-runtime` is not an
    /// embedder API, so a test reaches the UI runtime only through this host.
    pub(crate) fn enter<R>(&self, f: impl FnOnce(&UiRuntime) -> R) -> R {
        self.ui_runtime().enter(f)
    }

    /// The hosted UI runtime.
    ///
    /// # Panics
    ///
    /// Once the host's last window closed, which retired it.
    pub(crate) fn ui_runtime(&self) -> &UiRuntime {
        self.try_ui_runtime().expect(
            "a host whose last window closed has retired its UI runtime; `try_ui_runtime` \
             answers None then",
        )
    }

    /// The hosted UI runtime, `None` once the host's last window closed.
    pub(crate) fn try_ui_runtime(&self) -> Option<&UiRuntime> {
        self.ui_runtime.as_ref()
    }

    /// The owner-local post-frame handle the UI runtime installed on its build
    /// owner: a callback scheduled through it runs in the post-frame phase
    /// after this presentation has coherent geometry. An incomplete frame
    /// retains it until repair; an ordinary host-frame callback uses the
    /// scheduler's unscoped lane instead.
    ///
    /// # Panics
    ///
    /// If the UI runtime installed no owner-local post-frame handle, which a
    /// UI runtime's presentation always does.
    #[must_use]
    pub fn post_frame_handle(&self) -> PostFrameHandle {
        self.ui_runtime()
            .widgets()
            .with_build_owner(|owner| owner.post_frame_handle().cloned())
            .expect("BUG: a ui_runtime's presentation installs its owner-local post-frame handle")
    }

    /// Ask the UI runtime for a frame, as a widget's `setState` would: the next
    /// [`pump`](Self::pump) runs the pipeline even with nothing dirty.
    pub fn request_frame(&self) {
        self.ui_runtime().request_redraw();
    }

    /// Apply application motion policy to this host's UI runtime and all its
    /// presentations. Existing animations observe it at the next frame;
    /// presentations opened later inherit it before their first root build.
    ///
    /// # Panics
    ///
    /// Propagates the first inherited-data or frame-request callback failure,
    /// after the runtime has committed the preference to its presentations.
    pub fn set_motion_override(&self, preference: flui_animation::MotionOverride) {
        self.enter(|runtime| runtime.set_motion_override(preference));
    }

    /// The clock the UI runtime reads: the frame time, the gesture-arena
    /// deadlines and the produce gate. It moves only through
    /// [`HostClock::advance`], [`advance`](Self::advance) and
    /// [`pump`](Self::pump), each of which applies the scenario's changes
    /// that fall due.
    #[must_use]
    pub fn clock(&self) -> &HostClock {
        &self.scripted.clock
    }

    /// Move the clock forward by `dt` without a frame, and apply every
    /// change of the host's [scenario](Self::scenario) due at the new
    /// instant, as time passing between two input events does. Every path of
    /// the host that moves time goes through it.
    pub fn advance(&self, dt: Duration) {
        self.scripted.clock.advance(dt);
    }

    /// The sink frames are submitted through.
    #[must_use]
    pub fn sink(&self) -> &HeadlessSink {
        &self.sink
    }

    /// The window the UI runtime presents into.
    #[must_use]
    pub fn window(&self) -> &HeadlessWindow {
        &self.primary.window
    }

    /// The pull-model host the UI runtime's presentation speaks to, for a window
    /// built [`HeadlessWindow::with_text_store_host`].
    #[must_use]
    pub fn text_store_host(&self) -> Option<&Rc<RecordingTextStoreHost>> {
        self.primary.text_store_host.as_ref()
    }

    /// The scenario whose clock the host frames on and whose due changes
    /// each pump applies.
    #[must_use]
    pub fn scenario(&self) -> &Scenario {
        self.scripted.clock.scenario()
    }

    /// The clipboard the UI runtime hands its widgets.
    #[must_use]
    pub fn clipboard(&self) -> crate::clipboard::ScriptedClipboard {
        self.scripted.clipboard.clone()
    }

    /// The HTTP client the UI runtime hands its widgets: every request
    /// answers what the test scripted for its URL, and no request reaches the
    /// network. Registered only with this crate's `http` feature, as a
    /// runner registers HTTP only with its own.
    #[cfg(feature = "http")]
    #[must_use]
    pub fn http(&self) -> ScriptedHttp {
        self.scripted.http.clone()
    }

    /// The primary selection the UI runtime hands its widgets where the
    /// primary window's platform has one (Linux); elsewhere a widget is
    /// refused it, as on that platform, and what this double is told reaches
    /// nobody.
    #[must_use]
    pub fn primary_selection(&self) -> crate::primary_selection::ScriptedPrimarySelection {
        self.scripted.primary_selection.clone()
    }
}

/// A window of a [`HeadlessHost`]: its primary one or one it
/// [opened](HeadlessHost::open_window).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HeadlessWindowId(PresentationId);

/// The closes the windows' widgets asked for since the host's last turn, in
/// order, which the turn carries out as a runner's window queue does.
type CloseAsks = Rc<std::cell::RefCell<Vec<PresentationId>>>;

/// A window of a [`HeadlessHost`]: its presentation and what the host plays
/// for it.
#[derive(Clone)]
struct HostWindow {
    id: PresentationId,
    window: Rc<HeadlessWindow>,
    accessibility: Arc<HeadlessAccessibility>,
    text_store_host: Option<Rc<RecordingTextStoreHost>>,
}

/// The text a dropped-frame report raises with; `None` for a contained
/// recovery, which does not drop the frame.
fn dropped_frame_text(report: &FrameFailureReport) -> Option<String> {
    if report.disposition != FailureDisposition::FrameDropped {
        return None;
    }
    Some(match &report.kind {
        FrameFailureKind::SamplingPanic {
            first,
            blocking,
            message,
            ..
        } => match blocking {
            Some(blocking) => format!(
                "frame sampling panicked in {:?} ({:?}); publication blocked in {:?} ({:?}): {message}",
                first.stage, first.origin, blocking.stage, blocking.origin,
            ),
            None => format!(
                "frame sampling panicked in {:?} ({:?}): {message}",
                first.stage, first.origin,
            ),
        },
        FrameFailureKind::SegmentPanic { message, phase, .. } => {
            format!("frame segment panicked in {phase:?}: {message}")
        }
        FrameFailureKind::Pipeline { error } => format!("frame pipeline failed: {error}"),
        other => format!("frame dropped: {other:?}"),
    })
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::time::Duration;

    use super::{HeadlessHost, HeadlessWindow};

    /// A dropped-frame report the UI runtime made between pumps (from input
    /// delivery, say) is raised by the next pump, before it frames or moves
    /// the clock, and only once.
    ///
    /// Fails against a pump that clears the buffer before it runs: the
    /// report is erased unseen and the pump returns normally.
    #[test]
    fn a_report_made_between_pumps_is_raised_by_the_next_pump() {
        let mut ui_runtime = HeadlessHost::new(HeadlessWindow::new(40, 24));
        ui_runtime
            .failures
            .borrow_mut()
            .push("frame dropped: reported between pumps".to_owned());
        let before = flui_foundation::MonotonicClock::now(ui_runtime.clock());

        let raised = catch_unwind(AssertUnwindSafe(|| {
            ui_runtime.pump(Duration::from_millis(16))
        }))
        .expect_err("the leftover report is raised");

        assert_eq!(
            raised.downcast_ref::<String>().map(String::as_str),
            Some("frame dropped: reported between pumps")
        );
        assert_eq!(
            flui_foundation::MonotonicClock::now(ui_runtime.clock()),
            before,
            "the raising pump did not move the clock"
        );
        let _outcome = ui_runtime.pump(Duration::ZERO);
    }

    /// A hot reload renews the window's scene lineage within the pump whose
    /// frame then publishes it: the frame still takes the window's texture
    /// frames, from the registry its new lineage names.
    ///
    /// Fails against a sink that looks its registry up only by the lineages
    /// read before the pump: the frame publishes without its texture frame.
    #[test]
    fn a_frame_in_a_lineage_a_reload_renewed_takes_its_texture_frames() {
        let mut host = HeadlessHost::new(HeadlessWindow::new(100, 100));
        let registry = host
            .ui_runtime()
            .texture_registry(host.ui_runtime().presentation_id())
            .expect("the window's registry");
        let texture = registry.register().expect("a texture id");
        host.attach(&flui_widgets::Texture::new(texture.id()))
            .expect("root");
        let _ = host.pump(Duration::ZERO);
        let _ = host.pump(Duration::ZERO);
        let lineage = host.sink().version().expect("a version").id().lineage();

        texture.submit_frame(flui_painting::paint::Image::from_rgba8(
            1,
            1,
            vec![7, 7, 7, 255],
        ));
        host.ui_runtime()
            .command_sender()
            .request_hot_reload(flui_runtime::reload::ReloadTier::Reassemble)
            .expect("the inbox takes the reload");
        let _ = host.pump(Duration::ZERO);
        assert_ne!(
            host.sink().version().expect("a version").id().lineage(),
            lineage,
            "the reload renewed the lineage in this pump"
        );
        let taken: Vec<_> = host
            .sink()
            .textures()
            .frames
            .iter()
            .map(|(id, image)| (*id, image.data()[0]))
            .collect();
        assert_eq!(
            taken,
            [(texture.id(), 7)],
            "the frame rides the renewed lineage"
        );
    }

    /// The kept tree holds what an adapter holds: a node an incremental
    /// update detaches from the root is dropped, not kept forever.
    ///
    /// Fails against a fold that only upserts: the detached node stays.
    #[test]
    fn a_detached_node_leaves_the_published_tree() {
        use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};

        use super::PlatformAccessibility as _;

        let parent = |children: Vec<NodeId>| {
            let mut node = Node::new(Role::Window);
            node.set_children(children);
            node
        };
        let bridge = super::HeadlessAccessibility::default();
        bridge.publish(TreeUpdate {
            nodes: vec![
                (NodeId(1), parent(vec![NodeId(2)])),
                (NodeId(2), Node::new(Role::Label)),
            ],
            tree: Some(TreeInfo::new(NodeId(1))),
            tree_id: TreeId::ROOT,
            focus: NodeId(1),
        });
        bridge.publish(TreeUpdate {
            nodes: vec![
                (NodeId(1), parent(vec![NodeId(3)])),
                (NodeId(3), Node::new(Role::Label)),
            ],
            tree: None,
            tree_id: TreeId::ROOT,
            focus: NodeId(1),
        });

        let published = bridge.published.lock();
        let mut ids: Vec<_> = published
            .as_ref()
            .expect("a tree was published")
            .nodes
            .iter()
            .map(|(id, _)| *id)
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, [NodeId(1), NodeId(3)]);
    }
}
