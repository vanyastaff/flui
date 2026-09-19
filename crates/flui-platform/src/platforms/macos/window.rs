//! macOS window (NSWindow) implementation

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use cursor_icon::CursorIcon;
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{ClassType, class, sel};
use objc2_app_kit::{NSBackingStoreType, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use flui_types::geometry::{Bounds, DevicePixels, Pixels, Point, Size};
use parking_lot::Mutex;
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle,
};

// Mid-migration shims for the raw-send shape this file keeps: `NSWindow` is
// `MainThreadOnly` in objc2's typed API, but this backend constructs test
// windows on a caller-supplied off-main serial lane (`MacOSWindow::for_test`),
// which a `MainThreadMarker`-gated method would refuse. The file therefore
// sends raw `msg_send!` (objc2's macro accepts a raw `*mut AnyObject` receiver,
// `Bool` arguments, and a manual `release`) and these aliases keep the
// Cocoa-era spelling readable.
type ObjcId = *mut AnyObject;
type Object = AnyObject;
type Class = AnyClass;
const NIL: ObjcId = std::ptr::null_mut();
type BObjC = Bool;
const YES: Bool = Bool::YES;
const NO: Bool = Bool::NO;

use super::{display::refresh_period_for_screen, view};
use crate::{
    config::WindowConfiguration,
    shared::WindowCallbacks,
    traits::{CursorError, OpenWindowError, PlatformWindow, WindowId, WindowOptions},
};

/// macOS window wrapper around NSWindow
pub struct MacOSWindow {
    /// Native window handle (NSWindow*)
    ns_window: ObjcId,

    /// Window state
    state: Arc<Mutex<MacOSWindowState>>,

    /// Reference to all windows
    windows_map: Arc<Mutex<HashMap<u64, Arc<MacOSWindow>>>>,

    /// Per-window callbacks (input, resize, close, ...)
    callbacks: Arc<WindowCallbacks>,

    /// Set once `windowWillClose:` has fired for this window (or `close()`
    /// completed without it firing — see `close()`'s own doc). Shared via
    /// `Arc` like every other per-window field here, so every clone of this
    /// wrapper sees the same window's closed state instead of each tracking
    /// its own. Consulted by [`HasWindowHandle::window_handle`] because,
    /// with `setReleasedWhenClosed:NO` set at construction, `-[NSWindow
    /// close]` no longer deallocates `ns_window` — so `contentView` stays
    /// answerable long after the window is meaningless to hand a GPU handle
    /// for, and only this flag (not a nil check) can tell the two apart.
    closed: Arc<AtomicBool>,

    /// Window configuration
    config: WindowConfiguration,

    /// The owner lane every thread-affine AppKit message travels through
    /// (see `super::owner_lane`). The AppKit main queue for windows created
    /// by [`MacOSWindow::new`]; a caller-supplied serial lane for test
    /// windows created by `MacOSWindow::for_test` (a `#[cfg(test)]` constructor,
    /// absent from non-test doc builds, hence the plain-font reference).
    owner: &'static dispatch::Queue,

    /// True when `owner` is the main queue, so "on the main thread" is
    /// equivalent to "on the owner lane" for the direct-path probe in
    /// [`super::owner_lane::exec_on_owner`].
    owner_is_main: bool,

    /// This window's NSAccessibility bridge, subclassed onto the content
    /// view. A `OnceLock` slot rather than a plain field because the window
    /// value is constructed *before* its content view exists (the view
    /// holds a `Weak` back-reference to the window's callbacks); the
    /// constructor fills it immediately after installing the view.
    #[cfg(feature = "a11y")]
    accessibility: std::sync::OnceLock<Arc<super::accessibility::MacosAccessibility>>,

    /// The platform's wake pump, armed from [`PlatformWindow::request_redraw`].
    ///
    /// `Weak`, and a `OnceLock`: the pump is platform-owned and outlives every
    /// window, so a window must not keep it alive — and the slot is filled
    /// once, by `MacOSPlatform::open_window` right after construction. A
    /// window built by the test constructor (`MacOSWindow::for_test`, a
    /// `#[cfg(test)]` function absent from non-test doc builds, hence the
    /// plain-font reference) leaves it empty and simply never arms one.
    wake_pump: std::sync::OnceLock<super::wake_pump::WeakWakePump>,

    /// This window's IME capability, built on first access by
    /// [`PlatformWindow::text_input`]. A `OnceLock` for the same reason the
    /// accessibility slot above is one: the capability is discovered through a
    /// `&self` accessor and must be handed out as one shared instance per
    /// window, not a fresh one per call.
    text_input: std::sync::OnceLock<Arc<super::text_input::MacOSTextInput>>,
}

// SAFETY: the remaining fields are `Arc`/`Mutex`-protected, and sharing the
// wrapper across threads is required by the `PlatformWindow: Send + Sync`
// contract.
//
// # The NSWindow pointer is only messaged from the owner lane or from AppKit
//
// The raw `ns_window` pointer is thread-affine by AppKit's own doctrine
// (NSWindow is "generally not thread-safe"). What makes it sound to share this
// wrapper across threads is that every AppKit-messaging body of the public
// window surface is now mechanically routed: each travels through
// `route_on_owner` (this file), which dispatches the body onto the window's
// owner lane — the AppKit main queue in production, a caller-supplied serial
// queue for test windows (see `super::owner_lane`) — so no off-owner caller
// ever reaches AppKit bare. `request_redraw` was the first such site
// (issue #949); the sweep (issue #1194) extended the routing to every class-A
// body in the public window surface: the 12 `PlatformWindow` mutators,
// `request_redraw` itself, the 12 `WindowTrait` bodies, and the 11
// `MacOSWindowExtTrait` bodies. Off-owner callers of those methods (e.g. the
// scheduler's frame-wake hook completing on an IO-lane worker) therefore
// observe the routing, not a call-graph fact.
//
// One of those bodies reaches the lane by an extra hop rather than by a second
// door: `request_redraw`'s deferral hands its `setNeedsDisplay:` to
// `owner_lane::exec_async_guarded`, which installs the lane guard *before* the
// body runs and then routes that body through `route_on_owner` like the rest.
// The deferral changes *when* the call lands, never *which thread* issues it —
// which is why it does not weaken this argument.
//
// What remains outside the routed surface, and why each part is still sound:
// AppKit-delivered callbacks (delegate/view/event closures) run on the main
// thread by construction — AppKit does not deliver them elsewhere; the
// raw-handle accessors are `!Send` outputs whose enforcement is upstream
// (raw-window-metal's `Layer::from_ns_view` hard-panics off the main thread
// before surface creation) plus `debug_assert_appkit_main_thread`-guarded
// platform entries (ADR-0039) — their un-routed `contentView` getter is the
// owner-sweep plan's recorded class-E residual, a deliberate carve-out of the
// routing sweep rather than a local lapse; and [`Drop`] fire-and-forgets its
// AppKit tail
// (the a11y shutdown plus the window's balancing `release`) onto the owner
// lane without blocking. If the lane is no longer servicing, the closure is
// never run and the window is left over-retained — a leak, never a
// use-after-free, so a dealloc never runs off-main. The same ownership +
// affinity argument powers the lane-confined `OwnerLaneId` wrapper the drop
// tail uses: the id it owns is consumed only under the owner-lane guard.
//
// What the routing does NOT provide is the wake relay ADR-0045 decision 5
// mandates end-to-end: the scheduler hook still calls `request_redraw`
// directly. A relay (the hook posts, the owner thread drains) remains the
// real fix for the async lane and is scoped to the `PlatformProxy` redraw verb
// (#551/#559), not built here.
unsafe impl Send for MacOSWindow {}
// SAFETY: see `Send` above, including its statement of what is NOT enforced.
// Interior mutability is Mutex-guarded; the raw pointer's thread affinity is
// enforced by `route_on_owner` for the routed public surface, by the
// main-thread construction contract for AppKit-delivered callbacks, by the
// downstream `MainThreadMarker` hard panic for the surface-lease accessors,
// and by the owner-lane fire-and-forget tail for [`Drop`].
unsafe impl Sync for MacOSWindow {}

/// Mutable window state
struct MacOSWindowState {
    /// Current window bounds (logical pixels)
    bounds: Bounds<Pixels>,

    /// Scale factor (1.0 for non-Retina, 2.0 for Retina)
    scale_factor: f64,

    /// Inter-frame period of the display this window is on, when that display
    /// reports a rate. Cached, not queried per call: the query messages the
    /// window's screen, which is AppKit traffic this backend keeps on the
    /// owner lane, while `PlatformWindow::refresh_period` is a plain `&self`
    /// accessor any thread may call. Refreshed wherever `scale_factor` is —
    /// `windowDidChangeScreen:` and `windowDidChangeBackingProperties:` — which
    /// is the re-query point the trait's own contract names.
    refresh_period: Option<Duration>,

    /// Cursor selected by this exact window's presentation.
    cursor: CursorIcon,

    /// Last surface-visibility value dispatched through
    /// `WindowCallbacks::dispatch_visibility_status_change` (hidden-surface
    /// gating), fed by `windowDidChangeOcclusionState:`. Edge filter:
    /// AppKit documents that the notification may fire for reasons other
    /// than the visible bit flipping, so an unchanged value is never
    /// re-dispatched (`shared::visibility::visibility_edge`). Starts
    /// `true` — a freshly created window is treated as visible until AppKit
    /// says otherwise, matching every other backend's initial state.
    occlusion_visible: bool,
}

impl std::fmt::Debug for MacOSWindow {
    // Hand-written: `ns_window` is a raw Objective-C `id` and `WindowCallbacks`
    // is a callback payload; neither has a useful Debug form. `owner` (the
    // dispatch lane) has no useful Debug representation either, so it is
    // skipped along with them by `finish_non_exhaustive`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacOSWindow")
            .field("ns_window", &(self.ns_window as usize))
            .finish_non_exhaustive()
    }
}

impl MacOSWindow {
    /// Create a new macOS window
    ///
    /// # Errors
    /// [`OpenWindowError::Backend`] when AppKit refuses to allocate the
    /// NSWindow.
    pub fn new(
        options: WindowOptions,
        windows_map: Arc<Mutex<HashMap<u64, Arc<MacOSWindow>>>>,
        config: WindowConfiguration,
    ) -> Result<Arc<Self>, OpenWindowError> {
        // Production windows route every thread-affine AppKit message through
        // the AppKit main queue — the one lane all other process AppKit
        // traffic already serializes on (see `super::owner_lane`).
        Self::new_inner(
            options,
            windows_map,
            config,
            super::owner_lane::owner_queue(),
            true,
        )
    }

    /// Hand this window the platform's wake pump.
    ///
    /// Called once by `MacOSPlatform::open_window`, immediately after
    /// construction and before the window is handed to anyone who could
    /// redraw it. `pub(crate)` and a setter rather than a `MacOSWindow::new`
    /// parameter on purpose: `MacOSWindow` is public API, and threading a
    /// platform-internal type through its constructor would make the pump
    /// part of the surface for a dependency only the platform ever supplies.
    pub(crate) fn install_wake_pump(&self, pump: super::wake_pump::WeakWakePump) {
        // A second install is silently ignored rather than racing the first:
        // the slot is read on every redraw, so "first writer wins" is the
        // only outcome that cannot leave a window flipping between pumps.
        let _ = self.wake_pump.set(pump);
    }

    /// Arm the platform's wake pump, if this window has one.
    ///
    /// Called from [`PlatformWindow::request_redraw`] — the point that means
    /// "a frame is coming" for this window — because a frame arms its
    /// fallback deadline while it runs, and the pump is what looks back at
    /// that deadline once the frame has gone. See `super::wake_pump` for why
    /// the second look is needed at all.
    fn arm_wake_pump(&self) {
        if let Some(pump) = self.wake_pump.get().and_then(std::sync::Weak::upgrade) {
            pump.arm();
        }
    }

    /// Construct a test window on a caller-supplied owner lane.
    ///
    /// AppKit window construction is thread-affine, and this path is exercised
    /// off the AppKit main thread, so the whole construction sequence runs
    /// inside [`super::owner_lane::exec_on_owner`] on the lane rather than as a
    /// raw call from whichever thread the test runs on.
    #[cfg(test)]
    fn for_test(owner: &'static dispatch::Queue) -> Result<Arc<Self>, OpenWindowError> {
        super::owner_lane::exec_on_owner(owner, false, || {
            Self::new_inner(
                WindowOptions::default(),
                Arc::new(Mutex::new(HashMap::new())),
                WindowConfiguration::default(),
                owner,
                false,
            )
        })
    }

    fn new_inner(
        options: WindowOptions,
        windows_map: Arc<Mutex<HashMap<u64, Arc<MacOSWindow>>>>,
        config: WindowConfiguration,
        owner: &'static dispatch::Queue,
        owner_is_main: bool,
    ) -> Result<Arc<Self>, OpenWindowError> {
        // SAFETY: must run on the owner thread (enforced by the platform's
        // event-loop ownership, or by `for_test` routing construction onto
        // the lane); all messaged objects are alive: the freshly allocated
        // NSWindow is checked for NIL before further use.
        unsafe {
            // Convert logical size to NSRect
            let frame = NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(options.size.width.0 as f64, options.size.height.0 as f64),
            );

            // Build window style mask
            let mut style_mask = NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable;

            if options.decorated {
                style_mask |= NSWindowStyleMask::Titled;
            }

            if options.resizable {
                style_mask |= NSWindowStyleMask::Resizable;
            }

            // Create NSWindow
            let ns_window: ObjcId = msg_send![class!(NSWindow), alloc];
            let ns_window: ObjcId = msg_send![ns_window,
                initWithContentRect: frame
                styleMask: style_mask
                backing: NSBackingStoreType::Buffered
                defer: NO
            ];

            if ns_window == NIL {
                return Err(OpenWindowError::Backend {
                    message: "Failed to create NSWindow".to_string(),
                });
            }

            // AppKit's default for an alloc/init'd window is
            // `releasedWhenClosed: YES`: `-[NSWindow close]` would then
            // release the same +1 reference this constructor's `alloc`/
            // `init` pair already owns, and `MacOSWindow::drop`'s `release`
            // below would release it a second time — an over-release, and a
            // dangling `ns_window` for every `msg_send!` this type sends
            // afterward (including `window_handle()`'s `contentView`
            // fetch). Opting out here makes the `alloc`/`init` +1 released
            // exactly once, by `Drop`, matching winit's own
            // `setReleasedWhenClosed(false)` for its wrapped `NSWindow`.
            let _: () = msg_send![ns_window, setReleasedWhenClosed: NO];

            // Set window title
            let title = NSString::from_str(&options.title);
            let _: () = msg_send![ns_window, setTitle: &*title];

            // Apply size constraints
            if let Some(min) = options.min_size {
                let ns_size = NSSize::new(min.width.0 as f64, min.height.0 as f64);
                let _: () = msg_send![ns_window, setMinSize: ns_size];
            }
            if let Some(max) = options.max_size {
                let ns_size = NSSize::new(max.width.0 as f64, max.height.0 as f64);
                let _: () = msg_send![ns_window, setMaxSize: ns_size];
            }

            // Get backing scale factor
            let scale: f64 = msg_send![ns_window, backingScaleFactor];

            // Make window visible if requested
            if options.visible {
                let _: () = msg_send![ns_window, makeKeyAndOrderFront: NIL];
            }

            // Center window on screen
            let _: () = msg_send![ns_window, center];

            // Refresh period of the display this window is on. The window is
            // created at the origin — a point on a screen — so
            // `-[NSWindow screen]` answers here rather than returning NIL;
            // measured at this exact stage rather than assumed, by the
            // `scale_order.m` probe, which reports the screen live and the
            // display id reachable before `makeKeyAndOrderFront:`.
            // `screen` is nil only for a window not yet on any screen, which
            // cannot happen here (the window is created at the origin); a nil
            // therefore means no period, not a panic.
            let screen: ObjcId = msg_send![ns_window, screen];
            let refresh_period = screen_refresh_period(screen);

            let callbacks = Arc::new(WindowCallbacks::new());

            let window = Arc::new(Self {
                ns_window,
                state: Arc::new(Mutex::new(MacOSWindowState {
                    bounds: Bounds {
                        origin: Point::new(
                            flui_types::geometry::px(frame.origin.x as f32),
                            flui_types::geometry::px(frame.origin.y as f32),
                        ),
                        size: options.size,
                    },
                    scale_factor: scale,
                    refresh_period,
                    cursor: CursorIcon::default(),
                    occlusion_visible: true,
                })),
                windows_map: Arc::clone(&windows_map),
                callbacks,
                closed: Arc::new(AtomicBool::new(false)),
                config,
                owner,
                owner_is_main,
                #[cfg(feature = "a11y")]
                accessibility: std::sync::OnceLock::new(),
                wake_pump: std::sync::OnceLock::new(),
                text_input: std::sync::OnceLock::new(),
            });

            // Create content view for input events. `frame` is
            // objc2-foundation's `NSRect`, the type `create_content_view` takes.
            let content_view =
                view::create_content_view(frame, scale, Arc::downgrade(&window.callbacks));
            let _: () = msg_send![ns_window, setContentView: content_view];

            // Subclass the freshly installed content view for VoiceOver.
            // SAFETY: `content_view` is the live NSView this window just
            // created and installed; the window owns the capability, so the
            // adapter cannot outlive the view; and window construction runs
            // on the main thread (AppKit affinity, asserted by this whole
            // `unsafe` block's contract).
            #[cfg(feature = "a11y")]
            {
                let bridge = super::accessibility::MacosAccessibility::new(
                    content_view.cast::<std::ffi::c_void>(),
                );
                window
                    .accessibility
                    .set(Arc::new(bridge))
                    .expect("BUG: the accessibility slot was created empty in this constructor and nothing else can fill it");
            }

            // Enable mouse tracking for mouse moved events
            view::enable_mouse_tracking(content_view);

            // Make content view first responder to receive keyboard events
            let _: Bool = msg_send![ns_window, makeFirstResponder: content_view];

            // Set window delegate for lifecycle events
            let delegate = create_window_delegate(Arc::downgrade(&window));
            let _: () = msg_send![ns_window, setDelegate: delegate];

            // Store in windows map
            let window_id = ns_window as u64;
            let _prev = windows_map.lock().insert(window_id, Arc::clone(&window));

            tracing::info!(
                "Created NSWindow {:p} with size {}x{} (scale: {})",
                ns_window,
                options.size.width.0,
                options.size.height.0,
                scale
            );

            Ok(window)
        }
    }

    /// Get the NSWindow handle
    pub fn ns_window(&self) -> ObjcId {
        self.ns_window
    }

    /// Get the per-window callbacks registry
    pub fn callbacks(&self) -> &Arc<WindowCallbacks> {
        &self.callbacks
    }
}

/// Route one AppKit-messaging body through the window's owner lane.
///
/// This is the single throat every swept class-A body travels through: the
/// body runs ON the owner lane — inline on the OS main thread for a
/// main-lane owner, or dispatched under the reentrancy guard from any other
/// thread — so the lane, not a call-graph fact or a debug assert, is the
/// enforcement. A free function rather than a method so the always-run,
/// AppKit-free test can exercise it on the shared test lane without
/// constructing a real window.
///
/// The probe key is derived from `#[track_caller]`'s `(file, line)` under
/// `#[cfg(test)]`; production call sites carry no probe strings.
#[cfg_attr(test, track_caller)]
pub(super) fn route_on_owner<R: Send>(
    owner: &'static dispatch::Queue,
    owner_is_main: bool,
    f: impl FnOnce() -> R + Send,
) -> R {
    #[cfg(test)]
    let origin = {
        let loc = std::panic::Location::caller();
        (loc.file(), loc.line())
    };
    super::owner_lane::exec_on_owner(owner, owner_is_main, || {
        // Recorded INSIDE the routed body, at the probe point, so a bare un-routed
        // body (wrapper bypassed) records on_lane = false. Same single-writer
        // discipline as the #949 redraw probe. The witness is the
        // dispatch-guard marker, with exactly three arms: a dispatched off-lane
        // route installs the guard and records `true` (the enforcement arm); a
        // call NESTED inside an on-lane block inherits the OUTER block's guard
        // marker and also records `true`; only the OS-main cold-thread inline
        // arm — no marker in scope, and none may be installed (same-lane
        // self-deadlock) — records `false`. That is expected (see the
        // probe-semantics note below the module), so test assertions scope to
        // the off-lane arm.
        #[cfg(test)]
        routing_probe::record(origin, super::owner_lane::on_owner_queue(owner));
        f()
    })
}

/// Choose which arm a redraw request takes — the display-pass deferral, or the
/// inline send — and start it.
///
/// Free-standing for the same reason [`route_on_owner`] is: an always-run test
/// can pin the *choice* without an NSWindow, which a bare test process cannot
/// construct. Both arms arrive as closures, so what a test observes is exactly
/// which one started; what the window's own `request_redraw` supplies for them
/// is pinned by the bundled frame-pump probe, which reads frames rather than
/// closures.
///
/// AppKit discards a `setNeedsDisplay:` issued while it is displaying the view,
/// and the frame a display pass runs asks for its next frame from exactly there
/// (`drawRect:` → frame request → scheduler wake): that one request is dropped
/// and the pump stops for good — no frame, no wake, no frame. See
/// `super::display_pass` for the measurement behind that. The same call one turn
/// of the owner lane later is honoured instead, because the pass has unwound by
/// then; this is ADR-0039 §4(c) realised on AppKit — a wake that arrives while
/// the drain gate is closed defers rather than draining.
///
/// A request from any other moment is already delivered after the pass (its lane
/// hop waits for it), so it stays inline rather than paying a turn. The marker's
/// grain is the thread, not this window's pass, so a request for another window
/// issued while any view on this thread is displaying also defers: conservative,
/// and one lane turn late at worst, never dropped.
fn dispatch_redraw_request(defer: impl FnOnce(), send_inline: impl FnOnce() + Send) {
    if super::display_pass::in_display_pass() {
        defer();
        return;
    }
    send_inline();
}

#[cfg(test)]
mod routing_probe {
    use std::sync::Mutex;

    /// Shared sink of `(route_origin, on_lane)` witnesses. Every routed
    /// class-A body records its `(file, line)` origin and the dispatch-guard
    /// witness observed at the probe point. A single window test (or the
    /// always-run wrapper pin) is the only writer-reader, so no interleaving
    /// hazard; `clear()` demarcates a test phase.
    static SINK: Mutex<Vec<((&'static str, u32), bool)>> = Mutex::new(Vec::new());

    pub(super) fn clear() {
        *SINK
            .lock()
            .expect("routing_probe mutex is module-scoped and never poisoned") = Vec::new();
    }

    pub(super) fn record(origin: (&'static str, u32), on_lane: bool) {
        SINK.lock()
            .expect("routing_probe mutex is module-scoped and never poisoned")
            .push((origin, on_lane));
    }

    /// The last record's guard witness, or `None` before the first record of a
    /// test phase.
    pub(super) fn last() -> Option<bool> {
        SINK.lock()
            .expect("routing_probe mutex is module-scoped and never poisoned")
            .last()
            .map(|(_, on)| *on)
    }

    /// True when EVERY recorded witness reports the lane guard set.
    pub(super) fn all_on_lane() -> bool {
        SINK.lock()
            .expect("routing_probe mutex is module-scoped and never poisoned")
            .iter()
            .all(|(_, on)| *on)
    }
}

// Probe semantics. The probe records the guard witness at the probe point, so
// it is a *dispatch-guarded* marker, not "on the lane", and its truth has
// exactly three arms. (i) A dispatched off-lane route installs the guard and
// reads `true` — the enforcement arm. (ii) A call NESTED inside an on-lane
// block (caller already on the lane) runs `f()` inline WITHOUT installing a
// new guard, but it inherits the OUTER block's guard marker and therefore
// also reads `true`. (iii) The OS-main cold-thread inline arm is the only one
// that reads `false`: the thread is the owner's home thread, no guard marker
// is in scope, and none may be installed — installing one would mislabel a
// bare main-thread execution as guard-held, and the nested same-lane dispatch
// it would then fail to recognize self-deadlocks on a serial lane, which is
// precisely why `exec_on_owner` runs its direct paths bare. Test assertions
// against the probe must therefore be scoped to the off-lane arm (i).

impl MacOSWindow {
    /// Ask AppKit to display this window's content view.
    ///
    /// The one place `setNeedsDisplay:` is sent: `request_redraw`'s inline path
    /// calls it from inside its routed body, and the deferred path calls it
    /// from inside a routed body too, one turn of the lane later. `deferred`
    /// records which of the two sent it, because the two are only
    /// distinguishable in a log by their timing otherwise, and telling "the
    /// frame re-armed itself and the pump continued" from "the frame re-armed
    /// itself and was discarded" is what this whole path exists to make
    /// observable.
    ///
    /// # Safety
    ///
    /// - The caller must be on this window's owner lane: `ns_window` and its
    ///   content view are thread-affine AppKit objects. Both call sites reach
    ///   here from inside a `route_on_owner` body, which is what makes that
    ///   true; the `debug_assert` below re-checks it against the same predicate
    ///   `route_on_owner` routes with, so a caller that skips the routing
    ///   throat fails loudly in debug builds instead of messaging AppKit
    ///   off-lane.
    /// - `self` must own a live `ns_window`, which `&self` guarantees: the
    ///   wrapper's balancing `release` runs only from [`Drop`], and the
    ///   deferred path finds this window through the shared map, so a window
    ///   that closed first is absent from it rather than messaged.
    unsafe fn set_needs_display(&self, deferred: bool) {
        debug_assert!(
            super::owner_lane::on_owner_thread(self.owner, self.owner_is_main),
            "BUG: set_needs_display must run on the window's owner lane"
        );
        unsafe {
            // SAFETY: per the function's contract, the caller is on the owner
            // lane (re-checked by the assertion above) and `ns_window` is live.
            // The content view is NIL-checked before messaging.
            let content_view: ObjcId = msg_send![self.ns_window, contentView];
            if content_view == NIL {
                tracing::trace!(
                    deferred,
                    "request_redraw: window has no content view; request dropped"
                );
            } else {
                let _: () = msg_send![content_view, setNeedsDisplay: YES];
                // AppKit owns the display pass that turns this into a
                // `drawRect:` — nothing in this backend can observe it, so the
                // two ends of the pump are instrumented separately: this line
                // (the request went out) and `view.rs`'s `draw_rect` (AppKit
                // asked for content). A request with no matching `drawRect:` is
                // otherwise invisible from here, so when a trace subscriber is
                // attached, report the AppKit state the display pass is gated
                // on. The reads are msg_sends, so they are behind the level
                // check rather than unconditional fields.
                if tracing::enabled!(tracing::Level::TRACE) {
                    let window_visible: bool = msg_send![self.ns_window, isVisible];
                    let occlusion: usize = msg_send![self.ns_window, occlusionState];
                    let needs_display: bool = msg_send![content_view, needsDisplay];
                    let view_window: ObjcId = msg_send![content_view, window];
                    let is_key: bool = msg_send![self.ns_window, isKeyWindow];
                    let app: ObjcId = msg_send![class!(NSApplication), sharedApplication];
                    let app_active: bool = msg_send![app, isActive];
                    let policy: i64 = msg_send![app, activationPolicy];
                    let window_number: i64 = msg_send![self.ns_window, windowNumber];
                    let on_active_space: bool = msg_send![self.ns_window, isOnActiveSpace];
                    let screen: ObjcId = msg_send![self.ns_window, screen];
                    let alpha: f64 = msg_send![self.ns_window, alphaValue];
                    let level: i64 = msg_send![self.ns_window, level];
                    let view_hidden: bool = msg_send![content_view, isHiddenOrHasHiddenAncestor];
                    let view_frame: NSRect = msg_send![content_view, frame];
                    let backing_scale: f64 = msg_send![self.ns_window, backingScaleFactor];
                    tracing::trace!(
                        window_visible,
                        occlusion,
                        needs_display,
                        view_in_window = view_window != NIL,
                        is_key,
                        app_active,
                        policy,
                        window_number,
                        on_active_space,
                        has_screen = screen != NIL,
                        alpha,
                        level,
                        view_hidden,
                        vx = view_frame.origin.x,
                        vy = view_frame.origin.y,
                        vw = view_frame.size.width,
                        vh = view_frame.size.height,
                        backing_scale,
                        deferred,
                        "request_redraw: setNeedsDisplay sent to content view"
                    );
                }
            }
        }
    }

    /// Start the deferral of a redraw request onto the next turn of the owner
    /// lane. See [`Self::defer_redraw`] for the mechanism.
    fn defer_redraw_request(&self) {
        Self::defer_redraw(
            self.owner,
            self.owner_is_main,
            Arc::clone(&self.windows_map),
            self.ns_window as u64,
            0,
        );
    }

    /// Defer a redraw request to the next turn of the owner lane, delivering it
    /// there if that turn lands outside AppKit's display pass.
    ///
    /// The caller is inside AppKit's display pass of this window's view, where
    /// `setNeedsDisplay:` is discarded; this hands the same call to the lane so
    /// it lands after the pass has unwound instead of during it. This is
    /// ADR-0039 §4(c) realised on AppKit: the frame transaction is a region the
    /// drain gate is closed for, and a wake arriving while it is closed defers
    /// rather than draining.
    ///
    /// The body must NOT ride `route_on_owner`'s inline arm, which is where a
    /// call from inside `drawRect:` otherwise lands — a display pass runs on
    /// the owner thread, so `exec_on_owner` takes its main-thread shortcut, and
    /// the `setNeedsDisplay:` would be issued *during* the pass: the exact call
    /// AppKit discards, i.e. the bug this path exists to fix. Hence
    /// asynchronous, and never blocking. A synchronous dispatch would also
    /// self-deadlock on a serial *test* lane dispatching to itself; on the
    /// main-lane production path it would not hang — it would silently do the
    /// wrong thing, which is worse.
    ///
    /// The turn reaches the window through the shared window map rather than
    /// capturing a raw AppKit pointer, so a window that closed before the turn
    /// runs is simply no longer found — no dangling `id`, and no `Weak` cycle
    /// for the window to hold on itself. Two honest limits of that scheme:
    ///
    /// - The key is the NSWindow pointer — the pointer-as-id ADR-0039 §4 already
    ///   names as an ABA hazard for multi-window sessions. Address reuse cannot
    ///   make this message a *dead* object (the wrapper's balancing `release`
    ///   is queued on this same lane and runs only from [`Drop`], so a free
    ///   always follows any block already queued ahead of it), but it could in
    ///   principle make it message a *different* live window: one spurious
    ///   redraw, and the reason that ADR's monotonic-mint follow-up is the real
    ///   fix rather than this lookup.
    /// - Membership in the map is the whole test, so a window removed from the
    ///   map while still open (only a test harness does that today) drops a
    ///   deferred request that the inline path — which messages `self.ns_window`
    ///   directly — would have delivered.
    ///
    /// `hop` counts the turns this request has already taken; see the re-check
    /// below for why it exists and [`MAX_DEFER_HOPS`] for why it is bounded.
    fn defer_redraw(
        owner: &'static dispatch::Queue,
        owner_is_main: bool,
        windows_map: Arc<Mutex<HashMap<u64, Arc<MacOSWindow>>>>,
        key: u64,
        hop: u8,
    ) {
        super::owner_lane::exec_async_guarded(owner, move || {
            // The hop onto the lane is asynchronous, but the AppKit message
            // inside it is still routed: `exec_async_guarded` installed the lane
            // guard before this body, so `route_on_owner` runs it inline and
            // records its witness. One routed throat for every AppKit-messaging
            // body, with no second door beside it.
            route_on_owner(owner, owner_is_main, || {
                // This turn is supposed to run after the pass has unwound, and
                // that is what a lane turn normally is: the run loop services
                // the lane at its top level, after AppKit's display pass has
                // returned. The premise is only as strong as the drain, though.
                // A display pass that opens a nested run loop — a modal panel, a
                // drag session, a menu track — services this same lane from
                // *inside* itself, so a turn can land in a pass after all, and
                // messaging AppKit from there would discard the request again,
                // silently: a discarded deferral leaves the same trace as a
                // working one. So the turn re-checks and re-defers rather than
                // trusting the timing.
                if super::display_pass::in_display_pass() {
                    if hop < MAX_DEFER_HOPS {
                        tracing::trace!(
                            hop,
                            "request_redraw: the deferred turn landed inside another display \
                             pass (a nested run loop is draining the owner lane); deferring again"
                        );
                        Self::defer_redraw(owner, owner_is_main, windows_map, key, hop + 1);
                    } else {
                        // Dropped rather than sent: a `setNeedsDisplay:` issued
                        // here is the call AppKit throws away, so sending it
                        // would only produce a misleading "sent" trace. The pump
                        // stays stalled until the nested loop returns and
                        // something requests a redraw again, which is the
                        // residual this backend has anyway — the warning names
                        // the situation instead of leaving it to look like a
                        // healthy request.
                        tracing::warn!(
                            hop = MAX_DEFER_HOPS,
                            "request_redraw: a redraw re-arm is still inside a display pass \
                             after the deferral budget; dropping it. A nested run loop is \
                             draining the owner lane from inside a display pass, so no turn of \
                             that lane can land outside one; frames stay stalled until that \
                             loop returns and something requests a redraw again."
                        );
                    }
                    return;
                }
                // The lookup clones the `Arc` and drops the guard with the
                // statement: the map lock is never held across an AppKit call.
                let window = windows_map.lock().get(&key).cloned();
                if let Some(window) = window {
                    // SAFETY: `route_on_owner` ran this body on the owner lane,
                    // and finding the wrapper in the map means its NSWindow is
                    // live (the entry is removed before the wrapper's release).
                    unsafe { window.set_needs_display(true) };
                } else {
                    // Closed (or never in the map) before the deferred turn:
                    // the request has nothing left to redraw, which is not an
                    // error.
                    tracing::trace!(
                        "request_redraw: window closed before the deferred request ran; request dropped"
                    );
                }
            });
        });
    }
}

/// How many owner-lane turns a deferred redraw request may take before it is
/// dropped ([`MacOSWindow::defer_redraw`]).
///
/// One turn is the point of the deferral, so a value above 1 only exists for
/// the nested-run-loop case: each retry re-queues onto a lane that is being
/// drained from inside a display pass, and a lane drained that way hands every
/// retry straight back into a pass. The budget therefore has to be small — it
/// trades a bounded number of discarded turns for the log line that says the
/// nest exists, where an unbounded retry would spin at the lane's full rate for
/// as long as the nested loop runs.
const MAX_DEFER_HOPS: u8 = 4;

impl PlatformWindow for MacOSWindow {
    fn id(&self) -> WindowId {
        WindowId(self.ns_window as u64)
    }

    /// The window's own NSAccessibility bridge — the capability the
    /// composition root's accessibility wire discovers. Without this
    /// override the trait default (`None`) leaves every real macOS window
    /// silently invisible to VoiceOver.
    #[cfg(feature = "a11y")]
    fn accessibility(&self) -> Option<Arc<dyn crate::traits::PlatformAccessibility>> {
        self.accessibility
            .get()
            .map(|bridge| Arc::clone(bridge) as _)
    }

    /// The window's IME capability — the `NSTextInputClient` conformance the
    /// content view carries (see `super::text_input`). Without this override
    /// the trait default (`None`) leaves the native backend with no IME at all
    /// while the optional winit fallback has one.
    ///
    /// Built once and cached, like the accessibility bridge above, but lazily
    /// rather than in the constructor: the capability needs nothing the
    /// constructor has not already produced by the time the window value
    /// exists, and a window that never attaches a text input never builds one.
    /// `closed` is shared rather than copied so the capability observes the
    /// window's own close.
    fn text_input(&self) -> Option<Arc<dyn crate::traits::PlatformTextInput>> {
        let text_input = self.text_input.get_or_init(|| {
            Arc::new(super::text_input::MacOSTextInput::new(
                self.ns_window.cast::<objc2::runtime::AnyObject>(),
                Arc::clone(&self.closed),
                self.owner,
                self.owner_is_main,
            ))
        });
        Some(Arc::clone(text_input) as Arc<dyn crate::traits::PlatformTextInput>)
    }

    fn physical_size(&self) -> Size<DevicePixels> {
        let state = self.state.lock();
        let logical = state.bounds.size;
        let scale = state.scale_factor as f32;
        Size::new(
            flui_types::geometry::device_px((logical.width.0 * scale).round() as i32),
            flui_types::geometry::device_px((logical.height.0 * scale).round() as i32),
        )
    }

    fn logical_size(&self) -> Size<Pixels> {
        let state = self.state.lock();
        state.bounds.size
    }

    fn scale_factor(&self) -> f64 {
        let state = self.state.lock();
        state.scale_factor
    }

    fn refresh_period(&self) -> Option<Duration> {
        self.state.lock().refresh_period
    }

    /// # Thread affinity — owner-routed
    ///
    /// This method is called from whatever thread completed a future, through
    /// the scheduler's `on_frame_scheduled` hook (issue #949), so its AppKit
    /// body — `contentView` + `setNeedsDisplay:` — is dispatched onto the
    /// window's owner lane (the AppKit main queue for production windows, a
    /// caller-supplied serial lane for test windows; see `super::owner_lane`)
    /// and executes on the owner thread, never bare on a caller on another
    /// thread.
    ///
    /// It was the FIRST mechanically-routed AppKit message site in this file;
    /// issue #1194 swept the whole public window surface, so the routing is no
    /// longer this method's distinction: every class-A body — the 12
    /// `PlatformWindow` mutators, this method, the 12 `WindowTrait` bodies, and
    /// the 11 `MacOSWindowExtTrait` bodies — travels through the same
    /// `route_on_owner` throat, and the one body that has to arrive a lane turn
    /// later (the deferral below) is wrapped in it too, so it is witnessed like
    /// the rest. What is not on that surface is sound for the reasons the
    /// `unsafe impl Send`/`Sync` SAFETY block states (AppKit-delivered
    /// callbacks, upstream-enforced raw-handle accessors, the fire-and-forget
    /// `Drop` tail).
    ///
    /// What the routing does NOT provide is the wake relay ADR-0045 decision 5
    /// mandates end-to-end: the hook still calls this method directly. A relay
    /// (the hook posts, the owner thread drains) remains scoped to the
    /// `PlatformProxy` redraw verb (#559).
    fn request_redraw(&self) {
        // A frame is coming, so the platform's wake pump should look at the
        // deadline that frame is about to arm. Before the routing below, not
        // after: the deferred arm is a lane turn away, and the pump needs to
        // be scheduled no later than the frame itself.
        self.arm_wake_pump();
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        dispatch_redraw_request(
            // The deferral re-derives everything it needs from `self` because
            // it runs a lane turn later, after this call has returned — it
            // cannot borrow anything of this frame's.
            || self.defer_redraw_request(),
            || {
                route_on_owner(owner, owner_is_main, || unsafe {
                    // SAFETY: the body runs on the window's owner lane — inline
                    // on the OS main thread for a main-lane owner, or dispatched
                    // onto the lane under the reentrancy guard.
                    self.set_needs_display(false);
                });
            },
        );
    }

    fn is_focused(&self) -> bool {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for
            // a main-lane owner, or dispatched onto the lane under the
            // reentrancy guard — before the message is sent.
            let is_key: bool = msg_send![self.ns_window, isKeyWindow];
            is_key
        })
    }

    fn is_visible(&self) -> bool {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // message is sent.
            let is_visible: bool = msg_send![self.ns_window, isVisible];
            is_visible
        })
    }

    fn bounds(&self) -> Bounds<Pixels> {
        self.state.lock().bounds
    }

    fn get_title(&self) -> String {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive; `title` returns an autoreleased
            // NSString whose UTF8String buffer is copied before returning; the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // message is sent.
            let ns_title: ObjcId = msg_send![self.ns_window, title];
            if ns_title == NIL {
                return String::new();
            }
            let c_str: *const i8 = msg_send![ns_title, UTF8String];
            if c_str.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(c_str)
                    .to_string_lossy()
                    .into_owned()
            }
        })
    }

    fn set_title(&self, title: &str) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive; the NSString is created from a
            // valid Rust string and ownership passes to the window; the body
            // runs on the owner thread — inline on the OS main thread for a
            // main-lane owner, or dispatched onto the lane under the reentrancy
            // guard — before the message is sent.
            let ns_title = NSString::from_str(title);
            let _: () = msg_send![self.ns_window, setTitle: &*ns_title];
        });
    }

    fn activate(&self) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // message is sent.
            let _: () = msg_send![self.ns_window, makeKeyAndOrderFront: NIL];
        });
    }

    fn minimize(&self) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // message is sent.
            let _: () = msg_send![self.ns_window, miniaturize: NIL];
        });
    }

    fn maximize(&self) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // messages are sent.
            let is_zoomed: bool = msg_send![self.ns_window, isZoomed];
            if !is_zoomed {
                let _: () = msg_send![self.ns_window, zoom: NIL];
            }
        });
    }

    fn restore(&self) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // messages are sent.
            let is_minimized: bool = msg_send![self.ns_window, isMiniaturized];
            if is_minimized {
                let _: () = msg_send![self.ns_window, deminiaturize: NIL];
            }
            let is_zoomed: bool = msg_send![self.ns_window, isZoomed];
            if is_zoomed {
                let _: () = msg_send![self.ns_window, zoom: NIL];
            }
        });
    }

    fn toggle_fullscreen(&self) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // message is sent.
            let _: () = msg_send![self.ns_window, toggleFullScreen: NIL];
        });
    }

    fn resize(&self, size: Size<Pixels>) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // message is sent.
            // The stored size tracks the *content* area (see `handle_resize`
            // using `contentRectForFrameRect:`), so resize the content, not
            // the frame — on decorated windows a frame-sized `setFrame:`
            // would shrink the content by the titlebar height.
            let ns_size = NSSize::new(size.width.0 as f64, size.height.0 as f64);
            let _: () = msg_send![self.ns_window, setContentSize: ns_size];

            // Update state
            let mut state = self.state.lock();
            state.bounds.size = size;
        });
    }

    fn close(&self) {
        // No separate flag-set needed here: AppKit's `-[NSWindow close]`
        // posts `NSWindowWillCloseNotification` (routing to the delegate's
        // `windowWillClose:`, and so to `handle_close` below) whether or
        // not `windowShouldClose:` was ever asked — the same notification a
        // user-initiated close takes. `handle_close` is therefore reached
        // by every route through this window's own `close`, and is where
        // `closed` is actually set.
        //
        // The routed programmatic close does not change close-request callback
        // delivery: the should-close consultation round-trips through AppKit's
        // `windowShouldClose:` delegate (delivered on main), which vetoes a
        // wrong-thread delivery before invoking the handler.
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // message is sent.
            let _: () = msg_send![self.ns_window, close];
        });
    }

    // The explicit `arrowCursor` list below is intentional documentation of
    // which icons resolve to the default arrow; it is textually identical to
    // the `_` arm's body, which objc2's `msg_send!` expansion makes clippy
    // visible (objc 0.2's differing expansion hid it).
    #[allow(clippy::match_same_arms)]
    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            self.state.lock().cursor = cursor;

            // SAFETY: every selector below is an NSCursor class constructor
            // available on the supported macOS baseline. The returned singleton
            // remains owned by AppKit and `set` only selects it for the current
            // pointer location. The body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the messages are sent.
            let ns_cursor: ObjcId = match cursor {
                CursorIcon::ContextMenu => msg_send![class!(NSCursor), contextualMenuCursor],
                CursorIcon::Pointer => msg_send![class!(NSCursor), pointingHandCursor],
                CursorIcon::Cell | CursorIcon::Crosshair => {
                    msg_send![class!(NSCursor), crosshairCursor]
                }
                CursorIcon::Text => msg_send![class!(NSCursor), IBeamCursor],
                CursorIcon::VerticalText => {
                    msg_send![class!(NSCursor), IBeamCursorForVerticalLayout]
                }
                CursorIcon::Copy => msg_send![class!(NSCursor), dragCopyCursor],
                CursorIcon::Move | CursorIcon::Grabbing => {
                    msg_send![class!(NSCursor), closedHandCursor]
                }
                CursorIcon::Grab => msg_send![class!(NSCursor), openHandCursor],
                CursorIcon::NoDrop | CursorIcon::NotAllowed => {
                    msg_send![class!(NSCursor), operationNotAllowedCursor]
                }
                CursorIcon::EResize
                | CursorIcon::WResize
                | CursorIcon::EwResize
                | CursorIcon::ColResize => {
                    msg_send![class!(NSCursor), resizeLeftRightCursor]
                }
                CursorIcon::NResize
                | CursorIcon::SResize
                | CursorIcon::NsResize
                | CursorIcon::RowResize => {
                    msg_send![class!(NSCursor), resizeUpDownCursor]
                }
                CursorIcon::Default
                | CursorIcon::Help
                | CursorIcon::Progress
                | CursorIcon::Wait
                | CursorIcon::Alias
                | CursorIcon::NeResize
                | CursorIcon::NwResize
                | CursorIcon::SeResize
                | CursorIcon::SwResize
                | CursorIcon::NeswResize
                | CursorIcon::NwseResize
                | CursorIcon::AllScroll
                | CursorIcon::ZoomIn
                | CursorIcon::ZoomOut
                | CursorIcon::DndAsk => msg_send![class!(NSCursor), arrowCursor],
                _ => msg_send![class!(NSCursor), arrowCursor],
            };
            if ns_cursor == NIL {
                return Err(CursorError::Backend(
                    "AppKit returned a null NSCursor".to_string(),
                ));
            }
            let _: () = msg_send![ns_cursor, set];
            Ok(())
        })
    }

    // ==================== Callback Registration ====================

    crate::shared::impl_window_callback_setters!(callbacks);

    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        HasWindowHandle::window_handle(self)
    }

    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        HasDisplayHandle::display_handle(self)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// Implement raw-window-handle for wgpu integration
impl HasWindowHandle for MacOSWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        use std::ptr::NonNull;

        // Refuse once the window has closed (see `handle_close`'s own doc
        // for when `closed` is set). With `setReleasedWhenClosed: NO` set
        // at construction, `-[NSWindow close]` no longer deallocates
        // `ns_window`, so `contentView` below would otherwise keep
        // answering long after the window is meaningless to hand a GPU
        // handle for — a caller re-acquiring a handle from a retained
        // `Arc<dyn PlatformWindow>` (issue #1043's recovery path) needs
        // this flag, not a NIL check, to learn the window is gone.
        if self.closed.load(Ordering::SeqCst) {
            return Err(raw_window_handle::HandleError::Unavailable);
        }

        // raw-window-handle 0.6 AppKitWindowHandle expects the NSView, not
        // the NSWindow.
        // SAFETY: `ns_window` is alive for the lifetime of `self`; the
        // returned handle borrows `self`, so the view outlives it.
        let content_view: ObjcId = unsafe { msg_send![self.ns_window, contentView] };
        let ns_view = NonNull::new(content_view.cast::<std::ffi::c_void>())
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = AppKitWindowHandle::new(ns_view);

        // SAFETY: the handle is valid for the lifetime of `&self` (the view
        // is retained by the window, which `self` keeps alive).
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(RawWindowHandle::AppKit(handle)) })
    }
}

impl HasDisplayHandle for MacOSWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        let handle = AppKitDisplayHandle::new();
        // SAFETY: AppKit display handles carry no pointer; always valid.
        Ok(unsafe {
            raw_window_handle::DisplayHandle::borrow_raw(RawDisplayHandle::AppKit(handle))
        })
    }
}

impl Clone for MacOSWindow {
    fn clone(&self) -> Self {
        Self {
            ns_window: self.ns_window,
            state: Arc::clone(&self.state),
            windows_map: Arc::clone(&self.windows_map),
            callbacks: Arc::clone(&self.callbacks),
            closed: Arc::clone(&self.closed),
            config: self.config.clone(),
            // The lane is a property of the window, not of any one handle:
            // every clone of this wrapper routes through the same owner.
            owner: self.owner,
            owner_is_main: self.owner_is_main,
            // The clone shares the same window, so it shares the same
            // NSAccessibility bridge — one subclass per content view,
            // never two (`OnceLock<Arc<_>>` clones the shared handle).
            #[cfg(feature = "a11y")]
            accessibility: self.accessibility.clone(),
            // A clone shares the window, so it arms the same platform pump —
            // a redraw asked for through any handle must schedule the wake
            // deadline's landing place, or a clone would be a way to redraw
            // without the loop being able to come back.
            wake_pump: self.wake_pump.clone(),
            // A clone shares the window, so it is the same capability — one
            // `NSTextInputClient` view behind both handles, never two.
            text_input: self.text_input.clone(),
        }
    }
}

/// An Objective-C object id owned by the drop-tail closure and only ever
/// messaged ON its owner lane.
struct OwnerLaneId(ObjcId);

// SAFETY: `id` is `*mut Object`; a raw pointer is `!Send` because it generally
// carries no ownership or thread guarantee. This use provides both. The wrapper
// OWNS one outstanding retain (the window's balancing `release` is the ONLY
// message ever sent through it), and it is consumed either ON the owner lane —
// the object's home thread, where it was created — or dropped un-run, leaving
// the object over-retained (a leak, never a use-after-free). No code outside
// the owner lane ever dereferences it. This is the same ownership + affinity
// justification the file already makes for `unsafe impl Send for MacOSWindow {}`,
// scoped down to a single owned message.
unsafe impl Send for OwnerLaneId {}

impl OwnerLaneId {
    /// Send the wrapper's one balancing `release` message. Consumes the
    /// wrapper (so the caller's capture is the whole `OwnerLaneId`, a `Send`
    /// value, rather than its raw field).
    ///
    /// # Safety
    /// `self.0` must reference a live window that still holds this wrapper's
    /// outstanding retain, and the send must execute ON the owner lane — the
    /// object's home thread — under the lane guard.
    unsafe fn send_release(self) {
        // SAFETY: the fn-level safety contract — live window, outstanding
        // retain, owner-lane call — is exactly the precondition `msg_send!`
        // requires of its receiver.
        unsafe {
            let _: () = msg_send![self.0, release];
        }
    }
}

impl Drop for MacOSWindow {
    fn drop(&mut self) {
        // Last-clone gate: ONLY the final `Arc` wrapper clone owns teardown.
        // Every earlier clone drop returns with no work — and MUST: a live
        // clone's accessibility bridge is still in service, and only one
        // balancing `release` may ever be sent (an unconditional tail from
        // every clone would over-dealloc a live window).
        //
        // The gate IS this proxy: `self.state`'s `Arc` is held 1:1 by wrapper
        // clones — the ONLY clone site is `Clone for MacOSWindow` — so
        // `strong_count(&self.state) == 1` holds exactly when this is the
        // last wrapper clone. A future that clones the state `Arc`
        // independently (not through the wrapper) would silently degrade
        // teardown into a permanent leak — safe in direction, but silent; the
        // invariant is documented here so the gate is understood as the proxy
        // it is.
        if Arc::strong_count(&self.state) != 1 {
            return;
        }
        tracing::debug!("Closing NSWindow {:p}", self.ns_window);

        // Rust-only clean-up, never routed (no AppKit contact, no lane).
        let window_id = self.ns_window as u64;
        let _prev = self.windows_map.lock().remove(&window_id);

        // The AppKit tail, dispatched ONTO the owner lane and NOT awaited:
        // `Drop` never blocks, so teardown cannot hang (by construction),
        // and the closure OWNS every capture — nothing borrowed from this
        // dying value crosses the lane (the `'static` bound on
        // `exec_async_guarded`). If the lane is un-servicing (pre-`run`, or
        // the run loop is gone) the closure never runs and both halves fall
        // out soundly: the NSWindow is left over-retained — never released,
        // so no dealloc ever runs off-main — and the a11y adapter leaks +
        // warns via its own existing off-owner fallback.
        let owner = self.owner;
        let ns_window = OwnerLaneId(self.ns_window);
        #[cfg(feature = "a11y")]
        let a11y = self.accessibility.get().cloned(); // Option<Arc<...>>, Send
        super::owner_lane::exec_async_guarded(owner, move || unsafe {
            // SAFETY: the tail runs under the owner-lane guard — on-lane — so
            // the a11y unhook and the Window release both execute
            // owner-affine. `shutdown()` runs before the release in the same
            // block, preserving accesskit's documented precondition (unhook
            // the dynamic subclass while the content view is still alive).
            #[cfg(feature = "a11y")]
            if let Some(a11y) = a11y {
                a11y.shutdown();
            }
            // SAFETY: `send_release` consumes the whole `OwnerLaneId` (the
            // closure's capture is then that `Send` newtype, never its raw
            // field), and its own contract — live still-retained window, send
            // on the owner lane — is met here: the wrapper was built from this
            // value's `ns_window` before the last clone dropped, and this body
            // runs under the owner-lane guard.
            ns_window.send_release();
        });
    }
}

// ============================================================================
// Cross-Platform Window Trait Implementation
// ============================================================================

use crate::window::{
    RawWindowHandle as CrossRawWindowHandle, Window as WindowTrait, WindowId as CrossWindowId,
    WindowState,
};

impl WindowTrait for MacOSWindow {
    fn id(&self) -> CrossWindowId {
        CrossWindowId::new(self.ns_window as u64)
    }

    fn title(&self) -> String {
        PlatformWindow::get_title(self)
    }

    fn set_title(&mut self, title: &str) {
        PlatformWindow::set_title(self, title);
    }

    fn position(&self) -> Point<Pixels> {
        let state = self.state.lock();
        state.bounds.origin
    }

    fn set_position(&mut self, position: Point<Pixels>) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = &*self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the messages are sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            let frame: NSRect = msg_send![this.ns_window, frame];
            let new_frame = NSRect::new(
                NSPoint::new(position.x.0 as f64, position.y.0 as f64),
                frame.size,
            );
            let _: () = msg_send![this.ns_window, setFrame: new_frame display: YES];

            // Update state
            let mut state = this.state.lock();
            state.bounds.origin = position;
        });
    }

    fn size(&self) -> Size<Pixels> {
        let state = self.state.lock();
        state.bounds.size
    }

    fn set_size(&mut self, size: Size<Pixels>) {
        PlatformWindow::resize(self, size);
    }

    fn state(&self) -> WindowState {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // messages are sent.
            let is_minimized: bool = msg_send![self.ns_window, isMiniaturized];
            let is_zoomed: bool = msg_send![self.ns_window, isZoomed];
            let style_mask: NSWindowStyleMask = msg_send![self.ns_window, styleMask];

            if is_minimized {
                WindowState::Minimized
            } else if style_mask.contains(NSWindowStyleMask::FullScreen) {
                WindowState::Fullscreen
            } else if is_zoomed {
                WindowState::Maximized
            } else {
                WindowState::Normal
            }
        })
    }

    fn set_state(&mut self, state: WindowState) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = &*self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the messages are sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            match state {
                WindowState::Normal => {
                    // Restore from minimized
                    let is_minimized: bool = msg_send![this.ns_window, isMiniaturized];
                    if is_minimized {
                        let _: () = msg_send![this.ns_window, deminiaturize: NIL];
                    }

                    // Restore from maximized
                    let is_zoomed: bool = msg_send![this.ns_window, isZoomed];
                    if is_zoomed {
                        let _: () = msg_send![this.ns_window, zoom: NIL];
                    }

                    // Exit fullscreen
                    let style_mask: NSWindowStyleMask = msg_send![this.ns_window, styleMask];
                    if style_mask.contains(NSWindowStyleMask::FullScreen) {
                        let _: () = msg_send![this.ns_window, toggleFullScreen: NIL];
                    }
                }
                WindowState::Minimized => {
                    let _: () = msg_send![this.ns_window, miniaturize: NIL];
                }
                WindowState::Maximized => {
                    // First restore from minimized if needed
                    let is_minimized: bool = msg_send![this.ns_window, isMiniaturized];
                    if is_minimized {
                        let _: () = msg_send![this.ns_window, deminiaturize: NIL];
                    }

                    // Then zoom (maximize)
                    let is_zoomed: bool = msg_send![this.ns_window, isZoomed];
                    if !is_zoomed {
                        let _: () = msg_send![this.ns_window, zoom: NIL];
                    }
                }
                WindowState::Fullscreen => {
                    let style_mask: NSWindowStyleMask = msg_send![this.ns_window, styleMask];
                    if !style_mask.contains(NSWindowStyleMask::FullScreen) {
                        let _: () = msg_send![this.ns_window, toggleFullScreen: NIL];
                    }
                }
            }
        });
    }

    fn is_visible(&self) -> bool {
        PlatformWindow::is_visible(self)
    }

    fn set_visible(&mut self, visible: bool) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = &*self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            if visible {
                let _: () = msg_send![this.ns_window, makeKeyAndOrderFront: NIL];
            } else {
                let _: () = msg_send![this.ns_window, orderOut: NIL];
            }
        });
    }

    fn is_resizable(&self) -> bool {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // message is sent.
            let style_mask: NSWindowStyleMask = msg_send![self.ns_window, styleMask];
            style_mask.contains(NSWindowStyleMask::Resizable)
        })
    }

    fn set_resizable(&mut self, resizable: bool) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = &*self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            let mut style_mask: NSWindowStyleMask = msg_send![this.ns_window, styleMask];
            if resizable {
                style_mask |= NSWindowStyleMask::Resizable;
            } else {
                style_mask &= !NSWindowStyleMask::Resizable;
            }
            let _: () = msg_send![this.ns_window, setStyleMask: style_mask];
        });
    }

    fn is_minimizable(&self) -> bool {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // message is sent.
            let style_mask: NSWindowStyleMask = msg_send![self.ns_window, styleMask];
            style_mask.contains(NSWindowStyleMask::Miniaturizable)
        })
    }

    fn set_minimizable(&mut self, minimizable: bool) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = &*self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            let mut style_mask: NSWindowStyleMask = msg_send![this.ns_window, styleMask];
            if minimizable {
                style_mask |= NSWindowStyleMask::Miniaturizable;
            } else {
                style_mask &= !NSWindowStyleMask::Miniaturizable;
            }
            let _: () = msg_send![this.ns_window, setStyleMask: style_mask];
        });
    }

    fn is_closable(&self) -> bool {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // message is sent.
            let style_mask: NSWindowStyleMask = msg_send![self.ns_window, styleMask];
            style_mask.contains(NSWindowStyleMask::Closable)
        })
    }

    fn set_closable(&mut self, closable: bool) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = &*self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            let mut style_mask: NSWindowStyleMask = msg_send![this.ns_window, styleMask];
            if closable {
                style_mask |= NSWindowStyleMask::Closable;
            } else {
                style_mask &= !NSWindowStyleMask::Closable;
            }
            let _: () = msg_send![this.ns_window, setStyleMask: style_mask];
        });
    }

    fn focus(&mut self) {
        PlatformWindow::activate(self);
    }

    fn is_focused(&self) -> bool {
        PlatformWindow::is_focused(self)
    }

    fn close(&mut self) {
        PlatformWindow::close(self);
    }

    fn request_redraw(&mut self) {
        PlatformWindow::request_redraw(self);
    }

    fn set_min_size(&mut self, size: Option<Size<Pixels>>) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = &*self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            if let Some(size) = size {
                let ns_size = NSSize::new(size.width.0 as f64, size.height.0 as f64);
                let _: () = msg_send![this.ns_window, setMinSize: ns_size];
            } else {
                // Set to zero to remove constraint
                let ns_size = NSSize::new(0.0, 0.0);
                let _: () = msg_send![this.ns_window, setMinSize: ns_size];
            }
        });
    }

    fn set_max_size(&mut self, size: Option<Size<Pixels>>) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = &*self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            if let Some(size) = size {
                let ns_size = NSSize::new(size.width.0 as f64, size.height.0 as f64);
                let _: () = msg_send![this.ns_window, setMaxSize: ns_size];
            } else {
                // Set to max to remove constraint
                let ns_size = NSSize::new(f64::MAX, f64::MAX);
                let _: () = msg_send![this.ns_window, setMaxSize: ns_size];
            }
        });
    }

    fn scale_factor(&self) -> f32 {
        let state = self.state.lock();
        state.scale_factor as f32
    }

    fn raw_window_handle(&self) -> CrossRawWindowHandle {
        // SAFETY: `ns_window` is alive for the lifetime of `self`; the
        // returned raw pointers are opaque handles for GPU integration.
        unsafe {
            let content_view: ObjcId = msg_send![self.ns_window, contentView];
            CrossRawWindowHandle::MacOS {
                ns_view: content_view.cast::<std::ffi::c_void>(),
                ns_window: self.ns_window.cast::<std::ffi::c_void>(),
            }
        }
    }
}

// ============================================================================
// macOS Window Extension Trait Implementation
// ============================================================================

use super::{
    liquid_glass::{LiquidGlassConfig, LiquidGlassMaterial},
    window_ext::{
        MacOSCollectionBehavior, MacOSWindowExt as MacOSWindowExtTrait, MacOSWindowLevel,
    },
    window_tiling::TilingConfiguration,
};

impl MacOSWindowExtTrait for MacOSWindow {
    fn set_liquid_glass(&mut self, material: LiquidGlassMaterial) {
        // Create default config from material
        let config = LiquidGlassConfig::from_material(material);
        self.set_liquid_glass_config(config);
    }

    fn set_liquid_glass_config(&mut self, config: LiquidGlassConfig) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = &*self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows); NSVisualEffectView is alloc-init'ed and
            // ownership passes to the window via `setContentView:`; the body
            // runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // messages are sent.
            // Capturing `this` (a `&MacOSWindow`) rather than the raw-pointer
            // field is what keeps the closure `Send`: `&MacOSWindow: Send` via
            // the `unsafe impl Sync`.
            // Apply vibrancy effect to window content view
            let content_view: ObjcId = msg_send![this.ns_window, contentView];
            if content_view == NIL {
                tracing::warn!("Cannot apply Liquid Glass: content view is NIL");
                return;
            }

            // Create NSVisualEffectView
            let effect_view_class = class!(NSVisualEffectView);
            let effect_view: ObjcId = msg_send![effect_view_class, alloc];
            let effect_view: ObjcId = msg_send![effect_view, init];

            // Set frame to match content view
            let frame: NSRect = msg_send![content_view, frame];
            let _: () = msg_send![effect_view, setFrame: frame];

            // Set material (NSVisualEffectMaterial)
            let material_value: usize = config.material.to_ns_visual_effect_material();
            let _: () = msg_send![effect_view, setMaterial: material_value];

            // Set blending mode (NSVisualEffectBlendingMode)
            let blending_mode: usize = config.blending_mode.to_ns_blending_mode();
            let _: () = msg_send![effect_view, setBlendingMode: blending_mode];

            // Set state (NSVisualEffectState)
            let state: usize = 1; // NSVisualEffectStateActive
            let _: () = msg_send![effect_view, setState: state];

            // Enable autoresizing
            let autoresizing_mask: usize = (1 << 1) | (1 << 4); // NSViewWidthSizable | NSViewHeightSizable
            let _: () = msg_send![effect_view, setAutoresizingMask: autoresizing_mask];

            // Set as window content view
            let _: () = msg_send![this.ns_window, setContentView: effect_view];

            // Make window titlebar transparent if requested
            if config.transparent_titlebar {
                let style_mask: NSWindowStyleMask = msg_send![this.ns_window, styleMask];
                let new_style_mask = style_mask | NSWindowStyleMask::FullSizeContentView;
                let _: () = msg_send![this.ns_window, setStyleMask: new_style_mask];
                let _: () = msg_send![this.ns_window, setTitlebarAppearsTransparent: YES];
            }

            tracing::debug!("Applied Liquid Glass material: {:?}", config.material);
        });
    }

    fn clear_liquid_glass(&mut self) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive; the freshly created content view's
            // ownership passes to the window via `setContentView:`; the body
            // runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // messages are sent.
            let frame: NSRect = msg_send![self.ns_window, frame];

            // Remove visual effect view and restore normal content view
            let content_view = view::create_content_view(
                NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(frame.size.width, frame.size.height),
                ),
                PlatformWindow::scale_factor(self),
                Arc::downgrade(&self.callbacks),
            );

            let _: () = msg_send![self.ns_window, setContentView: content_view];

            // Restore titlebar appearance
            let _: () = msg_send![self.ns_window, setTitlebarAppearsTransparent: NO];

            tracing::debug!("Cleared Liquid Glass effect");
        });
    }

    fn enable_tiling(&mut self, config: TilingConfiguration) {
        // Native tiling API requires macOS 15+; until adopted, the
        // configuration is recorded for observability only.
        tracing::info!(
            "Window tiling enabled: position={:?}, ratio={}, layout={:?}",
            config.primary_position,
            config.split_ratio,
            config.layout
        );
    }

    fn disable_tiling(&mut self) {
        tracing::info!("Window tiling disabled");
    }

    fn is_tiling_enabled(&self) -> bool {
        // Native tiling API requires macOS 15+; not yet adopted.
        false
    }

    fn enable_tabbing(&mut self) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = &*self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            // Enable automatic tabbing (macOS 10.12+)
            let tabbing_mode: isize = 1; // NSWindowTabbingModeAutomatic
            let _: () = msg_send![this.ns_window, setTabbingMode: tabbing_mode];

            tracing::debug!("Window tabbing enabled");
        });
    }

    fn disable_tabbing(&mut self) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = &*self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            let tabbing_mode: isize = 2; // NSWindowTabbingModeDisallowed
            let _: () = msg_send![this.ns_window, setTabbingMode: tabbing_mode];

            tracing::debug!("Window tabbing disabled");
        });
    }

    fn add_tab_to_window(&mut self, other_window_id: u64) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = &*self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `other_window_id` round-trips an NSWindow pointer that
            // was handed out as a window id; nil is rejected before messaging;
            // the body runs on the owner thread — inline on the OS main thread
            // for a main-lane owner, or dispatched onto the lane under the
            // reentrancy guard — before the message is sent. Capturing `this`
            // (a `&MacOSWindow`) rather than the
            // raw-pointer field is what keeps the closure `Send`:
            // `&MacOSWindow: Send` via the `unsafe impl Sync`.
            // Window ids are NSWindow pointers (see `WindowTrait::id`)
            let other_ns_window = other_window_id as *mut Object;

            if other_ns_window == NIL {
                tracing::warn!("Cannot add tab: window {:?} not found", other_window_id);
            } else {
                let _: () = msg_send![this.ns_window, addTabbedWindow:other_ns_window ordered:0]; // NSWindowAbove
                tracing::debug!("Added tab to window {:p}", other_ns_window);
            }
        });
    }

    fn toggle_native_fullscreen(&mut self) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = &*self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            let _: () = msg_send![this.ns_window, toggleFullScreen: NIL];
            tracing::debug!("Toggled native fullscreen");
        });
    }

    fn set_window_level(&mut self, level: MacOSWindowLevel) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = &*self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            let level_value = level.to_ns_value();
            let _: () = msg_send![this.ns_window, setLevel: level_value];
            tracing::debug!("Set window level to {:?} ({})", level, level_value);
        });
    }

    fn window_level(&self) -> MacOSWindowLevel {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // message is sent.
            let level_value: isize = msg_send![self.ns_window, level];
            match level_value {
                0 => MacOSWindowLevel::Normal,
                3 => MacOSWindowLevel::Floating,
                8 => MacOSWindowLevel::ModalPanel,
                24 => MacOSWindowLevel::MainMenu,
                25 => MacOSWindowLevel::Status,
                101 => MacOSWindowLevel::PopUpMenu,
                1000 => MacOSWindowLevel::ScreenSaver,
                _ if level_value == isize::MAX - 1 => MacOSWindowLevel::FloatingPanel,
                _ => MacOSWindowLevel::Normal, // Default to normal for unknown values
            }
        })
    }

    fn set_collection_behavior(&mut self, behavior: MacOSCollectionBehavior) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = &*self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            let _: () = msg_send![this.ns_window, setCollectionBehavior: behavior.bits() as usize];
            tracing::debug!("Set collection behavior: {:?}", behavior);
        });
    }

    fn set_has_shadow(&mut self, has_shadow: bool) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = &*self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            let value: BObjC = if has_shadow { YES } else { NO };
            let _: () = msg_send![this.ns_window, setHasShadow: value];
            tracing::debug!("Set window shadow: {}", has_shadow);
        });
    }

    fn set_alpha(&mut self, alpha: f32) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = &*self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            let clamped_alpha = alpha.clamp(0.0, 1.0);
            let _: () = msg_send![this.ns_window, setAlphaValue: clamped_alpha as f64];
            tracing::debug!("Set window alpha: {}", clamped_alpha);
        });
    }

    fn backing_scale_factor(&self) -> f32 {
        let state = self.state.lock();
        state.scale_factor as f32
    }

    fn convert_point_from_backing(&self, point: Point<Pixels>) -> Point<Pixels> {
        let scale = self.backing_scale_factor();
        Point::new(Pixels(point.x.0 / scale), Pixels(point.y.0 / scale))
    }

    fn convert_point_to_backing(&self, point: Point<Pixels>) -> Point<Pixels> {
        let scale = self.backing_scale_factor();
        Point::new(Pixels(point.x.0 * scale), Pixels(point.y.0 * scale))
    }
}

// ============================================================================
// NSWindowDelegate Implementation
// ============================================================================

use std::sync::Weak;

/// The refresh period of the display a raw `NSScreen*` names, or `None` when
/// the pointer is nil or the display reports no rate.
///
/// # Safety
///
/// `screen` must be null or a live `NSScreen*`, and the call must run on the
/// owner lane (this messages the screen).
unsafe fn screen_refresh_period(screen: ObjcId) -> Option<Duration> {
    // SAFETY: the caller's contract — nil or a live `NSScreen*` on the owner
    // lane; the borrow is scoped to the `and_then` closure.
    let screen = unsafe { screen.as_ref() }?;
    // SAFETY: `NSScreen` and `AnyObject` share the Objective-C object layout,
    // and the caller guarantees the pointee really is an `NSScreen`.
    let screen: &objc2_app_kit::NSScreen =
        unsafe { &*std::ptr::from_ref::<AnyObject>(screen).cast() };
    refresh_period_for_screen(screen)
}

/// Create a window delegate for lifecycle events
fn create_window_delegate(window: Weak<MacOSWindow>) -> ObjcId {
    // SAFETY: the delegate class is registered before alloc/init; the boxed
    // Weak pointer stored in the ivar is reclaimed in the delegate's
    // `dealloc`, so it lives exactly as long as the delegate.
    unsafe {
        // Get or create delegate class
        let class = get_or_create_delegate_class();
        let delegate: ObjcId = msg_send![class, alloc];
        let delegate: ObjcId = msg_send![delegate, init];

        // Store weak pointer to window
        let window_ptr = Box::into_raw(Box::new(window)).cast::<std::ffi::c_void>();
        let ivar = class
            .instance_variable(c"window_ptr")
            .expect("BUG: FLUIWindowDelegate declares the window_ptr ivar");
        let slot: *mut *mut std::ffi::c_void = ivar.load_ptr(&*delegate);
        *slot = window_ptr;

        delegate
    }
}

/// Get or create the NSWindowDelegate class
fn get_or_create_delegate_class() -> &'static Class {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let superclass = objc2_foundation::NSObject::class();
        let mut decl = ClassBuilder::new(c"FLUIWindowDelegate", superclass)
            .expect("FLUIWindowDelegate must be registered exactly once (guarded by Once)");

        // Add ivar to store window pointer
        decl.add_ivar::<*mut std::ffi::c_void>(c"window_ptr");

        // windowDidResize:
        extern "C-unwind" fn window_did_resize(this: &Object, _sel: Sel, _notification: ObjcId) {
            // SAFETY: AppKit invokes delegate methods on live delegate objects.
            if let Some(window) = unsafe { get_window_from_delegate(this) } {
                window.handle_resize();
            }
        }

        // windowDidMove:
        extern "C-unwind" fn window_did_move(this: &Object, _sel: Sel, _notification: ObjcId) {
            // SAFETY: AppKit invokes delegate methods on live delegate objects.
            if let Some(window) = unsafe { get_window_from_delegate(this) } {
                window.handle_move();
            }
        }

        // windowDidBecomeKey:
        extern "C-unwind" fn window_did_become_key(
            this: &Object,
            _sel: Sel,
            _notification: ObjcId,
        ) {
            // SAFETY: AppKit invokes delegate methods on live delegate objects.
            if let Some(window) = unsafe { get_window_from_delegate(this) } {
                window.handle_focus_gained();
            }
        }

        // windowDidResignKey:
        extern "C-unwind" fn window_did_resign_key(
            this: &Object,
            _sel: Sel,
            _notification: ObjcId,
        ) {
            // SAFETY: AppKit invokes delegate methods on live delegate objects.
            if let Some(window) = unsafe { get_window_from_delegate(this) } {
                window.handle_focus_lost();
            }
        }

        // windowShouldClose:
        extern "C-unwind" fn window_should_close(
            this: &Object,
            _sel: Sel,
            _sender: ObjcId,
        ) -> BObjC {
            // SAFETY: AppKit invokes delegate methods on live delegate objects.
            if let Some(window) = unsafe { get_window_from_delegate(this) } {
                if window.handle_close_request() {
                    YES
                } else {
                    NO
                }
            } else {
                YES
            }
        }

        // windowWillClose:
        extern "C-unwind" fn window_will_close(this: &Object, _sel: Sel, _notification: ObjcId) {
            // SAFETY: AppKit invokes delegate methods on live delegate objects.
            if let Some(window) = unsafe { get_window_from_delegate(this) } {
                window.handle_close();
            }
        }

        // windowDidChangeBackingProperties: (Retina/DPI change)
        extern "C-unwind" fn window_did_change_backing_properties(
            this: &Object,
            _sel: Sel,
            _notification: ObjcId,
        ) {
            // SAFETY: AppKit invokes delegate methods on live delegate objects.
            if let Some(window) = unsafe { get_window_from_delegate(this) } {
                window.handle_backing_properties_changed();
            }
        }

        // windowDidChangeScreen: (moved to different monitor)
        extern "C-unwind" fn window_did_change_screen(
            this: &Object,
            _sel: Sel,
            _notification: ObjcId,
        ) {
            // SAFETY: AppKit invokes delegate methods on live delegate objects.
            if let Some(window) = unsafe { get_window_from_delegate(this) } {
                window.handle_screen_changed();
            }
        }

        // windowDidChangeOcclusionState: (hidden-surface gating — fires on
        // full occlusion by other windows, miniaturization, hide/unhide,
        // and space switches; the visible bit is the surface-composed
        // claim the runtime's FrameClock gate consumes)
        extern "C-unwind" fn window_did_change_occlusion_state(
            this: &Object,
            _sel: Sel,
            _notification: ObjcId,
        ) {
            // SAFETY: AppKit invokes delegate methods on live delegate objects.
            if let Some(window) = unsafe { get_window_from_delegate(this) } {
                window.handle_occlusion_state_changed();
            }
        }

        // dealloc — reclaim the boxed Weak<MacOSWindow>
        extern "C-unwind" fn delegate_dealloc(this: &Object, _sel: Sel) {
            // SAFETY: the ivar holds either null or a Box<Weak<MacOSWindow>>
            // leaked in `create_window_delegate`; reclaiming it exactly once
            // on dealloc is the matching release. The super dealloc message
            // is the mandatory NSObject teardown.
            unsafe {
                let window_ptr: *mut std::ffi::c_void = *this.get_ivar("window_ptr");
                if !window_ptr.is_null() {
                    drop(Box::from_raw(window_ptr.cast::<Weak<MacOSWindow>>()));
                }
                let superclass = class!(NSObject);
                let _: () = msg_send![super(this, superclass), dealloc];
            }
        }

        // Add methods
        // SAFETY: every registered function pointer matches the Objective-C
        // method signature of its selector, as required by
        // `ClassDecl::add_method`.
        unsafe {
            decl.add_method(
                sel!(windowDidResize:),
                window_did_resize as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(windowDidMove:),
                window_did_move as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(windowDidBecomeKey:),
                window_did_become_key as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(windowDidResignKey:),
                window_did_resign_key as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(windowShouldClose:),
                window_should_close as extern "C-unwind" fn(_, _, _) -> Bool,
            );
            decl.add_method(
                sel!(windowWillClose:),
                window_will_close as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(windowDidChangeBackingProperties:),
                window_did_change_backing_properties as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(windowDidChangeScreen:),
                window_did_change_screen as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(windowDidChangeOcclusionState:),
                window_did_change_occlusion_state as extern "C-unwind" fn(_, _, _),
            );
            decl.add_method(
                sel!(dealloc),
                delegate_dealloc as extern "C-unwind" fn(_, _),
            );
        }

        decl.register();
    });

    AnyClass::get(c"FLUIWindowDelegate")
        .expect("FLUIWindowDelegate was registered by the Once block above")
}

/// Get window from delegate
///
/// # Safety
///
/// `delegate` must be a live FLUIWindowDelegate whose `window_ptr` ivar is
/// either null or points to a `Weak<MacOSWindow>` owned by that delegate.
unsafe fn get_window_from_delegate(delegate: &Object) -> Option<Arc<MacOSWindow>> {
    // SAFETY: per the function contract the ivar is null or a valid
    // Box<Weak<MacOSWindow>> pointer owned by the delegate.
    unsafe {
        let window_ptr: *mut std::ffi::c_void = *delegate.get_ivar("window_ptr");
        let weak_ptr = window_ptr.cast::<Weak<MacOSWindow>>();
        if weak_ptr.is_null() {
            return None;
        }
        (*weak_ptr).upgrade()
    }
}

// ============================================================================
// Window Event Handlers
// ============================================================================

impl MacOSWindow {
    /// Handle window resize event
    fn handle_resize(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let frame: NSRect = msg_send![self.ns_window, frame];
            let content_rect: NSRect = msg_send![self.ns_window, contentRectForFrameRect: frame];

            let new_size = Size::new(
                flui_types::geometry::px(content_rect.size.width as f32),
                flui_types::geometry::px(content_rect.size.height as f32),
            );

            // Update state
            let scale = {
                let mut state = self.state.lock();
                state.bounds.size = new_size;
                state.scale_factor
            };

            // Notify per-window callbacks
            self.callbacks.dispatch_resize(new_size, scale as f32);

            tracing::debug!(
                "Window resized to {}x{}",
                new_size.width.0,
                new_size.height.0
            );
        }
    }

    /// Handle window move event
    fn handle_move(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let frame: NSRect = msg_send![self.ns_window, frame];

            let new_origin = Point::new(
                flui_types::geometry::px(frame.origin.x as f32),
                flui_types::geometry::px(frame.origin.y as f32),
            );

            // Update state
            {
                let mut state = self.state.lock();
                state.bounds.origin = new_origin;
            }

            self.callbacks.dispatch_moved();

            tracing::debug!("Window moved to ({}, {})", new_origin.x.0, new_origin.y.0);
        }
    }

    /// Handle focus gained event
    fn handle_focus_gained(&self) {
        self.callbacks.dispatch_active_status_change(true);
        // Key-window status is what VoiceOver treats as view focus; the
        // delegate delivers this on the main thread.
        #[cfg(feature = "a11y")]
        if let Some(bridge) = self.accessibility.get() {
            bridge.update_view_focus_state(true);
        }
        tracing::debug!("Window gained focus");
    }

    /// Handle focus lost event
    fn handle_focus_lost(&self) {
        self.callbacks.dispatch_active_status_change(false);
        #[cfg(feature = "a11y")]
        if let Some(bridge) = self.accessibility.get() {
            bridge.update_view_focus_state(false);
        }
        tracing::debug!("Window lost focus");
    }

    /// Handle occlusion state change (hidden-surface gating).
    ///
    /// Reads the window's current `occlusionState` and forwards the visible
    /// bit through `dispatch_visibility_status_change` — the wire the
    /// runtime's per-presentation `FrameClock` gate and `AppLifecycleState`
    /// derivation hang off. The decidable half (bit test, polarity, edge
    /// filter) lives in `crate::shared::visibility`, host-tested; only the
    /// `occlusionState` read is AppKit-bound.
    fn handle_occlusion_state_changed(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`;
        // `occlusionState` returns a plain NSUInteger bitmask.
        let raw: u64 = unsafe {
            let state: usize = msg_send![self.ns_window, occlusionState];
            state as u64
        };
        let visible = crate::shared::visibility::appkit_occlusion_state_is_visible(raw);
        let edge = {
            let mut state = self.state.lock();
            let edge = crate::shared::visibility::visibility_edge(state.occlusion_visible, visible);
            if edge.is_some() {
                state.occlusion_visible = visible;
            }
            edge
        };
        // Dispatch outside the state lock, like every other handler here.
        if let Some(visible) = edge {
            tracing::debug!(?visible, "Window occlusion state changed");
            self.callbacks.dispatch_visibility_status_change(visible);
        }
    }

    /// Handle close request event
    ///
    /// Returns true to allow close, false to prevent
    fn handle_close_request(&self) -> bool {
        let should_close = self.callbacks.dispatch_should_close();
        tracing::debug!("Window close requested: {}", should_close);
        should_close
    }

    /// Handle window close event
    ///
    /// Fires from `windowWillClose:` while `ns_window` is still valid — the
    /// delegate notification AppKit posts for every route through
    /// `-[NSWindow close]`, vetoed or not (see [`close`](Self::close)'s own
    /// doc). Order matters: `closed` is set BEFORE `dispatch_close` so that
    /// a frame callback the close dispatch itself triggers observes the
    /// window as already gone, and `callbacks.clear()` runs AFTER, so the
    /// `on_close` callback registered above still fires normally before its
    /// slot — and every other slot — is released.
    fn handle_close(&self) {
        self.closed.store(true, Ordering::SeqCst);

        // Untrack from the platform's window map here, on the owner thread the
        // delegate notification runs on, rather than from `Drop` — `Drop` is the
        // only other removal site and is unreachable while the map itself pins a
        // wrapper clone (issue #1147). Removing the entry lets the wrapper reach
        // `Drop`'s last-clone gate (and so the NSWindow release + a11y teardown
        // tail) as soon as the application drops its final external handle;
        // without it, every closed window stays pinned for the process lifetime.
        //
        // Removal precedes `dispatch_close()` so the closing window is already
        // untracked when user close callbacks run. Unlike the repo's winit
        // backend — which removes from tracking only AFTER its own close
        // callbacks (winit/platform.rs dispatch_close before windows.remove) —
        // this ordering is safe here because the macOS map is private with no
        // content readers: the early removal is observable only as the intended
        // lifetime change. `Drop`'s removal stays as an idempotent safety net.
        let _prev = self.windows_map.lock().remove(&(self.ns_window as u64));

        self.callbacks.dispatch_close();
        tracing::debug!("Window closed");

        // Release every remaining registered callback now, while
        // `ns_window` is still valid (this delegate method runs before
        // AppKit tears the window down further). Without this, the frame
        // callback registered via `on_request_frame` — which in
        // `flui-app`'s wiring owns this window's GPU renderer, whose
        // `wgpu::Surface` is built from the `Arc<dyn PlatformWindow>` clone
        // the renderer owns (ADR-0063) — stays pinned forever: window
        // (through its callback slots) → frame closure → raster lane →
        // renderer → surface → `Arc<MacOSWindow>`, a cycle nothing else
        // here breaks.
        self.callbacks.clear();

        // Match winit's own `windowWillClose:` handling: NIL the delegate
        // so no further delegate method can fire against a window this
        // wrapper now treats as closed. Harmless even without this today
        // (the delegate holds only a `Weak<MacOSWindow>`), but this is the
        // documented AppKit-recommended cleanup, not just a FLUI habit.
        //
        // SAFETY: `ns_window` is alive for the lifetime of `self`.
        unsafe {
            let _: () = msg_send![self.ns_window, setDelegate: NIL];
        }
    }

    /// Handle backing properties changed (Retina/DPI change)
    fn handle_backing_properties_changed(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`; the
        // content view is NIL-checked before use.
        unsafe {
            let new_scale: f64 = msg_send![self.ns_window, backingScaleFactor];

            // Re-read the refresh period in the same pass: this handler is
            // reached on both a backing-properties change and a screen change
            // (`handle_screen_changed` delegates here), and the period belongs
            // to the display, so it moves with the window exactly as the scale
            // does. No resize is dispatched for it — a period change does not
            // invalidate layout, and the runner re-reads the period when it
            // does resize.
            let screen: ObjcId = msg_send![self.ns_window, screen];
            let new_refresh_period = screen_refresh_period(screen);

            // Update window state
            let (changed, size) = {
                let mut state = self.state.lock();
                let changed = (state.scale_factor - new_scale).abs() > 0.01;
                if changed {
                    state.scale_factor = new_scale;
                    tracing::info!("Window scale factor changed to {}", new_scale);
                }
                state.refresh_period = new_refresh_period;
                (changed, state.bounds.size)
            };

            // Update content view scale factor
            let content_view: ObjcId = msg_send![self.ns_window, contentView];
            if content_view != NIL {
                view::update_view_scale_factor(
                    content_view.cast::<objc2::runtime::AnyObject>(),
                    new_scale,
                );
            }

            // A scale change invalidates layout: notify as a resize
            if changed {
                self.callbacks.dispatch_resize(size, new_scale as f32);
            }
        }
    }

    /// Handle screen changed (moved to different monitor)
    fn handle_screen_changed(&self) {
        // SAFETY: `ns_window` is alive for the lifetime of `self`; the
        // screen object is NIL-checked before messaging.
        unsafe {
            let screen: ObjcId = msg_send![self.ns_window, screen];
            if screen != NIL {
                let scale: f64 = msg_send![screen, backingScaleFactor];
                tracing::debug!("Window moved to screen with scale factor {}", scale);

                // Update scale factor (will trigger backing properties changed)
                self.handle_backing_properties_changed();
            }
        }
    }
}

// ============================================================================
// Window routing tests — always-run wrapper pin + `#[ignore]`d site tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::super::owner_lane::test_owner_queue;
    use super::*;

    /// Always-run, AppKit-free pin on the `route_on_owner` wrapper: a body
    /// dispatched onto the lane from a background thread must observe the
    /// lane guard at the probe point. A bare un-routed body (wrapper bypassed)
    /// would record `false`, so this is the executable "the sweep's wrapper
    /// routes" carrier — the owner_lane machinery tests pin `exec_on_owner`
    /// itself; this pin sits one layer above, at the window-surface throat.
    ///
    /// Per the probe semantics documented beside `routing_probe`, the
    /// assertion is scoped to the OFF-LANE arm: an inline (on-lane or
    /// OS-main-thread) call intentionally runs without the guard and records
    /// `false`, so a nested on-lane call must NOT be asserted to record `true`.
    #[test]
    fn route_on_owner_runs_body_under_lane_guard() {
        routing_probe::clear();
        let owner = test_owner_queue();
        let handle = std::thread::spawn(move || {
            route_on_owner(owner, false, || ());
        });
        handle
            .join()
            .expect("an off-lane routed body must complete without panicking");
        assert_eq!(
            routing_probe::last(),
            Some(true),
            "routing assertion: the off-lane body must observe the owner-lane guard at the \
             probe point, which a bare un-routed body would record as unset"
        );
    }

    /// Always-run, AppKit-free pin on the display-pass decision: a redraw
    /// request issued while this thread is inside a display pass must take the
    /// deferral arm and must NOT send inline. The inline `setNeedsDisplay:` is
    /// precisely the call AppKit discards mid-pass, and the reason the pump used
    /// to stop after one frame — so this is the branch that carries the fix, and
    /// deleting it must fail here.
    ///
    /// Both arms arrive as closures, so both are observable without an NSWindow
    /// (a bare test process cannot construct one). The `DisplayPassGuard` is the
    /// real marker the production decision reads, entered on this thread the way
    /// `view.rs`'s `draw_rect` enters it around a dispatched frame.
    #[test]
    fn a_redraw_request_inside_a_display_pass_defers_and_never_sends_inline() {
        let deferred = Arc::new(AtomicBool::new(false));
        let inline = Arc::new(AtomicBool::new(false));
        let (deferred_flag, inline_flag) = (Arc::clone(&deferred), Arc::clone(&inline));

        {
            let _pass = super::super::display_pass::DisplayPassGuard::enter();
            dispatch_redraw_request(
                || deferred_flag.store(true, Ordering::SeqCst),
                move || inline_flag.store(true, Ordering::SeqCst),
            );
        }

        assert!(
            deferred.load(Ordering::SeqCst),
            "a request from inside a display pass must defer: the inline call is the one AppKit \
             discards during the pass, and the frame it belonged to has already gone"
        );
        assert!(
            !inline.load(Ordering::SeqCst),
            "the inline arm must not run inside a display pass — it is the stalled-pump bug"
        );
    }

    /// The complementary arm, so the pin above cannot be satisfied by a
    /// `dispatch_redraw_request` that defers unconditionally (which would add a
    /// lane turn to every ordinary request).
    #[test]
    fn a_redraw_request_outside_a_display_pass_sends_inline() {
        let deferred = Arc::new(AtomicBool::new(false));
        let inline = Arc::new(AtomicBool::new(false));
        let (deferred_flag, inline_flag) = (Arc::clone(&deferred), Arc::clone(&inline));

        dispatch_redraw_request(
            || deferred_flag.store(true, Ordering::SeqCst),
            move || inline_flag.store(true, Ordering::SeqCst),
        );

        assert!(
            inline.load(Ordering::SeqCst),
            "a request from outside a display pass is already delivered after the pass — its lane \
             hop waits for it — so it must not pay a turn"
        );
        assert!(
            !deferred.load(Ordering::SeqCst),
            "the deferral must not start"
        );
    }

    /// The window integration test for the owner-lane sweep (issue #1194,
    /// extending the #949 routing test). It uses the shared test lane
    /// EXCLUSIVELY: it constructs a real NSWindow on the lane and routes the
    /// full class-A surface through it, so it must not run interleaved with
    /// any other test sharing that lane (the AppKit-free `owner_lane` tests
    /// never touch a window, and this is the only window test that drives the
    /// surface).
    ///
    /// Opt-in on a Mac with an active GUI session; the AppKit-free
    /// `route_on_owner` and `owner_lane` tests are the always-run routing
    /// carriers.
    ///
    /// A bare `cargo test` process has no NSApplication connection, so
    /// `[NSWindow initWithContentRect:]` throws an NSException from any thread:
    /// a construction probe on this machine aborted with SIGABRT through
    /// `-[NSWindow _initContent:...]` + `-`CFBundleGetValueForInfoKey`. Run with
    /// `cargo test -p flui-platform -- --ignored window_surface_is_owner_routed`
    /// only from a test process that pumps an AppKit run loop.
    #[test]
    #[ignore = "requires an AppKit-run-loop-pumping test process; the AppKit-free route_on_owner/owner_lane tests are the always-run routing carriers"]
    fn window_surface_is_owner_routed() {
        let owner = test_owner_queue();
        let window = MacOSWindow::for_test(owner)
            .expect("for_test must construct an NSWindow on the test lane");

        // Hand the OWNED value (the last wrapper — the map entry is removed
        // below) to an off-lane worker that drives EVERY swept class-A body.
        // Every driving call is an off-lane arm, so every probe record is a
        // dispatch-guard witness and `all_on_lane()` is the routing assertion.
        let map = Arc::clone(&window.windows_map);
        let _prev = map.lock().remove(&(window.ns_window as u64));
        drop(map);
        let mut window = Arc::try_unwrap(window)
            .expect("removing the window from its map leaves the returned Arc alone");

        routing_probe::clear();
        let handle = std::thread::spawn(move || {
            // ---- PlatformWindow surface (12 bodies) ----
            let _ = <MacOSWindow as PlatformWindow>::get_title(&window);
            <MacOSWindow as PlatformWindow>::set_title(&window, "owner-routed");
            let _ = <MacOSWindow as PlatformWindow>::is_focused(&window);
            let _ = <MacOSWindow as PlatformWindow>::is_visible(&window);
            <MacOSWindow as PlatformWindow>::activate(&window);
            <MacOSWindow as PlatformWindow>::minimize(&window);
            <MacOSWindow as PlatformWindow>::maximize(&window);
            <MacOSWindow as PlatformWindow>::restore(&window);
            <MacOSWindow as PlatformWindow>::toggle_fullscreen(&window);
            <MacOSWindow as PlatformWindow>::resize(
                &window,
                Size::new(Pixels(800.0), Pixels(600.0)),
            );
            <MacOSWindow as PlatformWindow>::set_cursor(&window, CursorIcon::Default)
                .expect("set_cursor must not fail on a real window");
            <MacOSWindow as PlatformWindow>::request_redraw(&window);
            <MacOSWindow as PlatformWindow>::close(&window);

            // ---- WindowTrait surface (12 bodies) ----
            <MacOSWindow as WindowTrait>::set_position(
                &mut window,
                Point::new(Pixels(0.0), Pixels(0.0)),
            );
            let _ = <MacOSWindow as WindowTrait>::state(&window);
            <MacOSWindow as WindowTrait>::set_state(&mut window, WindowState::Normal);
            <MacOSWindow as WindowTrait>::set_visible(&mut window, true);
            let _ = <MacOSWindow as WindowTrait>::is_resizable(&window);
            <MacOSWindow as WindowTrait>::set_resizable(&mut window, true);
            let _ = <MacOSWindow as WindowTrait>::is_minimizable(&window);
            <MacOSWindow as WindowTrait>::set_minimizable(&mut window, true);
            let _ = <MacOSWindow as WindowTrait>::is_closable(&window);
            <MacOSWindow as WindowTrait>::set_closable(&mut window, true);
            <MacOSWindow as WindowTrait>::set_min_size(
                &mut window,
                Some(Size::new(Pixels(100.0), Pixels(100.0))),
            );
            <MacOSWindow as WindowTrait>::set_max_size(
                &mut window,
                Some(Size::new(Pixels(1000.0), Pixels(1000.0))),
            );

            // ---- MacOSWindowExtTrait surface (11 bodies) ----
            <MacOSWindow as MacOSWindowExtTrait>::set_liquid_glass_config(
                &mut window,
                LiquidGlassConfig::from_material(LiquidGlassMaterial::Standard),
            );
            <MacOSWindow as MacOSWindowExtTrait>::clear_liquid_glass(&mut window);
            <MacOSWindow as MacOSWindowExtTrait>::enable_tabbing(&mut window);
            <MacOSWindow as MacOSWindowExtTrait>::disable_tabbing(&mut window);
            let own_id = window.ns_window as u64;
            <MacOSWindow as MacOSWindowExtTrait>::add_tab_to_window(&mut window, own_id);
            <MacOSWindow as MacOSWindowExtTrait>::toggle_native_fullscreen(&mut window);
            <MacOSWindow as MacOSWindowExtTrait>::set_window_level(
                &mut window,
                MacOSWindowLevel::Normal,
            );
            let _ = <MacOSWindow as MacOSWindowExtTrait>::window_level(&window);
            <MacOSWindow as MacOSWindowExtTrait>::set_collection_behavior(
                &mut window,
                MacOSCollectionBehavior::DEFAULT,
            );
            <MacOSWindow as MacOSWindowExtTrait>::set_has_shadow(&mut window, true);
            <MacOSWindow as MacOSWindowExtTrait>::set_alpha(&mut window, 0.5);
        });
        handle
            .join()
            .expect("driving the full window surface from a worker must complete without crashing");
        assert!(
            routing_probe::all_on_lane(),
            "routing assertion: every class-A body driven from the off-lane worker must have \
             executed under the owner-lane guard; an un-routed body records the guard unset"
        );
    }

    /// Dropping the last window wrapper routes the AppKit teardown tail onto
    /// the owner lane FIRE-AND-FORGET: the drop returns promptly — it never
    /// blocks on the lane, whatever the lane's servicing state — and the
    /// on-lane-ness of the tail body itself is covered by the always-run
    /// `exec_async_guarded_runs_body_under_lane_guard` mechanism pin (the Drop
    /// body is exactly the guarded body that pin runs).
    ///
    /// Same opt-in constraint as [`window_surface_is_owner_routed`]:
    /// constructing a real NSWindow needs an AppKit-run-loop-pumping test
    /// process.
    #[test]
    #[ignore = "requires an AppKit-run-loop-pumping test process; the AppKit-free mechanism pins are the always-run teardown carriers"]
    fn drop_routes_appkit_tail_off_owner() {
        let owner = test_owner_queue();
        let window = MacOSWindow::for_test(owner)
            .expect("for_test must construct an NSWindow on the test lane");

        // Remove the window from its own map so the `for_test` Arc is the last
        // wrapper reference, then unwrap to the owned value so the wrapper's
        // `Drop` is what runs on the background thread.
        let map = Arc::clone(&window.windows_map);
        let _prev = map.lock().remove(&(window.ns_window as u64));
        drop(map);
        let owned = Arc::try_unwrap(window)
            .expect("removing the window from its map leaves the returned Arc alone");

        let handle = std::thread::spawn(move || {
            drop(owned);
        });
        handle.join().expect(
            "dropping the last window wrapper on a worker thread must return promptly \
                     and crash-free (Drop never waits on the lane)",
        );
    }

    /// Closing a window must drain it from the platform's tracking map on the
    /// owner thread (issue #1147). The map is `MacOSPlatform`'s only strong
    /// reference that outlives the application's own handles, so a closed
    /// window whose entry is never removed pins its wrapper — and through it
    /// the native NSWindow and a11y adapter — for the process lifetime: `Drop`'s
    /// own removal cannot run while the map holds a clone. This test drives the
    /// real close route (`close()` → `-[NSWindow close]` → `windowWillClose:` →
    /// `handle_close`) and asserts the map drains.
    ///
    /// Same opt-in constraint as its two siblings: a bare `cargo test` process
    /// has no NSApplication connection and NSWindow construction SIGABRTs
    /// through `_CFBundleGetValueForInfoKey` (observed on this machine); a
    /// run-loop-pumping test process exercises the real close path, which is
    /// the AppKit close-path validation this issue's Win32 half inherits.
    #[test]
    #[ignore = "requires an AppKit-run-loop-pumping test process; the always-run mechanism pins cover the routing, and the map-drain assertion needs a real window"]
    fn close_drains_platform_map_entry() {
        let owner = test_owner_queue();
        let window = MacOSWindow::for_test(owner)
            .expect("for_test must construct an NSWindow on the test lane");

        let window_id = window.ns_window as u64;
        let map = Arc::clone(&window.windows_map);
        assert!(
            map.lock().contains_key(&window_id),
            "for_test must insert this window into its own map"
        );
        drop(map);

        <MacOSWindow as PlatformWindow>::close(&window);

        // The close route completes synchronously from the caller's
        // perspective on an uncontended serial lane (dispatch_sync runs
        // inline): `-[NSWindow close]` posts `windowWillClose:` synchronously,
        // so `handle_close` — and with it the map removal — has run by the
        // time `close` returns.
        assert!(
            !window.windows_map.lock().contains_key(&window_id),
            "closing a window must remove its entry from the platform's tracking map \
             (issue #1147); the map's clone otherwise pins the wrapper — and through \
             it the NSWindow + a11y adapter — for the process lifetime"
        );
    }
}
