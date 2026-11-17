//! The per-window contract.
//!
//! [`PlatformWindow`] is what the framework holds of a window; every native
//! host implements it, and so does a test double. What a window delivers (its input, state changes, frame and
//! close requests, surface changes) goes to the [`WindowHandler`] it was
//! opened with (ADR-0254 §4). It names no OS or AccessKit type
//! (ADR-0082 §1): the accessibility bridge is reached through
//! `flui_native::HostWindow`, a host-side subtrait only the composition root
//! sees.
//!
//! [`WindowHandler`]: crate::WindowHandler

use std::sync::Arc;

use flui_foundation::geometry::Point;

use crate::{
    AdmittedCommand, ApplyError, Cursor, CursorError, PlatformHaptics, PlatformTextInput,
    RedrawRequester, SurfaceTarget, Unsupported, WindowId, WindowState,
};

/// What lies under a point given in one window's coordinates, as
/// [`PlatformWindow::window_at`] answers.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum WindowAt {
    /// The content of a window of the same platform instance is topmost there:
    /// `window` (the asking window itself included) at `position`, in that
    /// window's logical pixels.
    Window {
        /// The window whose content is under the point.
        window: WindowId,
        /// The point in that window's logical pixels.
        position: Point<f64>,
    },
    /// No window content of this platform instance is under the point: the
    /// desktop, another application's window, a window frame, or a point no
    /// display can hold.
    Outside,
    /// The backend cannot place its windows on one desktop (Wayland, the web,
    /// mobile), so it cannot tell.
    Unknown,
}

/// One window of the platform, as the framework programs against it: its
/// identity, its [`WindowState`] snapshot and the [`WindowCommand`]s it takes,
/// the per-frame verbs (redraw, input regions, the refresh-rate hint, the
/// cursor) and its facets (text input, haptics, system chrome, the OS drag
/// source). Every native host implements it, and so does a test double.
///
/// [`WindowCommand`]: crate::WindowCommand
///
/// # What a window delivers
///
/// A window has no registrations. Its input, state changes, frame requests,
/// close requests, close and surface changes go to the
/// [`WindowHandler`](crate::WindowHandler) passed when it was opened, on the
/// owner thread, outside any lock the window holds (ADR-0254 §4).
///
/// # Thread affinity
///
/// A window is neither `Send` nor `Sync` (ADR-0254 §5): it lives on the thread
/// that owns the platform event loop, and every method is called there,
/// because no other thread can hold one. The thread that presents frames gets
/// the window's [`SurfaceTarget`] from [`surface_target`](Self::surface_target)
/// instead, which is `Send + Sync`.
///
/// A worker that needs the owner to act reaches it through
/// `flui_native::PlatformProxy`, the recorded cross-thread-to-owner lane
/// (ADR-0039 §3). Support is operation-specific: signals can support `wake`
/// and `request_quit` while refusing deferred `open_window` with
/// `flui_native::ProxySendError::Unsupported`. Direct backends create windows
/// through their owner capability; browser signals follow ADR-0196.
///
/// A window cannot be sent to another thread:
///
/// ```compile_fail,E0277
/// fn send(window: std::rc::Rc<dyn flui_platform::PlatformWindow>) {
///     std::thread::spawn(move || window.request_redraw());
/// }
/// ```
///
/// # One source of truth
///
/// Every fact of the window — its metrics and scale, focus, visibility, hover,
/// theme, display, safe area, execution state and capabilities — is a
/// field of the [`WindowState`] snapshot (ADR-0254 §1), and a change of any of
/// them is one [`WindowHandler::state_changed`](crate::WindowHandler::state_changed). There is no getter
/// and no callback per fact: a consumer that needs to know which fact changed
/// compares the new snapshot with the one it saw last. Showing, activating and
/// closing a window are [`WindowCommand`](crate::WindowCommand)s like every
/// other change.
///
/// # A window built from this crate alone
///
/// ```
/// use std::sync::Arc;
///
/// use flui_foundation::geometry::Point;
/// use flui_platform::input_regions::InputRegion;
/// use flui_platform::{
///     AdmissionError, AdmittedCommand, ApplyError, Cursor, CursorError, FrameRateHint,
///     GestureGeometry, PlatformHaptics, PlatformSystemChrome, PlatformTextInput, PlatformWindow,
///     PreferenceQueryError, RedrawRequester, SurfaceTarget, SwapChainStatistics, Unsupported,
///     UnsupportedReason, WindowAt, WindowCapabilities, WindowCommand, WindowExtent,
///     WindowGeometry, WindowId, WindowIdMint, WindowMaterial, WindowMode, WindowState,
/// };
/// use raw_window_handle::{
///     DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
/// };
///
/// /// It draws into no native surface, so it hands out none.
/// struct NoSurface;
///
/// impl HasWindowHandle for NoSurface {
///     fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
///         Err(HandleError::Unavailable)
///     }
/// }
/// impl HasDisplayHandle for NoSurface {
///     fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
///         Err(HandleError::Unavailable)
///     }
/// }
/// impl SurfaceTarget for NoSurface {
///     fn pre_present_notify(
///         &self,
///         _: Option<flui_foundation::FrameStamp>,
///         _: SwapChainStatistics,
///     ) {
///     }
///     fn keep_past_close(&self) {}
/// }
///
/// struct Offscreen {
///     id: WindowId,
///     /// What it honours, published once when it opened.
///     capabilities: WindowCapabilities,
/// }
///
/// impl PlatformWindow for Offscreen {
///     fn id(&self) -> WindowId {
///         self.id
///     }
///     // No window system, so no gesture geometry of its own.
///     fn gesture_geometry(&self) -> Result<Option<GestureGeometry>, PreferenceQueryError> {
///         Ok(None)
///     }
///     fn request_redraw(&self) {}
///     fn redraw_requester(&self) -> Arc<dyn RedrawRequester> {
///         Arc::new(|| {})
///     }
///     fn surface_target(&self) -> Arc<dyn SurfaceTarget> {
///         Arc::new(NoSurface)
///     }
///     fn state(&self) -> WindowState {
///         WindowState::new(WindowGeometry::EMPTY, WindowExtent::ZERO)
///             .with_capabilities(self.capabilities)
///     }
///     fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
///         // Under its own published capabilities, which honour nothing
///         // but the normal mode every window has.
///         command.against(&self.capabilities)?;
///         Ok(())
///     }
///     fn set_cursor(&self, _cursor: &Cursor) -> Result<(), CursorError> {
///         Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform).into())
///     }
///     fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
///         None
///     }
///     fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>> {
///         None
///     }
///     fn system_chrome(&self) -> Option<Arc<dyn PlatformSystemChrome>> {
///         None
///     }
///     fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
///         None
///     }
///     // It shares no desktop with other windows.
///     fn window_at(&self, _position: Point<f64>) -> WindowAt {
///         WindowAt::Unknown
///     }
///     // It is on no screen, so there is nothing to reveal.
///     fn reveal_after_first_frame(&self) {}
///     // Its capabilities leave every verb `Unavailable`, so each answers so.
///     fn set_frame_rate_hint(&self, _hint: FrameRateHint) -> Result<(), Unsupported> {
///         Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform))
///     }
///     fn set_input_regions(&self, _regions: &[InputRegion]) -> Result<(), Unsupported> {
///         Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform))
///     }
/// }
///
/// // The id comes from the mint of whoever opened the window.
/// let id = WindowIdMint::new().next().expect("a fresh mint issues an id");
/// let capabilities = WindowCapabilities::NONE
///     .publish()
///     .expect("the process has publications left");
/// let window: &dyn PlatformWindow = &Offscreen { id, capabilities };
/// assert_eq!(window.id(), id);
/// let capabilities = window.state().capabilities;
/// assert!(matches!(
///     WindowCommand::Show.admit(&capabilities),
///     Err(AdmissionError::Unsupported(_))
/// ));
/// assert!(matches!(
///     WindowCommand::SetMaterial(WindowMaterial::System).admit(&capabilities),
///     Err(AdmissionError::Unsupported(_))
/// ));
/// assert!(window.set_frame_rate_hint(FrameRateHint::High).is_err());
/// let normal = WindowCommand::SetMode(WindowMode::Normal)
///     .admit(&capabilities)
///     .expect("every window has the normal mode");
/// assert_eq!(window.apply(normal), Ok(()));
/// // Capabilities built by hand are no window's publication: nothing admits
/// // against them.
/// assert_eq!(
///     WindowCommand::SetMode(WindowMode::Normal).admit(&WindowCapabilities::ALL),
///     Err(AdmissionError::Unpublished)
/// );
/// // A window with no native surface hands out none.
/// let target = window.surface_target();
/// assert!(raw_window_handle::HasWindowHandle::window_handle(&*target).is_err());
/// ```
///
/// A window takes only what [`WindowCommand::admit`](crate::WindowCommand::admit) built, and nothing but
/// `admit` builds it:
///
/// ```compile_fail,E0308
/// fn show(window: &dyn flui_platform::PlatformWindow) {
///     let _ = window.apply(flui_platform::WindowCommand::Show);
/// }
/// ```
///
/// A fact is read from the snapshot, never asked of the window on its own:
///
/// ```compile_fail,E0599
/// fn focused(window: &dyn flui_platform::PlatformWindow) -> bool {
///     window.is_focused()
/// }
/// ```
///
/// The accessibility bridge is not part of this contract; a window that
/// tries to supply one here does not compile:
///
/// ```compile_fail,E0407
/// use std::sync::Arc;
///
/// use flui_foundation::geometry::Point;
/// use flui_platform::input_regions::InputRegion;
/// use flui_platform::{
///     AdmittedCommand, ApplyError, Cursor, CursorError, FrameRateHint, GestureGeometry,
///     PlatformHaptics, PlatformSystemChrome, PlatformTextInput, PlatformWindow,
///     PreferenceQueryError, RedrawRequester, SurfaceTarget, Unsupported, UnsupportedReason,
///     WindowAt, WindowExtent, WindowGeometry, WindowId, WindowState,
/// };
///
/// struct Offscreen(WindowId);
///
/// impl PlatformWindow for Offscreen {
///     fn id(&self) -> WindowId {
///         self.0
///     }
///     fn gesture_geometry(&self) -> Result<Option<GestureGeometry>, PreferenceQueryError> {
///         Ok(None)
///     }
///     fn request_redraw(&self) {}
///     fn redraw_requester(&self) -> Arc<dyn RedrawRequester> {
///         Arc::new(|| {})
///     }
///     fn surface_target(&self) -> Arc<dyn SurfaceTarget> {
///         unimplemented!("the window never presents")
///     }
///     fn window_at(&self, _position: Point<f64>) -> WindowAt {
///         WindowAt::Unknown
///     }
///     fn reveal_after_first_frame(&self) {}
///     fn state(&self) -> WindowState {
///         WindowState::new(WindowGeometry::EMPTY, WindowExtent::ZERO)
///     }
///     fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
///         // Under the capabilities of its own snapshot, the one place a
///         // window reads what it honours.
///         command.against(&self.state().capabilities)?;
///         Ok(())
///     }
///     fn set_cursor(&self, _cursor: &Cursor) -> Result<(), CursorError> {
///         Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform).into())
///     }
///     fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
///         None
///     }
///     fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>> {
///         None
///     }
///     fn system_chrome(&self) -> Option<Arc<dyn PlatformSystemChrome>> {
///         None
///     }
///     fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
///         None
///     }
///     fn set_frame_rate_hint(&self, _hint: FrameRateHint) -> Result<(), Unsupported> {
///         Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform))
///     }
///     fn set_input_regions(&self, _regions: &[InputRegion]) -> Result<(), Unsupported> {
///         Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform))
///     }
///     fn accessibility(&self) -> Option<()> { None }
/// }
/// ```
///
/// Nor is the downcast to a host's own window: `flui_native::HostWindow`,
/// which only the core host implements, carries it (ADR-0151 §8), so a window
/// the framework programs against is never turned into a concrete type:
///
/// ```compile_fail,E0599
/// fn downcast(window: &dyn flui_platform::PlatformWindow) {
///     let _ = window.as_any();
/// }
/// ```
pub trait PlatformWindow {
    /// This window's platform-internal identity.
    ///
    /// A window that cannot state its identity cannot be demultiplexed
    /// (ADR-0037 §2): the identity is what lets the demux boundary look up
    /// which `(UiRuntimeId, PresentationId)` a native event belongs to. Every
    /// implementor must return a real, stable-for-the-window's-lifetime
    /// value — never a shared sentinel that would make two different
    /// windows compare equal.
    fn id(&self) -> WindowId;

    /// The system's gesture distances and speeds projected into this window's
    /// coordinates: the one query a window answers beside
    /// [`state`](Self::state). It is not a fact of the window but a system
    /// preference read at the window's scale, which can fail and be retried,
    /// so it is not a field of the snapshot.
    ///
    /// Call on the platform owner thread after accepting system-preference or DPI
    /// changes. This query shares the host's observation lifetime; it installs no
    /// additional native subscription. `Ok(None)`: the window system has no
    /// such geometry, and the consumer keeps its own defaults. Every
    /// implementation says which; there is no default.
    ///
    /// # Errors
    /// A closed owner, wrong thread, native failure or invalid projection fails
    /// the query. Preserve the last accepted projection and arrange a bounded
    /// retry instead of replacing it with unknown values.
    fn gesture_geometry(
        &self,
    ) -> Result<Option<crate::GestureGeometry>, crate::PreferenceQueryError>;

    /// Request that this window produce a frame: the handler's
    /// [`frame_requested`](crate::WindowHandler::frame_requested) runs on a
    /// later owner turn.
    ///
    /// The raster side never calls it (ADR-0045 decision 5). A thread other
    /// than the owner that wants a frame asks through the window's
    /// [`redraw_requester`](Self::redraw_requester).
    fn request_redraw(&self);

    /// What asks this window for a frame from any thread: the same request as
    /// [`request_redraw`](Self::request_redraw), carried to the owner where
    /// the window system needs that. It does not keep the window open.
    fn redraw_requester(&self) -> Arc<dyn RedrawRequester>;

    /// The window's surface, as the thread that presents its frames holds it
    /// (ADR-0254 §5).
    ///
    /// Every call returns a target for the same native surface. It answers
    /// [`HandleError::Unavailable`](raw_window_handle::HandleError::Unavailable)
    /// once this window is gone, and does not keep the window open. Required,
    /// with no default: a window with no native surface hands out a target
    /// whose handle queries answer `Unavailable` (a test double's is
    /// `flui_platform::testing::NoNativeSurface`, under the `testing`
    /// feature).
    fn surface_target(&self) -> Arc<dyn SurfaceTarget>;

    // ==================== State and commands (ADR-0237) ====================

    /// The window's state as the operating system reports it now: every fact
    /// of the window, its [`WindowCapabilities`](crate::WindowCapabilities)
    /// included (ADR-0254 §1).
    ///
    /// An immutable snapshot with a [`WindowRevision`](crate::WindowRevision):
    /// the revision grows when something differs from the previous call and
    /// stays when nothing does. The snapshot describes what the OS did, not
    /// what a command asked for. Call it on the owner thread.
    ///
    /// **A read runs no handler.** A window that finds its window system
    /// ahead of the snapshot it published answers with the new one, and
    /// delivers the [`state_changed`](crate::WindowHandler::state_changed) it
    /// owes at the owner's turn, never from inside this call: the caller may
    /// hold a borrow the handler takes again (ADR-0254 §4).
    ///
    /// Required, with no default: a window that cannot publish a state of its
    /// own says so with a snapshot built from what it knows, not by inheriting
    /// one that would never change.
    fn state(&self) -> WindowState;

    /// Request a change to the window.
    ///
    /// Accepted means requested: the outcome is visible only in the next
    /// [`state`](Self::state), because the OS may clamp, delay or refuse it.
    /// Call it on the owner thread. The command was admitted
    /// ([`WindowCommand::admit`](crate::WindowCommand::admit)), so its numbers
    /// describe a window and the capabilities it was checked against honour
    /// it: an implementation has no refusal of its own to remember. It opens
    /// the token with [`AdmittedCommand::against`] the capabilities of its
    /// last published snapshot, which answers [`ApplyError::Superseded`] when
    /// they changed since admission.
    ///
    /// [`WindowCommand::Close`](crate::WindowCommand::Close) is a decision already made, not a
    /// request: it asks no veto
    /// ([`WindowHandler::close_requested`](crate::WindowHandler::close_requested))
    /// and runs the same teardown a user's close takes — the handler's
    /// [`closed`](crate::WindowHandler::closed), removal from the backend's window
    /// tracking, cleanup of per-window input state, and the exit-policy consult
    /// that ends the loop when this was the last window. Win32, X11 and the headless
    /// double run that teardown before `apply` returns. A window that has
    /// already closed answers
    /// [`ApplyError::Closed`].
    ///
    /// # Errors
    /// [`ApplyError`]: the window closed, its capabilities moved since
    /// admission, or the operating system refused or failed the request.
    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError>;

    /// This window's IME text-input capability: the receiver of the
    /// requests a field makes of the input method (whether it may compose,
    /// where its caret is). `None` for a window whose input method reads the
    /// field's store instead (the pull model, ADR-0135: a Win32 window with
    /// TSF), which is how text input is taken there and not an absence, or
    /// that has no input method. Every implementation says which; there is
    /// no default (ADR-0151 §5).
    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>>;

    /// This window's haptic feedback capability (see [`PlatformHaptics`]'s
    /// module doc for why it is per window).
    ///
    /// Absence of haptics is the actuator's to say: a window of a platform or
    /// device with no actuator hands out one that answers
    /// [`HapticOutcome::Unsupported`](crate::haptics::HapticOutcome::Unsupported)
    /// with its reason, and every window of the core host hands one out. The
    /// `Option` is the facet's, not the capability's: a facet beyond the
    /// required core is reached through one (ADR-0151 §5), and `None` says
    /// the window has no haptics facet at all, as a window built outside the
    /// hosts may not. Every implementation says which; there is no default.
    fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>>;

    /// This window's system-chrome capability (the system bars' overlay
    /// style). A window of a platform that draws no system bars over its
    /// content hands out one that styles nothing, as every window of the core
    /// host hands one out; `None` says the window has no such facet at all
    /// (ADR-0151 §5), as a window built outside the hosts may not. Every
    /// implementation says which; there is no default.
    fn system_chrome(&self) -> Option<Arc<dyn crate::PlatformSystemChrome>>;

    /// Get this window's OS drag source, if the backend can start OS drag
    /// sessions (ADR-0211). `None` for backends without one; their
    /// [`InputCapabilities::outgoing_drag`](crate::input_capabilities::InputCapabilities::outgoing_drag)
    /// says `Unavailable`, and a drag that needs the OS stays in-window.
    /// Every implementation says which; there is no default.
    fn drag_source(&self) -> Option<Arc<dyn crate::drag::PlatformDragSource>>;

    /// Which window's content is topmost under `position`, given in this
    /// window's logical pixels and possibly outside it, honouring the stacking
    /// order of every window on the desktop. A drag on this window's pointer
    /// capture uses it to reach the other windows of its process without an OS
    /// session (ADR-0211 §2).
    ///
    /// Owner thread only. A backend that cannot place its windows on one
    /// desktop answers [`WindowAt::Unknown`]. Every implementation says which;
    /// there is no default.
    fn window_at(&self, position: Point<f64>) -> WindowAt;

    /// Hand the OS this window's input regions, in window-logical pixels (ADR-0217 §5).
    ///
    /// The framework calls it on every composited frame with the window's whole set,
    /// whether or not it changed. The backend projects the set to device pixels with the
    /// window's current scale, each rectangle rounded outward, and updates
    /// the OS only when the projection, the scale or the display changed, so no caller-side
    /// comparison can hide a scale or display change from it.
    ///
    /// A scale change or a move to another display need not composite a frame, so the
    /// framework also re-sends the last set when the presentation's pixel ratio changes
    /// when a published [`state`](Self::state) puts the window somewhere else: another
    /// position, display or frame. A backend whose window can change displays at an equal
    /// scale must publish that state and call its handler's
    /// [`state_changed`](crate::WindowHandler::state_changed),
    /// or the OS keeps the previous display's projection until the next composited frame.
    ///
    /// The set is in paint order, and a backend keeps that order: a region later in it lies
    /// over the ones before it, which is how
    /// [`chrome_hit`](crate::input_regions::chrome_hit) reads move areas, the content cut out
    /// of them and the caption buttons. A backend keeps the regions of the kinds its window
    /// honours ([`InputCapabilities::input_region`](crate::input_capabilities::InputCapabilities::input_region))
    /// and drops the rest.
    ///
    /// It is a verb, not a [`WindowCommand`](crate::WindowCommand): commands apply at the
    /// owner's next turn, and this is sent every frame (ADR-0254 §3). Owner thread only.
    ///
    /// # Errors
    ///
    /// [`Unsupported`] exactly when the window honours no kind of input region
    /// ([`InputCapabilities::honours_input_regions`](crate::input_capabilities::InputCapabilities::honours_input_regions)
    /// is `false`), and the set is not kept.
    fn set_input_regions(
        &self,
        regions: &[crate::input_regions::InputRegion],
    ) -> Result<(), Unsupported>;

    // ==================== Control ====================

    /// The embedder has presented the first frame into this window's
    /// surface — or has waited as long as it is willing to for one.
    ///
    /// A backend that defers the physical reveal of a window opened
    /// [`WindowOptions::visible`](crate::WindowOptions::visible)
    /// `== true` performs it now, exactly once; a window opened hidden, one
    /// already shown explicitly ([`WindowCommand::Show`](crate::WindowCommand::Show) or
    /// [`WindowCommand::Activate`](crate::WindowCommand::Activate)),
    /// or a backend that reveals at open, ignores the call. Calling it
    /// again is harmless.
    ///
    /// The embedder calls this after the first frame whose present
    /// succeeded, and at a bounded fallback after a frame that ran and
    /// presented nothing, so a surface that never presents still yields a
    /// window the user can see and close rather than a process with no
    /// window at all.
    ///
    /// Every implementation says what it does; there is no default, so a
    /// backend that defers its reveal cannot forget to perform it and leave
    /// its window unseen:
    ///
    /// ```compile_fail,E0046
    /// use std::sync::Arc;
    ///
    /// use flui_platform::input_regions::InputRegion;
    /// use flui_platform::*;
    ///
    /// struct Deferred(WindowId);
    ///
    /// impl PlatformWindow for Deferred {
    ///     fn id(&self) -> WindowId { self.0 }
    ///     fn gesture_geometry(&self) -> Result<Option<GestureGeometry>, PreferenceQueryError> {
    ///         Ok(None)
    ///     }
    ///     fn request_redraw(&self) {}
    ///     fn redraw_requester(&self) -> Arc<dyn RedrawRequester> { Arc::new(|| {}) }
    ///     fn surface_target(&self) -> Arc<dyn SurfaceTarget> { unimplemented!() }
    ///     fn state(&self) -> WindowState {
    ///         WindowState::new(WindowGeometry::EMPTY, WindowExtent::ZERO)
    ///     }
    ///     fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
    ///         command.against(&self.state().capabilities)?;
    ///         Ok(())
    ///     }
    ///     fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> { None }
    ///     fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>> { None }
    ///     fn system_chrome(&self) -> Option<Arc<dyn PlatformSystemChrome>> { None }
    ///     fn drag_source(&self) -> Option<Arc<dyn drag::PlatformDragSource>> { None }
    ///     fn window_at(&self, _: flui_foundation::geometry::Point<f64>) -> WindowAt {
    ///         WindowAt::Unknown
    ///     }
    ///     fn set_input_regions(&self, _: &[InputRegion]) -> Result<(), Unsupported> {
    ///         Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform))
    ///     }
    ///     fn set_cursor(&self, _: &Cursor) -> Result<(), CursorError> { Ok(()) }
    ///     fn set_frame_rate_hint(&self, _: FrameRateHint) -> Result<(), Unsupported> {
    ///         Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform))
    ///     }
    ///     // No `reveal_after_first_frame`.
    /// }
    /// ```
    fn reveal_after_first_frame(&self);

    /// Show `cursor` while the pointer is over this window: the cursor its
    /// presentation selected for the hovered region, a system icon, an image
    /// of the application's, or none.
    ///
    /// This is deliberately window-scoped: a process-global cursor setter
    /// cannot identify which of several presentations owns the hovered region.
    /// A hidden cursor is hidden over this window alone and comes back with
    /// the next cursor the window is given. A verb, sent when the hovered
    /// region asks for another cursor, not a
    /// [`WindowCommand`](crate::WindowCommand) (ADR-0254 §3). Owner thread
    /// only.
    ///
    /// # Errors
    ///
    /// Returns [`CursorError::Unsupported`] when this window backend has no
    /// pointer-cursor facility, or for a [`Cursor::Image`] exactly when the
    /// window's [`WindowCapabilities::cursor_image`](crate::WindowCapabilities::cursor_image)
    /// is `Unavailable`; [`CursorError::Platform`] when the window system
    /// refuses or fails the update.
    fn set_cursor(&self, cursor: &Cursor) -> Result<(), CursorError>;

    /// Tell the system which refresh rate this window wants from its display.
    ///
    /// [`FrameRateHint::High`](crate::FrameRateHint::High) asks for the display's highest rate while the
    /// window animates or is touched; [`FrameRateHint::Default`](crate::FrameRateHint::Default) withdraws the
    /// request and leaves the choice to the system. It is a request, not a
    /// command: the system may keep any rate. A native surface carries the
    /// request, so a host sends the current hint again after the surface was
    /// replaced.
    ///
    /// A verb, not a [`WindowCommand`](crate::WindowCommand): commands apply at
    /// the owner's next turn, and this is sent as the window's animation
    /// starts and stops (ADR-0254 §3). Called on the owner thread.
    ///
    /// # Errors
    ///
    /// [`Unsupported`] exactly when the window's
    /// [`WindowCapabilities::frame_rate_hint`](crate::WindowCapabilities::frame_rate_hint)
    /// is `Unavailable`: the window system takes no such request, or this
    /// operating system build lacks it ([`NotOnThisDevice`](crate::UnsupportedReason::NotOnThisDevice)).
    fn set_frame_rate_hint(&self, hint: crate::FrameRateHint) -> Result<(), Unsupported>;

    /// Name this window to a desktop portal for one request: answer `answer`
    /// with the [`PortalParent`](crate::capability::PortalParent) the request
    /// passes as `parent_window` and holds until it ends (ADR-0363).
    ///
    /// An X11 window answers `x11:<XID>` at once. A Wayland window exports
    /// itself for the request (`xdg-foreign`) and answers `wayland:<handle>`
    /// once the compositor named the export, which the answer's drop
    /// releases; a compositor with no exporter answers no window. Called on
    /// the owner thread, through
    /// [`WindowRef::portal_parent`](crate::capability::WindowRef::portal_parent),
    /// only while the window is open.
    ///
    /// The default answers [`PortalParent::none`](crate::capability::PortalParent::none):
    /// only a Linux desktop has portals, so a window of any other system has
    /// no name a portal takes.
    fn portal_parent(&self, answer: crate::capability::Resolver<crate::capability::PortalParent>) {
        // The owner retiring first leaves nothing waiting for the answer.
        let _ = answer.resolve(crate::capability::PortalParent::none());
    }
}
