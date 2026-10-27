//! The per-window contract.
//!
//! A thin abstraction over platform windows for testability and
//! flexibility. What a window delivers (its input, state changes, frame and
//! close requests, surface changes) goes to the [`WindowHandler`] it was
//! opened with (ADR-0254 §4). It names no OS, winit or AccessKit type
//! (ADR-0082 §1): the accessibility bridge is reached through
//! `flui_native::HostWindow`, a host-side subtrait only the composition root
//! sees.
//!
//! [`WindowHandler`]: crate::WindowHandler

use std::sync::Arc;

use cursor_icon::CursorIcon;
use flui_foundation::geometry::Point;

use crate::{
    AdmittedCommand, ApplyError, CursorError, PlatformHaptics, PlatformTextInput, Unsupported,
    WindowId, WindowState,
};

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

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

/// Trait for platform window abstraction
///
/// Provides a minimal interface for window operations, enabling
/// testing and future flexibility (e.g., headless rendering).
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
/// `Send + Sync` on this trait is what lets a backend store a window and a
/// wake path carry one across threads. It is not permission to drive a window
/// from any thread. **The default for every method that drives a window —
/// anything reaching a native windowing API — is: call it on the thread that
/// owns the platform event loop.** A method callable from elsewhere says so on
/// itself; silence on the rest is this rule applying, not an omission.
///
/// A worker that needs the owner to act reaches it through
/// `flui_native::PlatformProxy`, the recorded cross-thread-to-owner lane
/// (ADR-0039 §3). Support is operation-specific: signals can support `wake`
/// and `request_quit` while refusing deferred `open_window` with
/// `flui_native::ProxySendError::Unsupported`. The winit lane also transports
/// deferred window creation. Direct backends create windows through their
/// owner capability; browser signals follow ADR-0196.
///
/// How each backend enforces this default is recorded in `flui-native`'s
/// `ARCHITECTURE.md`.
///
/// # One source of truth
///
/// Every fact of the window — its metrics and scale, focus, visibility, hover,
/// appearance, display, safe area, execution state and capabilities — is a
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
/// use flui_platform::input_regions::InputRegion;
/// use flui_platform::{
///     AdmissionError, AdmittedCommand, ApplyError, CursorError, CursorIcon, FrameRateHint,
///     PlatformHaptics, PlatformSystemChrome, PlatformTextInput, PlatformWindow, Unsupported,
///     UnsupportedReason, WindowCapabilities, WindowCommand, WindowGeometry, WindowId,
///     WindowMaterial, WindowMode, WindowState,
/// };
/// use flui_foundation::geometry::Size;
///
/// struct Offscreen;
///
/// impl PlatformWindow for Offscreen {
///     fn id(&self) -> WindowId {
///         WindowId(7)
///     }
///     fn request_redraw(&self) {}
///     fn state(&self) -> WindowState {
///         WindowState::new(WindowGeometry::new(None, Size::default()), Size::default())
///     }
///     fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
///         // Under the capabilities of its own snapshot, which honour
///         // nothing but the normal mode every window has.
///         command.against(&WindowCapabilities::NONE)?;
///         Ok(())
///     }
///     fn set_cursor(&self, _cursor: CursorIcon) -> Result<(), CursorError> {
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
///     // Its capabilities leave every verb `Unavailable`, so each answers so.
///     fn set_frame_rate_hint(&self, _hint: FrameRateHint) -> Result<(), Unsupported> {
///         Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform))
///     }
///     fn set_input_regions(&self, _regions: &[InputRegion]) -> Result<(), Unsupported> {
///         Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform))
///     }
/// }
///
/// let window: &dyn PlatformWindow = &Offscreen;
/// assert_eq!(window.id(), WindowId(7));
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
/// use flui_platform::input_regions::InputRegion;
/// use flui_platform::{
///     AdmissionError, AdmittedCommand, ApplyError, CursorError, CursorIcon, FrameRateHint,
///     PlatformHaptics, PlatformSystemChrome, PlatformTextInput, PlatformWindow, Unsupported,
///     UnsupportedReason, WindowCapabilities, WindowCommand, WindowGeometry, WindowId,
///     WindowMaterial, WindowMode, WindowState,
/// };
/// use flui_foundation::geometry::Size;
///
/// struct Offscreen;
///
/// impl PlatformWindow for Offscreen {
///     fn id(&self) -> WindowId {
///         WindowId(7)
///     }
///     fn request_redraw(&self) {}
///     fn state(&self) -> WindowState {
///         WindowState::new(WindowGeometry::new(None, Size::default()), Size::default())
///     }
///     fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
///         // Under the capabilities of its own snapshot, which honour
///         // nothing but the normal mode every window has.
///         command.against(&WindowCapabilities::NONE)?;
///         Ok(())
///     }
///     fn set_cursor(&self, _cursor: CursorIcon) -> Result<(), CursorError> {
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
pub trait PlatformWindow: Send + Sync {
    /// This window's platform-internal identity.
    ///
    /// A window that cannot state its identity cannot be demultiplexed
    /// (ADR-0037 §2): the identity is what lets the demux boundary look up
    /// which `(UiRuntimeId, PresentationId)` a native event belongs to. Every
    /// implementor must return a real, stable-for-the-window's-lifetime
    /// value — never a shared sentinel that would make two different
    /// windows compare equal.
    fn id(&self) -> WindowId;

    /// Query gesture geometry for this presentation's native coordinate context.
    ///
    /// Call on the platform owner thread after accepting system-preference or DPI
    /// changes. This query shares the host's observation lifetime; it installs no
    /// additional native subscription. `Ok(None)` means unsupported geometry.
    ///
    /// # Errors
    /// A closed owner, wrong thread, native failure or invalid projection fails
    /// the query. Preserve the last accepted projection and arrange a bounded
    /// retry instead of replacing it with unknown values.
    fn gesture_geometry(
        &self,
    ) -> Result<Option<crate::GestureGeometry>, crate::PreferenceQueryError> {
        Ok(None)
    }

    /// Request that this window produce a frame.
    ///
    /// **Owner thread only**, per this trait's [Thread
    /// affinity](#thread-affinity) default. ADR-0045 decision 5 settles the
    /// direction — "the raster side never calls
    /// `PlatformWindow::request_redraw`, on any backend" — and lists the
    /// alternative, calling it directly on backends where it appears to work,
    /// as rejected.
    ///
    /// **No supported worker-side route exists yet.** The intended one is a
    /// redraw verb on `flui_native::PlatformProxy`, and it is absent
    /// on *every* backend, not just the lane-less ones: that lane carries only
    /// `open_window` and `request_quit`. So a worker needing a frame today has
    /// no conforming call available — which is precisely why the two paths
    /// below violate this rule instead of being fixable at their call sites.
    ///
    /// The rule is spelled out here rather than left to the default because
    /// `Send + Sync` makes the wrong call compile from anywhere, and because
    /// what the wrong call costs differs per backend in a way that hides it:
    ///
    /// - **winit** posts and the loop delivers later; **Win32**'s
    ///   `InvalidateRect` leaves `WM_PAINT` for the owning thread's pump;
    ///   **android** sets an atomic. All three tolerate a cross-thread call, so
    ///   exercising one there proves nothing about the rule.
    /// - **headless** carries a cross-thread request to the owner's next turn,
    ///   where the window handler's `frame_requested` runs; **web** has one
    ///   thread.
    /// - **macOS** messages `-[NSView setNeedsDisplay:]` inside an `unsafe`
    ///   block whose `unsafe impl Send` justification IS main-thread affinity.
    ///   It is the only backend where the wrong call is *unsound* rather than
    ///   merely tolerated, and it is type-checked by `cross-typecheck` (with
    ///   Win32 and android) but never linked or executed anywhere.
    ///
    /// # Violated on two paths (issue #949)
    ///
    /// Stating the rule does not enforce it. Neither of these can be fixed by
    /// its own caller: both need the proxy's redraw verb, which needs
    /// transports the lane-less backends do not have yet (tracked by #949,
    /// scoped with #559 and #551).
    ///
    /// - `flui_app`'s frame wake handle — **unconditional**. It is installed as
    ///   the scheduler's `on_frame_scheduled` hook and handed to async wakers,
    ///   so it fires on whatever thread completed the future. No test pins
    ///   it.
    /// - `flui_runtime`'s AccessKit activation listener — **conditional**: every
    ///   Win32, AppKit and Linux winit window has an adapter, but the listener
    ///   fires only once an assistive technology attaches, on the thread the
    ///   adapter calls it from (AT-SPI's is the adapter's own). No test pins
    ///   this one.
    fn request_redraw(&self);

    /// Tell the backend a frame is about to be presented for this window —
    /// called once per presented frame, immediately before the present.
    ///
    /// On Wayland this is what makes [`request_redraw`](Self::request_redraw)
    /// compositor-paced: winit arms the surface's frame callback here, and
    /// withholds the next `RedrawRequested` until the compositor delivers
    /// it — one per display refresh while the surface is visible, none at
    /// all while it is fully hidden, so a running animation is paced by the
    /// panel and an occluded window costs nothing. Without this call every
    /// `request_redraw` is delivered immediately, whatever the display is
    /// doing. A no-op on backends with no such protocol.
    fn pre_present_notify(&self) {}

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
    /// that ends the loop when this was the last window. Win32 and the headless
    /// double run that teardown before `apply` returns; winit runs it on the
    /// owner thread's next turn. A window that has already closed answers
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
    /// Owner thread only. The default, for a backend that cannot place its
    /// windows on one desktop, answers [`WindowAt::Unknown`].
    fn window_at(&self, position: Point<f64>) -> WindowAt {
        let _ = position;
        WindowAt::Unknown
    }

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
    /// It is a verb, not a [`WindowCommand`](crate::WindowCommand): commands apply at the
    /// owner's next turn, and this is sent every frame (ADR-0254 §3). Owner thread only.
    ///
    /// # Errors
    ///
    /// [`Unsupported`] exactly when the window's
    /// [`InputCapabilities::input_regions`](crate::input_capabilities::InputCapabilities::input_regions)
    /// is `Unavailable`: the OS honours no input region here, and the set is not kept.
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
    fn reveal_after_first_frame(&self) {}

    /// Apply the cursor selected by this window's presentation.
    ///
    /// This is deliberately window-scoped: a process-global cursor setter
    /// cannot identify which of several presentations owns the hovered region.
    ///
    /// # Errors
    ///
    /// Returns [`CursorError::Unsupported`] when this window backend has no
    /// pointer-cursor facility, or [`CursorError::Backend`] when the native
    /// update fails.
    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError>;

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

    // ==================== Window Handles (for GPU integration)
    // ====================

    /// Get a window handle for creating GPU surfaces (wgpu, etc.)
    ///
    /// Concrete platform windows (WindowsWindow, MacOSWindow) implement
    /// `raw_window_handle::HasWindowHandle` and delegate through this method.
    /// Headless windows return `HandleError::Unavailable`.
    ///
    /// # Contract: MUST answer `Unavailable` once the native window is gone
    ///
    /// [`raw_window_handle::HasWindowHandle`]'s own contract is an
    /// object-lifetime claim, not a borrow-lifetime one: the returned handle
    /// "should last for the lifetime of the object", and the implementation
    /// "should return an error if the application is inactive" (that trait's
    /// doc, `raw-window-handle` 0.6.2). The `'_` in this signature is the
    /// borrow of `self`, which is a different bound and does not encode the
    /// pointer's validity — the Android backend below demonstrates the gap:
    /// its `AndroidWindow` is held across a pause and answers `Ok` there,
    /// while the `ANativeWindow` the returned handle wraps is released by the
    /// command applied *after* the `TerminateWindow` callback returns.
    /// Upstream also makes a type-level claim beyond that prose: the
    /// `WindowHandle<'a>` type's doc says all pointers within it "are
    /// guaranteed to be valid and not dangling for the lifetime of the
    /// handle", and that is the claim `WindowHandle::borrow_raw`'s `# Safety`
    /// answers to. The Android backend cannot meet that claim by
    /// construction, because the handle's `'a` is the borrow of `self` and
    /// nothing ties the pointer's life to it; the SAFETY block on
    /// `AndroidWindow::window_handle` says so and names the consumer-enforced
    /// obligation that stands in for it.
    ///
    /// So this trait states the contract the engine needs, and it is stricter
    /// than upstream's: an engine holding a long-lived `Arc<dyn PlatformWindow>`
    /// (issue #1043) relies on it, re-querying this method on recovery rather
    /// than reusing a handle captured earlier, and a stale `Ok` handed back
    /// for a destroyed or suspended window is exactly the unsound escape that
    /// design closes. Every implementor **must** return
    /// `Err(HandleError::Unavailable)` once its native window is destroyed or
    /// otherwise unusable — never a handle whose pointee no longer exists, or
    /// exists but is temporarily unusable.
    ///
    /// How each backend satisfies this:
    /// - **winit** — delegates to the wrapped `winit::Window`, which owns the
    ///   native window for as long as it is alive; there is no destroyed
    ///   state to detect separately.
    /// - **Win32** (`WindowsWindow`) — consults the pure `teardown_route`
    ///   identity probe (`flui-native`'s `owner::hwnd_affinity`) and returns
    ///   `Unavailable` for `AlreadyGone`/`StaleHandle` instead of handing out a handle
    ///   wrapping a destroyed or recycled `HWND`.
    /// - **AppKit** (`MacOSWindow`) — an explicit `closed` flag, set from the
    ///   `windowWillClose:` delegate callback, which AppKit's own `-close`
    ///   posts on every route through it regardless of the should-close
    ///   veto (see `MacOSWindow::close`'s own doc), so no second call site
    ///   needs to set it.
    /// - **Android** (`AndroidWindow`) — answers `Unavailable` exactly when
    ///   `AndroidApp::native_window()` is `None`, and `android-activity`
    ///   0.6.1 leaves that `Some` across an ordinary pause: the command that
    ///   clears it is applied after the `TerminateWindow` callback returns,
    ///   and the new window is set before the `InitWindow` callback runs. So
    ///   across an ordinary pause — one with no `TerminateWindow` inside it —
    ///   this method keeps answering `Ok` for the whole span between the
    ///   `Pause` and the matching `Resume`; a `TerminateWindow` inside that
    ///   span flips the answer to `Unavailable` from
    ///   `post_exec_cmd(TermWindow)` until `pre_exec_cmd(InitWindow)`. Either
    ///   way the answer is **not** the same thing as the surface's validity:
    ///   the swapchain dies with the window, at `TerminateWindow`, and a
    ///   pause that keeps the window keeps the swapchain too. The release is
    ///   driven by the handler's
    ///   [`surface_changed`](crate::WindowHandler::surface_changed), not by
    ///   this method's answer.
    /// - **Headless** — always `Unavailable` (no native handle exists).
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::Unavailable)
    }

    /// Get a display handle for creating GPU surfaces (wgpu, etc.)
    ///
    /// Concrete platform windows (WindowsWindow, MacOSWindow) implement
    /// `raw_window_handle::HasDisplayHandle` and delegate through this method.
    /// Headless windows return `HandleError::Unavailable`.
    ///
    /// This MUST applies at minimum to [`window_handle`](Self::window_handle),
    /// whose returned handle wraps a per-window native identity that
    /// genuinely goes away. It is inert on the two native desktop backends
    /// this repository implements today because their display-handle types
    /// carry no such identity to go stale: `WindowsDisplayHandle` and
    /// `AppKitDisplayHandle` are both zero-field placeholders (Windows and
    /// AppKit multi-monitor enumeration happens through separate APIs, not
    /// through this handle), so `WindowsWindow`/`MacOSWindow` return `Ok`
    /// from `display_handle` unconditionally rather than gating it on the
    /// same destroyed/closed check `window_handle` uses — there is nothing
    /// in the handle itself for that check to protect. A future backend
    /// whose display handle DOES wrap a live native reference must still
    /// honor the MUST there.
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::Unavailable)
    }
}

impl HasWindowHandle for dyn PlatformWindow + '_ {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        PlatformWindow::window_handle(self)
    }
}

impl HasDisplayHandle for dyn PlatformWindow + '_ {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        PlatformWindow::display_handle(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shared window a renderer holds is a raw-handle target on its own,
    /// with no other crate in the graph enabling `raw-window-handle/alloc`.
    const fn assert_bounds<T: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static>() {}
    const _: () = assert_bounds::<std::sync::Arc<dyn PlatformWindow>>();
}
