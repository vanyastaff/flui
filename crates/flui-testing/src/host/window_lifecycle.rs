//! The windows of a [`HeadlessHost`]: opening, closing and moving them, and what a test
//! sets on them.

#[cfg(doc)]
use flui_runtime::owner_turn::CloseRequestRouter;

use std::rc::Rc;
use std::sync::Arc;

use std::time::Duration;

use flui_foundation::PresentationId;
use flui_foundation::geometry::DevicePixelRatio;
use flui_foundation::panic::FirstPanic;
use flui_platform::text::store::TextStoreHost;
use flui_platform::{DisplayId, PlatformWindow, WindowId};

use flui_runtime::owner_turn::CloseResponse;
use flui_runtime::presentation::PresentationWindow;

use flui_runtime::ui_runtime::UiRuntime;
use flui_semantics::platform::PlatformAccessibility;
use flui_view::CloseReason;

use super::accessibility::HeadlessAccessibility;
use super::errors::WindowSystemMismatch;
use super::window::HeadlessWindow;
use super::window_control::{install_window_control, register_close_requests};
use super::{HeadlessHost, HeadlessWindowId, HostWindow};
use crate::a11y::A11yTree;
use crate::text_store_host::RecordingTextStoreHost;

impl HeadlessHost {
    /// The recording text store backend of a particular presentation.
    #[must_use]
    pub fn text_store_host_on(
        &self,
        window: HeadlessWindowId,
    ) -> Option<&Rc<RecordingTextStoreHost>> {
        self.host_window(window).text_store_host.as_ref()
    }

    /// The window the UI runtime was built on.
    #[must_use]
    pub fn primary_window(&self) -> HeadlessWindowId {
        HeadlessWindowId(self.primary.id)
    }

    /// Ask to close `window` for `reason`, as its platform does when the user
    /// clicks its close button (or as the program or the session asks). Where
    /// the window's platform lets the application refuse that close
    /// ([`close_vetoes`](flui_runtime::owner_turn::close_vetoes)), it is put
    /// to the window's close guard and its close-request handler
    /// ([`HeadlessWindow::with_close_request_handler`]) in the order a
    /// runner's window asks them ([`CloseRequestRouter::consult`]: the guard
    /// first, the handler first at the session's end): a close either refuses is
    /// recorded, nothing is torn down, and the pump after the last hold is
    /// released carries it out. `true` when the window closed now; `false`
    /// when it was refused or the window is already gone.
    ///
    /// A [`CloseReason::Program`] close a guard recorded is, when it comes
    /// due, the window's consent to a quit ([`request_quit`](Self::request_quit)),
    /// as a runner treats it: it closes nothing alone.
    ///
    /// A window another one outlives is removed and the OS focus moves to a
    /// survivor; the last one retires the UI runtime, as on a real host
    /// ([`is_retired`](Self::is_retired)).
    pub fn request_close(&mut self, window: HeadlessWindowId, reason: CloseReason) -> bool {
        if !self.is_open(window) {
            return false;
        }
        if self.close_vetoes.contains(&reason) {
            let router = Rc::clone(&self.close_requests);
            if router.consult(self.address(window.0), reason) != CloseResponse::Close {
                return false;
            }
        }
        self.ui_runtime().commit_composition_for_close(window.0);
        self.close_presentation(window.0)
    }

    /// The address of presentation `id` of this host's UI runtime.
    pub(super) fn address(&self, id: PresentationId) -> flui_foundation::PresentationAddress {
        flui_foundation::PresentationAddress {
            ui_runtime_id: self.ui_runtime_id,
            presentation_id: id,
        }
    }

    /// Whether `window` has closed: at once by [`request_close`](Self::request_close),
    /// by a pump after its close's last hold was released or a decision
    /// closed it, or because it is gone. A close refused and still recorded
    /// is not one.
    #[must_use]
    pub fn is_closed(&self, window: HeadlessWindowId) -> bool {
        !self.is_open(window)
            || self
                .ui_runtime()
                .presentation_closing_for_test(window.0)
                .unwrap_or(true)
    }

    /// Whether `window` is one of the windows open now.
    pub(super) fn is_open(&self, window: HeadlessWindowId) -> bool {
        self.open.iter().any(|open| open.id == window.0)
    }

    /// Open `window` beside the existing ones, as a runner installs a
    /// presentation for a window the app opened: the UI runtime builds it a
    /// pipeline of its own at the window's scale factor. It shows nothing
    /// until a view is [attached](Self::attach_to) to it.
    ///
    /// The window gets a window identity no window of this host had, open
    /// or closed. It reports itself as it opened, focused and shown, as a
    /// native window does once installed: it takes the OS focus from the
    /// other windows, which forget their held keys, and the application's
    /// lifecycle moves with it.
    ///
    /// The window system is the host's, set by its primary window
    /// ([`HeadlessWindow::in_browser`]): a window that plays another is
    /// refused ([`try_open_window`](Self::try_open_window)). A window's
    /// [platform](HeadlessWindow::with_platform) names only its conventions.
    ///
    /// # Panics
    ///
    /// When `window` plays another window system than the host's; and resumes
    /// the panic of an observer that report reaches, with the window open.
    pub fn open_window(&mut self, window: HeadlessWindow) -> HeadlessWindowId {
        self.try_open_window(window)
            .unwrap_or_else(|refused| panic!("open_window: {refused}"))
    }

    /// [`open_window`](Self::open_window), refusing instead of panicking when
    /// `window` plays another window system than the host's.
    ///
    /// # Errors
    ///
    /// [`WindowSystemMismatch`] when `window` plays the browser and the host
    /// does not, or the other way round.
    ///
    /// # Panics
    ///
    /// Resumes the panic of an observer that report reaches, with the window
    /// open.
    pub fn try_open_window(
        &mut self,
        window: HeadlessWindow,
    ) -> Result<HeadlessWindowId, WindowSystemMismatch> {
        let host = self.primary.window.window_system();
        if window.in_browser != self.primary.window.in_browser {
            return Err(WindowSystemMismatch {
                host,
                window: window.window_system(),
            });
        }
        Ok(self.install_window(window))
    }

    /// Install `window` beside the open ones.
    fn install_window(&mut self, mut window: HeadlessWindow) -> HeadlessWindowId {
        window.id = self
            .window_ids
            .next()
            .expect("BUG: a test opens far fewer than u64::MAX windows");
        let text_store_host = window
            .text_store_host
            .map(RecordingTextStoreHost::with_selection_ui);
        let gesture_preset = window.gesture_preset.clone();
        let platform = window.platform;
        let close_handler = window.close_request_handler.take();
        let window = Rc::new(window);
        let accessibility = Arc::new(HeadlessAccessibility::default());
        let presentation = self.ui_runtime().assemble_presentation(
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
        );
        let id = self.ui_runtime_mut().install_presentation(presentation);
        install_window_control(self.ui_runtime(), id, &window, &self.close_asks);
        register_close_requests(
            &self.close_requests,
            &self.close_vetoes,
            self.ui_runtime(),
            id,
            &window,
            close_handler,
        );
        let seed = window.state();
        self.open.push(HostWindow {
            id,
            window,
            accessibility,
            text_store_host,
        });
        // The window reports itself as it opened, as a native host does once
        // it is installed: the change it makes to the application's lifecycle
        // is delivered in the owner's turn.
        self.observe(
            id,
            flui_runtime::owner::WindowObservation::Snapshot {
                execution: seed.execution,
                focused: seed.focus.is_focused(),
                visible: seed.is_seen(),
            },
        );
        HeadlessWindowId(id)
    }

    /// Attach `view` as the root widget of `window`, wrapped as
    /// [`attach`](Self::attach) wraps the primary window's.
    ///
    /// # Errors
    ///
    /// [`flui_view::__runtime::AttachError`] if a root is already attached.
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host or has closed.
    pub fn attach_to<V>(
        &self,
        window: HeadlessWindowId,
        view: &V,
    ) -> Result<(), flui_view::__runtime::AttachError>
    where
        V: flui_view::View + Clone + 'static,
    {
        // Every window's root is the same: the catalog's keyboard bindings,
        // then this window's own `MediaQuery`. A closed window panics here, with the host's own
        // message, before the runtime is asked for a presentation it may already have retired.
        let _open = self.open_window_of(window);
        self.ui_runtime()
            .attach_root_widget_to(window.0, &flui_widgets::app_root(view.clone()))
    }

    /// Attach `view` as the primary window's root, as [`attach`](Self::attach)
    /// does, taking the view by value: the root the host builds owns it, so the
    /// view need not be `Clone`.
    pub(crate) fn attach_root(
        &self,
        view: impl flui_view::IntoView,
    ) -> Result<(), flui_view::__runtime::AttachError> {
        let window = self.primary_window();
        let _open = self.open_window_of(window);
        self.ui_runtime()
            .attach_root_widget_to(window.0, &flui_widgets::app_root(view))
    }

    /// Unmount the primary window's root view, keeping the window and the UI
    /// runtime: every element below it is unmounted and every state disposed,
    /// in the runtime's scope, and a frame is asked for so the next
    /// [`pump`](Self::pump) presents the empty window. A retired host does
    /// nothing.
    ///
    /// # Panics
    ///
    /// With the first panic of a `dispose` or a destructor the unmount runs.
    pub(crate) fn unmount_root(&self) {
        let Some(runtime) = self.try_ui_runtime() else {
            return;
        };
        let window = self.primary_window();
        if self.is_closed(window) {
            return;
        }
        runtime.enter(|runtime| {
            runtime
                .presentation_widgets_for_test(window.0)
                .detach_root_widget();
        });
        runtime.request_redraw();
    }

    /// Hand `intent` to `window`'s URL owner, the outermost `Router` of its
    /// tree, as a runner hands it a URL the platform opened for that window
    /// (ADR-0330 §5). The Router's edit is built by the next
    /// [`pump`](Self::pump).
    ///
    /// # Errors
    ///
    /// What [`UiRuntime::navigate`] refuses with:
    /// [`NoPresentation`](flui_view::navigation::NavigationRefused::NoPresentation)
    /// for a window that has closed,
    /// [`NoUrlOwner`](flui_view::navigation::NavigationRefused::NoUrlOwner)
    /// for a tree with no `Router`, and the Router's own refusals.
    pub fn navigate(
        &self,
        window: HeadlessWindowId,
        intent: flui_view::navigation::NavigationIntent,
    ) -> Result<(), flui_view::navigation::NavigationRefused> {
        self.ui_runtime().navigate(window.0, intent)
    }

    /// Open `url` in `window`, as a runner opens a URL the platform hands
    /// that window: split, admitted by the declarations
    /// [`set_open_declarations`](Self::set_open_declarations) set, and
    /// applied to the window's `Router` as a `Go` (ADR-0330 §5). The Router's
    /// edit is built by the next [`pump`](Self::pump).
    ///
    /// # Errors
    ///
    /// What [`UiRuntime::open_url`] refuses with: a URL too long, malformed
    /// or not declared, and the navigation refusals of
    /// [`navigate`](Self::navigate).
    pub fn open_url(
        &self,
        window: HeadlessWindowId,
        url: &str,
    ) -> Result<(), flui_view::navigation::OpenUrlRefused> {
        self.ui_runtime().open_url(window.0, url)
    }

    /// Set the URL schemes and associated domains the application declares,
    /// which [`open_url`](Self::open_url) admits; a fresh host declares none.
    pub fn set_open_declarations(&self, declarations: flui_view::navigation::OpenDeclarations) {
        self.ui_runtime().set_open_declarations(declarations);
    }

    /// Run as a page of `origin` (`https://app.example.com`), as the web
    /// platform's runner sets it: [`open_url`](Self::open_url) admits that
    /// origin's URLs beside the declarations ([`UiRuntime::set_page_origin`]).
    ///
    /// # Errors
    ///
    /// What [`UiRuntime::set_page_origin`] refuses `origin` with.
    pub fn set_page_origin(
        &self,
        origin: &str,
    ) -> Result<(), flui_view::navigation::OpenUrlRefused> {
        self.ui_runtime().set_page_origin(origin)
    }

    /// Attach assistive technology to `window`; see
    /// [`enable_semantics`](Self::enable_semantics).
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host.
    pub fn enable_semantics_on(&self, window: HeadlessWindowId) {
        self.accessibility_of(window).attach();
    }

    /// Move `window` to a monitor of scale `scale_factor`, as a runner
    /// delivers the platform's scale change: the window keeps its logical
    /// size and reports the new scale, the primary window's surface takes
    /// the new device size, and the window's own presentation (never a
    /// sibling's) lays out, paints and publishes semantics at it from the
    /// next [`pump`](Self::pump).
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host, or its logical size at
    /// `scale_factor` is no drawable area (past `i32` device pixels, or below
    /// one); [`try_set_scale_factor`](Self::try_set_scale_factor) answers that
    /// instead.
    pub fn set_scale_factor(&mut self, window: HeadlessWindowId, scale_factor: DevicePixelRatio) {
        self.try_set_scale_factor(window, scale_factor).expect(
            "a scale the window's size has metrics at; try_set_scale_factor answers one it has not",
        );
    }

    /// [`set_scale_factor`](Self::set_scale_factor) for a scale that may make
    /// no metrics: the window changes only when it does.
    ///
    /// # Errors
    ///
    /// The [`InvalidMetrics`](flui_foundation::geometry::InvalidMetrics) the
    /// window's logical size at `scale_factor` makes: `Overflow` or
    /// `EmptyExtent` for a size that does not fit at it. Nothing changes.
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host.
    pub fn try_set_scale_factor(
        &mut self,
        window: HeadlessWindowId,
        scale_factor: DevicePixelRatio,
    ) -> Result<(), flui_foundation::geometry::InvalidMetrics> {
        // The runner's path: a metrics observation of the window, which a
        // recording sees as the `Metrics` row it is.
        let size = self.window_of(window).size;
        let metrics =
            flui_foundation::geometry::WindowMetrics::from_logical(size.get(), scale_factor)?;
        self.observe(
            window.0,
            flui_runtime::owner::WindowObservation::Metrics(metrics),
        );
        self.size_sink_to_primary();
        Ok(())
    }

    /// Move `window` to the display `display` (`None`: the backend can name none) at its
    /// current scale, as a runner delivers the platform's move: the window publishes a
    /// state on the new display, and the UI runtime reads it when the runner reports
    /// that the state changed. A move to the display the window is on publishes
    /// nothing.
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host.
    pub fn move_to_display(&mut self, window: HeadlessWindowId, display: Option<DisplayId>) {
        let native = self.window_of(window);
        if native.display.replace(display) == display {
            return;
        }
        native.advance_revision();
        self.observe(window.0, flui_runtime::owner::WindowObservation::State);
    }

    /// Deliver `display` as the display `window` is on, as a runner delivers
    /// the one it derived from the platform's display snapshot: its refresh
    /// period is what the UI runtime paces `window`'s presentation by. A
    /// window that closed hears nothing, as on a real host.
    pub fn set_window_display(
        &self,
        window: HeadlessWindowId,
        display: Option<flui_platform::WindowDisplay>,
    ) {
        self.observe(
            window.0,
            flui_runtime::owner::WindowObservation::Display(
                flui_runtime::owner::DisplayUpdate::new(display),
            ),
        );
    }

    /// Report that `window` gained OS focus, as a backend's focus callback
    /// does: keyboard input goes to its presentation from now on, whatever
    /// window it is stamped for, and the window that held focus loses it. A
    /// window that closed hears nothing, as on a real host.
    pub fn focus_window(&self, window: HeadlessWindowId) {
        self.observe(
            window.0,
            flui_runtime::owner::WindowObservation::Focus(true),
        );
    }

    /// The accessibility tree `window` last published to its platform
    /// adapter, every incremental update folded in; `None` before
    /// assistive technology attached and a [`pump`](Self::pump) ran.
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host.
    #[must_use]
    pub fn published_a11y_tree(&self, window: HeadlessWindowId) -> Option<A11yTree> {
        self.accessibility_of(window)
            .published
            .lock()
            .clone()
            .map(A11yTree::new)
    }

    /// Apply `values` as the next system-preference revision, as a host's
    /// preference source delivers one (ADR-0172): gesture timing, motion and
    /// text values reach the windows' new sequences from now on.
    pub fn set_system_preferences(&self, values: flui_platform::SystemPreferences) {
        self.ui_runtime()
            .enter(|ui_runtime| ui_runtime.apply_system_preferences_for_test(values));
    }

    /// Report the primary window shown (`true`) or hidden, minimized or fully
    /// occluded (`false`), as a backend's visibility callback does: the host
    /// delivers `WindowObservation::Visibility(visible)`, and the window's
    /// focus and execution state stay as they were.
    pub fn set_window_visible(&self, visible: bool) {
        self.observe(
            self.primary.id,
            flui_runtime::owner::WindowObservation::Visibility(visible),
        );
    }

    /// Report that the primary window gained (`true`) or lost OS focus, as a
    /// backend's focus callback does: the host delivers
    /// `WindowObservation::Focus(focused)`. A window that loses focus forgets
    /// its pressed keys and cancels its pointer sequences; the focus inside
    /// its tree stays where it was. [`focus_window`](Self::focus_window)
    /// moves the OS focus between windows this host opened.
    pub fn set_window_focused(&self, focused: bool) {
        self.observe(
            self.primary.id,
            flui_runtime::owner::WindowObservation::Focus(focused),
        );
    }

    /// Report whether the platform lets `window` run, as a phone's host
    /// reports its activity or scene stopping and starting (Android's
    /// `onStop`/`onStart`, a UIKit scene entering the background, a page
    /// frozen by the browser): the window's state says so, and the host
    /// delivers `WindowObservation::Execution(execution)`, which suspends
    /// (`Paused`) or resumes the window's lifecycle. Its focus and visibility
    /// stay as they were. A window that closed hears nothing, as on a real
    /// host.
    pub fn set_window_execution(
        &self,
        window: HeadlessWindowId,
        execution: flui_platform::WindowExecutionState,
    ) {
        let Some(open) = self.open.iter().find(|open| open.id == window.0) else {
            return;
        };
        open.window.execution.set(execution);
        self.observe(
            window.0,
            flui_runtime::owner::WindowObservation::Execution(execution),
        );
    }

    /// Report whether the window system keeps `window` off the screen
    /// ([`WindowState::occluded`](flui_platform::WindowState::occluded)), as a
    /// host reports a covering, a lock or a background app. As a host does on
    /// a change, the window's state says so at a new revision, the host
    /// delivers the new snapshot (`WindowObservation::State`) and, when it
    /// changes whether the window is seen, that too
    /// (`WindowObservation::Visibility`); the same value again changes and
    /// delivers nothing. Whether the occlusion may be a covering is the
    /// window's [`with_occlusion_reports`](HeadlessWindow::with_occlusion_reports).
    /// A window that closed hears nothing, as on a real host.
    pub fn set_window_occluded(&self, window: HeadlessWindowId, occluded: bool) {
        let Some(open) = self.open.iter().find(|open| open.id == window.0) else {
            return;
        };
        let was_seen = open.window.state().is_seen();
        if open.window.occluded.replace(occluded) == occluded {
            return;
        }
        open.window.advance_revision();
        let seen = open.window.state().is_seen();
        self.observe(window.0, flui_runtime::owner::WindowObservation::State);
        if seen != was_seen {
            self.observe(
                window.0,
                flui_runtime::owner::WindowObservation::Visibility(seen),
            );
        }
    }

    /// Deliver `notice` to every subscriber of the application, as a host
    /// delivers the system's notices: the machine about to sleep or waking,
    /// the session locked or unlocked, or the system asking for memory back.
    /// A session's end has its own entries
    /// ([`end_session`](Self::end_session)).
    ///
    /// A retired host has no subscriber left to tell.
    ///
    /// # Panics
    ///
    /// With the first panic of a subscriber, once every subscriber heard the
    /// notice.
    pub fn notify(&self, notice: flui_platform::SystemNotice) {
        if let Some(runtime) = self.try_ui_runtime() {
            runtime.deliver_host_notice(&flui_platform::HostNotice::from(notice));
        }
        self.ask_owed_frames();
    }

    /// Report `insets` as what covers `window`, as a host reports a change of
    /// its safe area, keyboard or system gesture regions: the presentation's
    /// `MediaQuery` carries them from the next [`pump`](Self::pump), and a
    /// moving keyboard is sampled at each frame's time until it settles. A
    /// window that closed hears nothing, as on a real host.
    pub fn set_window_insets(&self, window: HeadlessWindowId, insets: flui_platform::WindowInsets) {
        let Some(open) = self.open.iter().find(|open| open.id == window.0) else {
            return;
        };
        open.window.insets.set(insets);
        self.observe(
            window.0,
            flui_runtime::owner::WindowObservation::Insets(insets),
        );
    }

    /// Start moving `window`'s keyboard to `placement` now, over `duration`
    /// on `curve`, as a host reports a keyboard that begins to show, hide,
    /// undock or dock: the inset moves from where it is at this instant, and
    /// the safe area and system gesture regions stay as they were. Returns
    /// the keyboard as reported, whose [`at`](flui_platform::SoftKeyboard::at)
    /// names the inset each later frame lays out with.
    ///
    /// # Panics
    ///
    /// If `window` was not opened by this host, or the motion is invalid: a
    /// placement [`SoftKeyboard::try_moving`](flui_platform::SoftKeyboard::try_moving)
    /// refuses, a zero `duration`, or an invalid `curve`.
    pub fn move_keyboard(
        &self,
        window: HeadlessWindowId,
        placement: flui_platform::KeyboardPlacement,
        duration: Duration,
        curve: flui_platform::InsetCurve,
    ) -> flui_platform::SoftKeyboard {
        let now = self.event_now();
        let insets = self.window_of(window).insets.get();
        let motion =
            flui_platform::InsetMotion::try_new(insets.keyboard().at(now), now, duration, curve)
                .expect("a keyboard motion with a duration and a valid curve");
        let keyboard = flui_platform::SoftKeyboard::try_moving(placement, motion)
            .expect("a keyboard placement that is finite, not negative and not empty");
        self.set_window_insets(window, insets.with_keyboard(keyboard));
        keyboard
    }

    /// The presentation of the window `window` names.
    pub(super) fn presentation_of(&self, window: WindowId) -> PresentationId {
        std::iter::once(&self.primary)
            .chain(&self.open)
            .find(|open| open.window.id == window)
            .map_or_else(
                || panic!("the recording names {window:?}, a window this host has not opened"),
                |open| open.id,
            )
    }

    /// The surface of `presentation`'s window a recording names.
    pub(super) fn surface_of(
        &self,
        presentation: PresentationId,
        surface: flui_runtime::RecordedSurface,
    ) -> flui_interaction::ids::InputSurface {
        match surface {
            flui_runtime::RecordedSurface::Window => flui_interaction::ids::InputSurface::Window,
            flui_runtime::RecordedSurface::Embedded(raw) => self
                .embedded
                .iter()
                .copied()
                .find(|embedded| {
                    presentation == self.primary.id
                        && matches!(embedded, flui_interaction::ids::InputSurface::Embedded(id) if id.get() == raw)
                })
                .unwrap_or_else(|| {
                    panic!("the recording names embedded surface {raw}, which this host has not embedded")
                }),
        }
    }

    /// Close presentation `id`, as a runner closes a window: a window another
    /// one outlives is closed and removed, and the OS focus moves to a
    /// survivor; the last window stops its presentation and retires the UI
    /// runtime with it, as a real host retires its UI runtime when its last
    /// window closes, so whatever the UI runtime still held to answer ends as
    /// [`OwnerRetired`](flui_platform::capability::OwnerRetired). `false`
    /// when the window is already closed.
    ///
    /// # Panics
    ///
    /// With the first panic of the code the close runs (observers,
    /// disposers, destructors), once the close is done.
    pub(super) fn close_presentation(&mut self, id: PresentationId) -> bool {
        let Some(position) = self.open.iter().position(|window| window.id == id) else {
            return false;
        };
        let closed = self.open.remove(position);
        closed.window.closed.set(true);
        if let Some(exports) = &closed.window.portal_exports {
            exports.release_all();
        }
        // A window that closes no longer holds the session end it refused,
        // however the close came; its handler is dropped outside the
        // router's table. A quit it held learns it is gone at the next turn.
        let address = self.address(id);
        self.session_end.release_refusal(address);
        drop(self.close_requests.take(address));
        if !self.open.is_empty() {
            return self.ui_runtime_mut().close_presentation_entered(id);
        }
        let runtime = self
            .ui_runtime
            .take()
            .expect("BUG: a host with an open window holds its UI runtime");
        // The presentation stops inside the runtime's scope, so its Detached
        // observers run as on a real host, and the runtime is then dropped,
        // which retires what it still owes; a panic in either is kept and
        // the other still runs (ADR-0281).
        let mut first = FirstPanic::new();
        first.contain(|| runtime.enter(UiRuntime::stop_presentations));
        first.contain(move || drop(runtime));
        first.resume();
        true
    }

    /// Points the sink at each open presentation's texture registry, under
    /// its scene's current lineage.
    pub(super) fn track_textures(&mut self) {
        let Self {
            ui_runtime: Some(ui_runtime),
            sink,
            open,
            ..
        } = self
        else {
            return;
        };
        let ui_runtime = &*ui_runtime;
        sink.track_textures(open.iter().filter_map(|window| {
            let registry = ui_runtime.texture_registry(window.id)?;
            let totals = ui_runtime.presentation_scene_totals_for_test(window.id)?;
            Some((totals.version.lineage(), registry))
        }));
    }

    /// The presentations of the open windows, in opening order.
    pub(super) fn presentation_ids(&self) -> Vec<PresentationId> {
        self.open.iter().map(|window| window.id).collect()
    }

    /// The presentation at `position` of [`presentation_ids`](Self::presentation_ids)'s order.
    pub(super) fn presentation_at(&self, position: usize) -> Option<PresentationId> {
        self.open.get(position).map(|window| window.id)
    }

    /// The window `window` names: the primary one, open or closed, or one
    /// the host [opened](Self::open_window) and has not closed.
    ///
    /// # Panics
    /// When `window` names a secondary window that closed.
    #[must_use]
    pub fn window_of(&self, window: HeadlessWindowId) -> &HeadlessWindow {
        &self.host_window(window).window
    }

    fn accessibility_of(&self, window: HeadlessWindowId) -> &HeadlessAccessibility {
        &self.host_window(window).accessibility
    }

    /// The primary window, open or closed, or an open one.
    fn host_window(&self, window: HeadlessWindowId) -> &HostWindow {
        if window == self.primary_window() {
            return &self.primary;
        }
        self.open_window_of(window)
    }

    /// The open window `window` names.
    pub(super) fn open_window_of(&self, window: HeadlessWindowId) -> &HostWindow {
        self.open
            .iter()
            .find(|open| open.id == window.0)
            .expect("a HeadlessWindowId names a window this host opened and has not closed")
    }
}
