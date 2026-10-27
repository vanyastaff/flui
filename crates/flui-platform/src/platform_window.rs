//! The per-window contract.
//!
//! A thin abstraction over platform windows for testability and
//! flexibility, including per-window callback registration for event
//! delivery. It names no OS, winit or AccessKit type (ADR-0082 §1): the
//! accessibility bridge is reached through `flui_native::HostWindow`, a
//! host-side subtrait only the composition root sees.

use std::sync::Arc;

use cursor_icon::CursorIcon;
use flui_foundation::geometry::Point;

use crate::{
    CursorError, DispatchEventResult, PlatformHaptics, PlatformInput, PlatformTextInput,
    WindowBackgroundAppearance, WindowCommand, WindowCommandError, WindowId, WindowState,
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
/// # Callback Registration
///
/// Per-window callbacks use `&self` (not `&mut self`) with interior mutability.
/// This allows registering callbacks on shared references (`Arc<dyn
/// PlatformWindow>`). Callbacks are invoked by the platform's event loop when
/// native events arrive.
///
/// Callback storage locks are released before user code is invoked. Nested
/// notifications share one causal FIFO across event kinds; see
/// `flui_native::WindowCallbacks` for nested input return semantics.
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
/// *Registering* a callback is not driving one: registration writes a `Send`
/// callback into mutex-held storage from any thread, while **delivery** is
/// bound to the platform/event-loop thread that registered it. A backend
/// marshals first, or rejects the dispatch, rather than running user code on an
/// arbitrary worker. That is the same split ADR-0039 §2 records for
/// `Platform`'s own `on_*` methods.
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
/// them is one [`on_state_changed`](Self::on_state_changed). There is no getter
/// and no callback per fact: a consumer that needs to know which fact changed
/// compares the new snapshot with the one it saw last. Showing, activating and
/// closing a window are [`WindowCommand`]s like every other change.
///
/// # A window built from this crate alone
///
/// ```
/// use std::sync::Arc;
///
/// use flui_platform::{
///     CursorError, CursorIcon, PlatformHaptics, PlatformSystemChrome, PlatformTextInput,
///     PlatformWindow, Unsupported, UnsupportedReason, WindowCapabilities, WindowCommand,
///     WindowCommandError, WindowGeometry, WindowId, WindowState,
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
///     fn apply(&self, command: WindowCommand) -> Result<(), WindowCommandError> {
///         command.admit(&WindowCapabilities::NONE)?;
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
/// }
///
/// let window: &dyn PlatformWindow = &Offscreen;
/// assert_eq!(window.id(), WindowId(7));
/// assert!(matches!(
///     window.apply(WindowCommand::Show),
///     Err(WindowCommandError::Unsupported(_))
/// ));
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
/// use flui_platform::{
///     CursorError, CursorIcon, PlatformHaptics, PlatformSystemChrome, PlatformTextInput,
///     PlatformWindow, Unsupported, UnsupportedReason, WindowCapabilities, WindowCommand,
///     WindowCommandError, WindowGeometry, WindowId, WindowState,
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
///     fn apply(&self, command: WindowCommand) -> Result<(), WindowCommandError> {
///         command.admit(&WindowCapabilities::NONE)?;
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
    /// - **headless** dispatches the registered `on_request_frame` callback
    ///   *synchronously on the calling thread*, so a cross-thread call runs user
    ///   code on a worker — the delivery half of the rule above, broken. It is
    ///   the one backend here CI actually executes. **web** shares that body but
    ///   is wasm32-only with no executing coverage at all (#985), so nothing
    ///   exercises it either way.
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
    /// Call it on the owner thread. An implementation begins with
    /// [`WindowCommand::admit`] against the capabilities of its own snapshot,
    /// so it refuses with [`WindowCommandError::Unsupported`] exactly when a
    /// capability is unavailable, and with
    /// [`WindowCommandError::InvalidGeometry`] before the OS is called.
    ///
    /// [`WindowCommand::Close`] is a decision already made, not a request: it
    /// asks no should-close veto ([`on_should_close`](Self::on_should_close))
    /// and runs the same teardown a user's close takes — the
    /// [`on_close`](Self::on_close) callback, removal from the backend's window
    /// tracking, cleanup of per-window input state, and the exit-policy consult
    /// that ends the loop when this was the last window. Win32 and the headless
    /// double run that teardown before `apply` returns; winit runs it on the
    /// owner thread's next turn. A window that has already closed answers
    /// [`WindowCommandError::Closed`].
    ///
    /// # Errors
    /// [`WindowCommandError`].
    fn apply(&self, command: WindowCommand) -> Result<(), WindowCommandError>;

    /// Register a callback meaning "[`state`](Self::state) answers differently
    /// now".
    ///
    /// Called on the owner thread after a new snapshot was published, outside
    /// any lock the window holds, once per revision whichever fact changed:
    /// geometry, mode, metrics, appearance, display, safe area, execution
    /// state, focus, activation, visibility, hover or capabilities. Facts the window system
    /// reports together are published in one revision. A host delivers it
    /// inside the OS callback that reported the change where the OS gives one
    /// (each `WM_SIZE` of a Win32 live resize, inside its modal loop), before
    /// the callback for that one fact. The callback carries no payload: it
    /// re-reads `state()` and compares it with the snapshot it last saw.
    fn on_state_changed(&self, callback: Box<dyn FnMut() + Send>) {
        let _ = callback;
    }

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
    /// window's current scale through
    /// [`ProjectedInputRegions`](crate::input_regions::ProjectedInputRegions) and updates
    /// the OS only when the projection, the scale or the display changed, so no caller-side
    /// comparison can hide a scale or display change from it.
    ///
    /// A scale change or a move to another display need not composite a frame, so the
    /// framework also re-sends the last set when the presentation's pixel ratio changes
    /// when a published [`state`](Self::state) puts the window somewhere else: another
    /// position, display or frame. A backend whose window can change displays at an equal
    /// scale must publish that state and raise [`on_state_changed`](Self::on_state_changed),
    /// or the OS keeps the previous display's projection until the next composited frame.
    ///
    /// A backend whose OS honours no input region keeps this default, which ignores the set,
    /// and reports [`InputCapabilities::input_regions`](crate::input_capabilities::InputCapabilities::input_regions)
    /// as `Unavailable`. Owner thread only.
    fn set_input_regions(&self, regions: &[crate::input_regions::InputRegion]) {
        let _ = regions;
    }

    // ==================== Control ====================

    /// The embedder has presented the first frame into this window's
    /// surface — or has waited as long as it is willing to for one.
    ///
    /// A backend that defers the physical reveal of a window opened
    /// [`WindowOptions::visible`](crate::WindowOptions::visible)
    /// `== true` performs it now, exactly once; a window opened hidden, one
    /// already shown explicitly ([`WindowCommand::Show`] or
    /// [`WindowCommand::Activate`]),
    /// or a backend that reveals at open, ignores the call. Calling it
    /// again is harmless.
    ///
    /// The embedder calls this after the first frame whose present
    /// succeeded, and at a bounded fallback after a frame that ran and
    /// presented nothing, so a surface that never presents still yields a
    /// window the user can see and close rather than a process with no
    /// window at all.
    fn reveal_after_first_frame(&self) {}

    /// Set the window's background appearance (backdrop material)
    fn set_background_appearance(&self, appearance: WindowBackgroundAppearance) {
        let _ = appearance;
    }

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

    // ==================== Callback Registration ====================

    /// All callbacks registered on a window must be invoked on the same
    /// platform/event-loop thread that registered them. `Send` permits backend
    /// storage and wake plumbing; it is not permission to execute a UI callback
    /// on an arbitrary worker thread. Backends must marshal first or reject the
    /// dispatch when they cannot uphold this contract.
    ///
    /// Register a callback for input events (pointer, keyboard)
    ///
    /// The callback receives a `PlatformInput` and returns a
    /// `DispatchEventResult` indicating whether the event was consumed.
    ///
    /// It need not be `Send` (ADR-0082 §4): a backend stores it for the
    /// window's owner thread only, refuses a registration from any other
    /// thread (dropping the callback on the thread that made it), and runs
    /// and drops it on the owner, so it may capture an `Rc`.
    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        let _ = callback;
    }

    /// Register a callback for frame rendering requests
    ///
    /// Called by the platform when a new frame should be rendered (e.g., after
    /// `request_redraw()` or when the compositor needs content), with the
    /// [`FrameRequest`](crate::FrameRequest) the platform could fill: the frame's vsync and,
    /// where the platform names it, when the frame will be shown (ADR-0257).
    fn on_request_frame(&self, callback: Box<dyn FnMut(crate::FrameRequest) + Send>) {
        let _ = callback;
    }

    /// Register a callback for when the window is destroyed
    ///
    /// This fires once when the window is actually closed/destroyed.
    /// Uses `FnOnce` since it can only fire once.
    fn on_close(&self, callback: Box<dyn FnOnce() + Send>) {
        let _ = callback;
    }

    /// Register a callback to query whether the window should close
    ///
    /// Return `false` to veto the close request (e.g., unsaved changes dialog).
    /// If no callback is registered, close is always allowed.
    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool + Send>) {
        let _ = callback;
    }

    /// Tell the system which refresh rate this window wants from its display.
    ///
    /// [`FrameRateHint::High`](crate::FrameRateHint::High) asks for the display's highest rate while the
    /// window animates or is touched; [`FrameRateHint::Default`](crate::FrameRateHint::Default) withdraws the
    /// request and leaves the choice to the system. It is a request, not a
    /// command: the system may keep any rate. A native surface carries the
    /// request, so a host sends the current hint again after the surface was
    /// replaced. A backend without such a request, or one running on an
    /// operating system version that lacks it, ignores the hint; that is the
    /// default.
    ///
    /// Called on the owner thread.
    fn set_frame_rate_hint(&self, hint: crate::FrameRateHint) {
        let _ = hint;
    }

    /// Register a callback for the GPU surface's availability.
    ///
    /// Called with `false` when the surface behind this window is about to
    /// become invalid and must be released **before this callback returns**,
    /// and with `true` when a surface valid for the window's current native
    /// handle must exist. The signal is a request about the present, not a
    /// report about the past: a `true` asks for a live surface whatever
    /// happened before, so an implementation may receive more of either than
    /// it needs.
    ///
    /// This is not the snapshot's focus or execution state under another
    /// name: those change when a window loses focus or is suspended, and the
    /// surface must not be dropped for that; the signals coincide on some
    /// backends and diverge on the ones that report focus at all.
    ///
    /// # Delivery is backend-conditional, and the asymmetry costs
    ///
    /// **Android** emits: `MainEvent::TerminateWindow`
    /// and `MainEvent::Pause` produce `false`, `MainEvent::InitWindow` and
    /// `MainEvent::Resume` produce `true`. Winit's own Android support
    /// forwards `suspended()`/`resumed()` for the same pair, so a second
    /// emitter is available to the winit backend. Native **iOS** emits on true
    /// background/foreground transitions, never on temporary focus loss.
    ///
    /// A backend that never emits either signal is harmless: the surface is
    /// never released and the window is always treated as available. A
    /// backend that emits `false` and never `true` is not — the presentation
    /// stays released, every later frame is a skipped one, and the window
    /// stays blank while the loop reports "nothing to present" forever. An
    /// implementation that overrides no setter at all (a test double built
    /// from this trait's defaults) therefore accepts a registration and
    /// silently drops it. Inside this crate the mitigation is a convention
    /// rather than a mechanism: `shared::impl_window_callback_setters!` is
    /// `pub(crate)`, so only an in-crate backend can use it, and every one of
    /// them does — that macro is the single place this family's setters are
    /// written, so a backend *inside this crate* cannot override this method
    /// without storing its callback. An implementor outside this crate is not
    /// covered by that and has to store the callback itself; `flui-app`'s
    /// `TestWindow` (`window_test_support.rs`) implements this trait and
    /// overrides no setter, which is the live counterexample to reading that
    /// macro as a guarantee.
    ///
    /// # Why `bool`
    ///
    /// The signal is genuinely two-state. A callback's parameter type is a one-way
    /// door — changing it later breaks every registrant's closure — so the
    /// alternative considered was a `#[non_exhaustive] enum` (the shape these
    /// crates use for input and owner status). A third state would be a third
    /// state of the *surface*, which belongs in its own callback rather than
    /// in a widened parameter here (ADR-0035 splits this callback family by
    /// signal, not by arity).
    fn on_surface_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        let _ = callback;
    }

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
    ///   driven by
    ///   [`on_surface_status_change`](Self::on_surface_status_change), not by
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
