//! Platform window trait
//!
//! Provides a thin abstraction over platform windows for testability
//! and flexibility. Includes per-window callback registration for event
//! delivery.

use std::{any::Any, sync::Arc};

use cursor_icon::CursorIcon;
use flui_types::geometry::{Bounds, DevicePixels, Pixels, Point, Size};

use flui_platform_api::{
    CursorError, DispatchEventResult, Modifiers, PlatformDisplay, PlatformHaptics, PlatformInput,
    PlatformTextInput, WindowAppearance, WindowBackgroundAppearance, WindowBounds,
    WindowExecutionState, WindowId, WindowShowError,
};

use super::accessibility::PlatformAccessibility;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
#[cfg(feature = "winit-backend")]
use winit::window::Window;

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
/// [`crate::WindowCallbacks`] for nested input return semantics.
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
/// [`PlatformProxy`](crate::PlatformProxy), the recorded cross-thread-to-owner
/// lane (ADR-0039 §3). That lane is **incomplete**, and this section states the
/// rule ahead of the mechanism for obeying it: the lane carries `open_window`
/// and `request_quit` only, and just one backend (winit) supplies a transport
/// at all — the rest return `ClosedTransport`, answering every request with
/// [`ProxySendError::Unsupported`](crate::ProxySendError::Unsupported).
///
/// [`close`](Self::close) is the one method that documents itself out of this
/// default: it is callable from any thread the native API permits, and states
/// per backend what the cross-thread route costs (AppKit excepted).
///
/// The macOS backend enforces this default mechanically: every window-driving
/// `PlatformWindow`/`WindowTrait`/`MacOSWindowExtTrait` body on `MacOSWindow`
/// re-enters the owner lane through `route_on_owner`, so a call from any
/// thread is marshaled onto the window's owner lane before any AppKit message
/// is sent. Two carve-outs: the class-E raw-handle accessors
/// (`raw_window_handle`/`window_handle`, and `display_handle` which carries no
/// pointer) take their NSView outside the routing — `!Send` outputs whose
/// enforcement is upstream (raw-window-metal's main-thread hard panic plus
/// `debug_assert_appkit_main_thread`-guarded platform entries, ADR-0039); and
/// the `enable_tiling`/`disable_tiling`/`is_tiling_enabled` trio never routes
/// (recorded for observability only until the native API is adopted).
///
pub trait PlatformWindow: Send + Sync {
    /// This window's platform-internal identity.
    ///
    /// A window that cannot state its identity cannot be demultiplexed
    /// (ADR-0037 §2): the identity is what lets the demux boundary look up
    /// which `(RealmId, PresentationId)` a native event belongs to. Every
    /// implementor must return a real, stable-for-the-window's-lifetime
    /// value — never a shared sentinel that would make two different
    /// windows compare equal.
    fn id(&self) -> WindowId;

    /// Get the window size in physical pixels (device pixels)
    fn physical_size(&self) -> Size<DevicePixels>;

    /// Get the window size in logical pixels
    fn logical_size(&self) -> Size<Pixels>;

    /// Get the scale factor (DPI scaling)
    fn scale_factor(&self) -> f64;

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
    /// redraw verb on [`PlatformProxy`](crate::PlatformProxy), and it is absent
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
    ///   so it fires on whatever thread completed the future. Pinned by
    ///   `the_frame_wake_pokes_the_window_from_the_thread_that_fired_it`.
    /// - `flui_app`'s AccessKit activation listener — **conditional**, and so
    ///   not reachable in a default build: it exists only under the non-default
    ///   `a11y` feature, and fires only once an assistive technology attaches to
    ///   the adapter's own thread. No test pins this one.
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

    /// Check if window is focused
    fn is_focused(&self) -> bool;

    /// Check if window is visible
    fn is_visible(&self) -> bool;

    // ==================== Query Methods (US2) ====================

    /// Get the window bounds (position + size) in logical pixels
    fn bounds(&self) -> Bounds<Pixels> {
        Bounds::default()
    }

    /// Get the content (client area) size in logical pixels
    fn content_size(&self) -> Size<Pixels> {
        self.logical_size()
    }

    /// Get the window bounds state (windowed, maximized, or fullscreen)
    fn window_bounds(&self) -> WindowBounds {
        WindowBounds::Windowed(self.bounds())
    }

    /// Check if window is maximized
    fn is_maximized(&self) -> bool {
        false
    }

    /// Check if window is in fullscreen mode
    fn is_fullscreen(&self) -> bool {
        false
    }

    /// Check if window is the active (foreground) window
    fn is_active(&self) -> bool {
        self.is_focused()
    }

    /// Check if the mouse cursor is hovering over this window
    fn is_hovered(&self) -> bool {
        false
    }

    /// Get the current mouse position in logical pixels (relative to window)
    fn mouse_position(&self) -> Point<Pixels> {
        Point::default()
    }

    /// Get the currently pressed keyboard modifiers
    fn modifiers(&self) -> Modifiers {
        Modifiers::empty()
    }

    /// Get the window's current appearance (light/dark)
    fn appearance(&self) -> WindowAppearance {
        WindowAppearance::default()
    }

    /// Get the display this window is currently on
    fn display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        None
    }

    /// The refresh period of the display this window is currently on, when
    /// the backend can tell — the interval an embedder's own frame pacing
    /// should never produce faster than. `None` when unknown (no monitor
    /// reported, a backend without the query); an embedder then falls back
    /// to a documented default rather than guessing. Re-query after a move
    /// or resize: a window can change monitors, and monitors differ.
    fn refresh_period(&self) -> Option<std::time::Duration> {
        None
    }

    /// Get this window's IME text-input capability, if the backend supports
    /// it. `None` for backends that cannot honor IME composition (returned
    /// by this trait's default so every non-desktop/no-IME backend does not
    /// have to inherit unusable `set_ime_allowed`/`set_ime_cursor_area`
    /// methods directly on `PlatformWindow`).
    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        None
    }

    /// Get this window's haptic feedback capability, if the backend
    /// supports it. `None` for backends with no haptic hardware (desktop
    /// winit targets; a minimal future embedder) — see
    /// [`PlatformHaptics`]'s module doc for the full per-window-not-global
    /// rationale.
    fn haptics(&self) -> Option<Arc<dyn PlatformHaptics>> {
        None
    }

    /// Get this window's accessibility capability, if the backend exposes one.
    ///
    /// `None` for a backend with no accessibility integration — which is every
    /// backend until its per-OS adapter is wired, and permanently for one with
    /// no such platform API. A composition root that gets `None` simply never
    /// enables semantics assembly, so the cost is not paid either.
    fn accessibility(&self) -> Option<Arc<dyn PlatformAccessibility>> {
        None
    }

    /// Get the window title
    fn get_title(&self) -> String {
        String::new()
    }

    // ==================== Control Methods (US2) ====================

    /// Set the window title
    fn set_title(&self, title: &str) {
        let _ = title;
    }

    /// Show this window, unminimizing it and requesting foreground focus.
    /// Preserves maximized/fullscreen mode and the existing widget tree.
    /// Focus is a request: the window manager may decline to grant it.
    ///
    /// # Errors
    /// Returns unsupported when the backend cannot perform this operation,
    /// closed after native teardown, or a native operation failure.
    fn show(&self) -> Result<(), WindowShowError> {
        Err(WindowShowError::Unsupported)
    }

    /// The embedder has presented the first frame into this window's
    /// surface — or has waited as long as it is willing to for one.
    ///
    /// A backend that defers the physical reveal of a window opened
    /// [`WindowOptions::visible`](crate::traits::WindowOptions::visible)
    /// `== true` performs it now, exactly once; a window opened hidden, one
    /// already shown explicitly (`show`/`set_visible(true)`/`activate`),
    /// or a backend that reveals at open, ignores the call. Calling it
    /// again is harmless.
    ///
    /// The embedder calls this after the first frame whose present
    /// succeeded, and at a bounded fallback after a frame that ran and
    /// presented nothing, so a surface that never presents still yields a
    /// window the user can see and close rather than a process with no
    /// window at all.
    fn reveal_after_first_frame(&self) {}

    /// Activate (bring to front / focus) the window.
    fn activate(&self) {}

    /// Minimize the window
    fn minimize(&self) {}

    /// Maximize the window
    fn maximize(&self) {}

    /// Restore the window from minimized or maximized state
    fn restore(&self) {}

    /// Toggle fullscreen mode
    fn toggle_fullscreen(&self) {}

    /// Resize the window to the given logical size
    fn resize(&self, size: Size<Pixels>) {
        let _ = size;
    }

    /// Close and destroy the window — a decision already made, not a
    /// request.
    ///
    /// Bypasses the should-close veto ([`on_should_close`](Self::on_should_close))
    /// when called on the backend's owning thread — natively so: AppKit's
    /// `-close` never sends `windowShouldClose:`, and a same-thread Win32
    /// `DestroyWindow` never sends `WM_CLOSE`. Off the owning thread the
    /// veto's fate is backend-defined: Win32 posts `WM_CLOSE`, whose owner-side
    /// handler re-asks `on_should_close` before destroying, so a cross-thread
    /// `close()` there is a close *request* (see that impl); winit defers the
    /// whole teardown to the owner thread's next turn with no veto asked.
    ///
    /// Never bypasses the *bookkeeping* once the close proceeds: the backend
    /// runs the same teardown a user-initiated close takes — the
    /// [`on_close`](Self::on_close) callback, removal from the backend's window
    /// tracking, cleanup of per-window input state, the global
    /// [`WindowEvent::Closed`](crate::WindowEvent::Closed), and the exit-policy
    /// consult that ends the loop when this was the last window. That
    /// bookkeeping is universal; the *timing* below is not.
    ///
    /// Callable from any thread the native windowing API permits. On winit the
    /// teardown, and so `on_close`, runs on the owner thread's next turn —
    /// never synchronously within this call, whichever thread makes it. Win32's
    /// same-thread route is the opposite: `DestroyWindow` dispatches
    /// `WM_DESTROY`, and with it `on_close` and `Closed`, before this call
    /// returns. The headless double follows Win32's shape and runs the teardown
    /// synchronously, which is what the tests asserting state right after
    /// `close()` pin; issue #937 tracks an opt-in mode for winit's timing, for
    /// when something needs to observe it headlessly. AppKit's `close()` has no
    /// thread marshaling today: call it from the main thread only.
    ///
    /// One thing the double does *not* model, and cannot until #937 lands: it
    /// runs `on_close` on the calling thread, while this trait requires every
    /// window callback to run on the thread that registered it.
    fn close(&self) {}

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
    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult + Send>) {
        let _ = callback;
    }

    /// Register a callback for frame rendering requests
    ///
    /// Called by the platform when a new frame should be rendered (e.g., after
    /// `request_redraw()` or when the compositor needs content).
    fn on_request_frame(&self, callback: Box<dyn FnMut() + Send>) {
        let _ = callback;
    }

    /// Register a callback for window resize events
    ///
    /// Called with the new logical size and current scale factor.
    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32) + Send>) {
        let _ = callback;
    }

    /// Register a callback for window move events
    fn on_moved(&self, callback: Box<dyn FnMut() + Send>) {
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

    /// Register a callback for focus changes
    ///
    /// Called with `true` when the window gains focus, `false` when it loses
    /// focus.
    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        let _ = callback;
    }

    /// Safe-area intrusions in logical pixels, relative to the content view.
    /// Backends without native inset reporting return zero. Detached windows
    /// retain their last accepted geometry; this is not keyboard occlusion.
    fn safe_area_insets(&self) -> flui_types::geometry::EdgeInsets {
        flui_types::geometry::EdgeInsets::ZERO
    }

    /// Observe safe-area changes on the owner thread. Register, then resample
    /// `safe_area_insets` to cover changes before callback installation.
    fn on_safe_area_change(
        &self,
        callback: Box<dyn FnMut(flui_types::geometry::EdgeInsets) + Send>,
    ) {
        let _ = callback;
    }

    /// Current native execution eligibility. Backends without suspension use Running.
    fn execution_state(&self) -> WindowExecutionState {
        WindowExecutionState::Running
    }

    /// Observe native execution changes on the window's owner thread.
    /// Registration does not emit a snapshot; register first, then read execution_state.
    /// This does not signal focus or GPU surface success. Detached is reversible.
    fn on_execution_state_change(&self, callback: Box<dyn FnMut(WindowExecutionState) + Send>) {
        let _ = callback;
    }

    /// Register a callback for visibility (occlusion) changes.
    ///
    /// Called with `true` when the window becomes visible/unoccluded,
    /// `false` when it becomes fully occluded (or minimized, on backends
    /// that report that through the same signal). Distinct from
    /// [`on_active_status_change`](Self::on_active_status_change): a window
    /// can be visible but unfocused, or occluded while still nominally
    /// focused.
    ///
    /// Delivery is backend-conditional, verified against winit 0.30's own
    /// `WindowEvent::Occluded` documentation and `platform_impl` source
    /// (not assumed): the emitting backends are **X11** (via Xlib's
    /// `VisibilityFullyObscured` — fires only on FULL obscuration, never
    /// partial), **macOS**, **iOS**, and **Web**. Winit's own doc states
    /// plainly: "Android / Wayland / Windows / Orbital: Unsupported." —
    /// there is no Wayland emitter anywhere in `platform_impl` at all, not
    /// merely a compositor-dependent one. On a Wayland compositor (this
    /// workspace's own primary desktop reference session), this callback
    /// never fires and the window is always treated as visible; on X11 it
    /// only fires when a window is fully covered, which a compositing
    /// window manager may rarely or never produce.
    ///
    /// The native backends derive their own signal (rules in
    /// `shared::visibility`, host-tested): **Win32** from
    /// `WM_SIZE`-minimize/restore plus `WM_SHOWWINDOW` hide/show (Windows
    /// has no occlusion events at all), **AppKit** from
    /// `windowDidChangeOcclusionState:`'s visible bit (fires on full
    /// occlusion, miniaturization, hide, and space switches). The headless
    /// backend's `simulate_visibility` drives the same wire for tests.
    fn on_visibility_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        let _ = callback;
    }

    /// Register a callback for mouse hover changes
    ///
    /// Called with `true` when the mouse enters the window, `false` when it
    /// leaves.
    fn on_hover_status_change(&self, callback: Box<dyn FnMut(bool) + Send>) {
        let _ = callback;
    }

    /// Register a callback for system appearance changes (light/dark theme)
    fn on_appearance_changed(&self, callback: Box<dyn FnMut() + Send>) {
        let _ = callback;
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
    /// This is not [`on_active_status_change`](Self::on_active_status_change)
    /// with a different name. That one also fires on focus changes, and the
    /// surface must not be dropped because a window lost focus; the two
    /// signals coincide on some backends and diverge on the ones that report
    /// focus at all.
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
    /// The signal is genuinely two-state, and every sibling in this family is
    /// `bool` (`on_active_status_change`, `on_visibility_status_change`,
    /// `on_hover_status_change`). A callback's parameter type is a one-way
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
    ///   identity probe (`shared::hwnd_affinity`) and returns `Unavailable`
    ///   for `AlreadyGone`/`StaleHandle` instead of handing out a handle
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

    // ==================== Utility ====================

    /// Get the underlying winit window (if available)
    ///
    /// Returns `None` for non-winit platforms (e.g., headless testing).
    #[cfg(feature = "winit-backend")]
    fn as_winit(&self) -> Option<&Arc<Window>> {
        None
    }

    /// Downcast to concrete type.
    ///
    /// No default body: a panicking default here would only be discovered
    /// the first time some caller downcasts a backend that forgot to
    /// override it — every implementor must supply its own (invariably
    /// `{ self }`), the same shape [`super::PlatformHaptics::as_any`]
    /// already requires.
    fn as_any(&self) -> &dyn Any;
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

    // Mock window for testing
    struct MockWindow {
        size: Size<Pixels>,
        scale_factor: f64,
        focused: bool,
        visible: bool,
    }

    impl PlatformWindow for MockWindow {
        fn id(&self) -> WindowId {
            WindowId(1)
        }

        fn physical_size(&self) -> Size<DevicePixels> {
            use flui_types::geometry::device_px;

            Size::new(
                device_px((self.size.width.0 * self.scale_factor as f32) as i32),
                device_px((self.size.height.0 * self.scale_factor as f32) as i32),
            )
        }

        fn logical_size(&self) -> Size<Pixels> {
            self.size
        }

        fn scale_factor(&self) -> f64 {
            self.scale_factor
        }

        fn request_redraw(&self) {
            // No-op for mock
        }

        fn is_focused(&self) -> bool {
            self.focused
        }

        fn is_visible(&self) -> bool {
            self.visible
        }

        fn set_cursor(&self, _cursor: CursorIcon) -> Result<(), CursorError> {
            Ok(())
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[test]
    fn test_mock_window() {
        use flui_types::geometry::{device_px, px};

        let window = MockWindow {
            size: Size::new(px(800.0), px(600.0)),
            scale_factor: 2.0,
            focused: true,
            visible: true,
        };

        assert_eq!(
            window.physical_size(),
            Size::new(device_px(1600), device_px(1200))
        );
        assert_eq!(window.logical_size(), Size::new(px(800.0), px(600.0)));
        assert_eq!(window.scale_factor(), 2.0);
        assert!(window.is_focused());
        assert!(window.is_visible());
        assert_eq!(window.show(), Err(WindowShowError::Unsupported));
    }
}
