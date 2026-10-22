//! The macOS window: [`MacOSWindow`] over an `NSWindow` (its state as AppKit
//! reports it, the requests its commands send, and [`display_pass`], whether
//! the thread is inside a view's display pass, which decides how a redraw is
//! asked for), and [`MacOSWindows`], what the windows open with.
//!
//! Two parts live beside it: [`delegate`], the window's `NSWindowDelegate`,
//! one AppKit object with the handlers of what it reports; and [`ext`],
//! [`MacOSWindowExt`] with the tiling configuration it takes, the public
//! AppKit features beyond the cross-platform window.
//!
//! The loop holds the windows it tracks as `CallbackWindow`s
//! (`owner::apple::macos`).

// Direct AppKit objc calls have no safe wrapper: the window is one of the
// crate's `unsafe` FFI islands, opted out at its module boundary.
#![expect(unsafe_code)]
// The window still sends raw `msg_send!` in objc2's comma-less form and reads
// its delegate's ivar by name, both deprecated by objc2 in favour of the typed
// API this file has not moved to yet.
#![expect(deprecated)]

pub(crate) mod delegate;
pub(crate) mod ext;

pub(crate) mod display_pass {
    //! Whether the current thread is inside an AppKit display pass of a view.
    //!
    //! AppKit discards a `setNeedsDisplay:` that is issued *while the view is being
    //! displayed*: the flag is cleared when the pass unwinds, so the request that
    //! was supposed to schedule the next frame disappears with it. Measured on
    //! macOS 15.7 (Darwin 24.6) with an isolated probe, on a window that is fully
    //! visible (`occlusionState` has the visible bit) in every arm:
    //!
    //! | re-arm issued from inside `drawRect:` | draws over 4 s |
    //! |---|---|
    //! | `[view setNeedsDisplay:YES]` | 1 |
    //! | nothing at all (control) | 1 |
    //! | `[view.layer setNeedsDisplay]` | 1 |
    //! | `setNeedsDisplay:` deferred to the next main-queue turn | 396 |
    //!
    //! So a view's `drawRect:` cannot re-arm itself in place, and neither can its
    //! layer; deferring the same call by one turn of the lane that is already
    //! serialized is what actually re-arms it. `[view needsDisplay]` reads `NO`
    //! immediately after the in-pass call and `[view.layer needsDisplay]` reads
    //! `YES` while still never being serviced — the flag that survives is not the
    //! flag AppKit acts on, so neither is a usable signal to test against.
    //!
    //! This module is the condition that decision is made on, kept apart from both
    //! ends so it can be tested without a window: [`DisplayPassGuard`] is entered
    //! by the view's `drawRect:` for the duration of the frame it dispatches, and
    //! consulted by the window's `request_redraw` to choose between setting the
    //! flag in place and deferring it.
    //!
    //! It is the AppKit realisation of ADR-0039 §4(b)/(c): the frame transaction
    //! is a region the drain gate is closed for, and a wake arriving while it is
    //! closed **defers** — the lane stays queued and the wake re-arms at the next
    //! top-level anchor — so the frame transaction stays uninterruptible. The
    //! `DrainGate` that same section specifies for the lane is this marker's
    //! sibling: a wake delivered during a nested modal run loop must defer for the
    //! same reason, and when the AppKit `CFRunLoopSource` relay lands it must
    //! consult a gate that agrees with this one. Two owner-thread re-entrancy
    //! markers that never reference each other is how they drift apart, so the
    //! relation is written down here rather than implied.
    //!
    //! The marker is thread-local because "displaying" is a property of the thread
    //! AppKit is displaying on, not of the process: every other thread is free to
    //! request a redraw at that moment, and its request is queued behind the pass
    //! rather than swallowed by it. The grain is therefore the *thread*, not "this
    //! window's display pass" — a request for a different window issued on this
    //! thread while any view here is displaying defers as well. That is
    //! conservative in the safe direction (a deferred `setNeedsDisplay:` is always
    //! honoured; it costs one lane turn at worst), and it is the reason
    //! `request_redraw` does not claim to defer exactly the discarded call.

    use std::cell::Cell;

    thread_local! {
        /// True while this thread is inside a `drawRect:`-driven display pass.
        static IN_DISPLAY_PASS: Cell<bool> = const { Cell::new(false) };
    }

    /// Is the current thread inside an AppKit display pass of a view?
    pub(super) fn in_display_pass() -> bool {
        IN_DISPLAY_PASS.with(Cell::get)
    }

    /// Marks the current thread as inside a display pass until dropped.
    ///
    /// The guard restores the flag's previous value rather than clearing it, so
    /// nesting (a display pass that somehow triggers another) unwinds correctly
    /// instead of leaving the outer pass's marker cleared early.
    pub(crate) struct DisplayPassGuard {
        /// The flag's value before this guard set it.
        previous: bool,
    }

    impl DisplayPassGuard {
        /// Mark the current thread as inside a display pass.
        pub(crate) fn enter() -> Self {
            let previous = IN_DISPLAY_PASS.with(|flag| flag.replace(true));
            Self { previous }
        }
    }

    impl Drop for DisplayPassGuard {
        fn drop(&mut self) {
            let previous = self.previous;
            IN_DISPLAY_PASS.with(|flag| flag.set(previous));
        }
    }
}

use delegate::{create_window_delegate, display_of_screen};
pub use ext::tiling::{TilePosition, TilingConfiguration, TilingError, TilingLayout, TilingState};
pub use ext::{MacOSCollectionBehavior, MacOSWindowExt, MacOSWindowLevel};

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use cursor_icon::CursorIcon;
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{MainThreadMarker, msg_send};
use objc2::{class, sel};
use objc2_app_kit::{NSBackingStoreType, NSWindowCollectionBehavior, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::{
    SizeConstraints, WindowAppearance, WindowCapabilities, WindowCommand, WindowCommandError,
    WindowGeometry, WindowMode, WindowState,
};

use crate::window::apple::geometry::{
    frame::{
        cocoa_frame, cocoa_frame_keeping_top, cocoa_top_left_point, constraints_of_content_limits,
        content_limits_of_constraints, globalize, top_left_of_frame,
    },
    mode::{ModeSequence, ModeStep, WindowFlags, is_shown, mode_of},
};
use parking_lot::Mutex;
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle,
};

// Mid-migration shims for the raw-send shape this file keeps: `NSWindow` is
// `MainThreadOnly` in objc2's typed API, and this backend constructs windows
// on a caller-supplied owner lane, which a `MainThreadMarker`-gated method
// would refuse. The file therefore sends raw `msg_send!` (objc2's macro accepts a raw `*mut AnyObject` receiver,
// `Bool` arguments, and a manual `release`) and these aliases keep the
// Cocoa-era spelling readable.
type ObjcId = *mut AnyObject;
type Object = AnyObject;
type Class = AnyClass;
const NIL: ObjcId = std::ptr::null_mut();
type BObjC = Bool;
const YES: Bool = Bool::YES;
const NO: Bool = Bool::NO;

use crate::host;
use crate::host_window::HostWindow;
use crate::input::apple::macos as view;
use crate::owner::apple::MacOSLoop;
use crate::owner::apple::macos::{HostDisplays, WindowRegistry, debug_assert_appkit_main_thread};
use crate::owner::call_point::install;
use crate::owner::handlers::CallbackWindow;
use crate::{
    owner::OpenWindowError, owner::handlers::WindowCallbacks, window::config::WindowConfiguration,
    window::state::WindowStateCell,
};
use flui_platform::{
    CursorError, DisplayId, PlatformWindow, WindowEvent, WindowId, WindowOptions, WindowReveal,
};

/// macOS window wrapper around NSWindow
pub struct MacOSWindow {
    /// Native window handle (NSWindow*)
    ns_window: ObjcId,

    /// This window's `FLUIContentView` (NSView*): the view that receives
    /// input, IME and appearance changes, draws on `drawRect:`, carries the
    /// accessibility bridge, and is the `ns_view` of the raw window handle.
    ///
    /// Fixed for the window's lifetime and read from here, never from
    /// `-[NSWindow contentView]`: Liquid Glass makes an `NSVisualEffectView`
    /// the content view and re-parents this view inside it. Not retained by
    /// the wrapper: the window's view hierarchy holds it, and the one code
    /// path that detaches it (the Liquid Glass swap) retains it across the
    /// swap.
    view: ObjcId,

    /// Window state
    state: Arc<Mutex<MacOSWindowState>>,

    /// Reference to all windows
    windows_map: WindowRegistry,

    /// Per-window callbacks (input, resize, close, ...)
    callbacks: Arc<WindowCallbacks>,

    /// Set once `windowWillClose:` has fired for this window (or `close()`
    /// completed without it firing — see `close()`'s own doc). Shared via
    /// `Arc` like every other per-window field here, so every clone of this
    /// wrapper sees the same window's closed state instead of each tracking
    /// its own. Consulted by [`HasWindowHandle::window_handle`] because,
    /// with `setReleasedWhenClosed:NO` set at construction, `-[NSWindow
    /// close]` no longer deallocates `ns_window` — so its view stays
    /// answerable long after the window is meaningless to hand a GPU handle
    /// for, and only this flag (not a nil check) can tell the two apart.
    closed: Arc<AtomicBool>,
    loop_control: std::sync::OnceLock<std::sync::Weak<crate::owner::apple::macos::LoopControl>>,

    /// Window configuration
    config: WindowConfiguration,

    /// The owner lane every thread-affine AppKit message travels through
    /// (see `owner::apple::macos::owner_lane`). The AppKit main queue for
    /// windows created by [`MacOSWindow::new`].
    owner: &'static dispatch::Queue,

    /// True when `owner` is the main queue, so "on the main thread" is
    /// equivalent to "on the owner lane" for the direct-path probe in
    /// [`crate::owner::apple::macos::exec_on_owner`].
    owner_is_main: bool,

    /// This window's NSAccessibility bridge, subclassed onto the content
    /// view. A `OnceLock` slot rather than a plain field because the window
    /// value is constructed *before* its content view exists (the view
    /// holds a `Weak` back-reference to the window's callbacks); the
    /// constructor fills it immediately after installing the view.
    #[cfg(feature = "a11y")]
    accessibility: std::sync::OnceLock<Arc<crate::accessibility::apple::MacosAccessibility>>,

    /// The platform's wake pump, armed from [`PlatformWindow::request_redraw`].
    ///
    /// `Weak`, and a `OnceLock`: the pump is platform-owned and outlives every
    /// window, so a window must not keep it alive — and the slot is filled
    /// once, by the window position's `open` right after construction. A
    /// window constructed without a platform leaves it empty and simply never
    /// arms one.
    wake_pump: std::sync::OnceLock<crate::owner::apple::macos::WeakWakePump>,

    /// This window's IME capability, built on first access by
    /// [`PlatformWindow::text_input`]. A `OnceLock` for the same reason the
    /// accessibility slot above is one: the capability is discovered through a
    /// `&self` accessor and must be handed out as one shared instance per
    /// window, not a fresh one per call.
    text_input: std::sync::OnceLock<Arc<crate::text::apple::macos::MacOSTextInput>>,

    /// The platform's display registry, by which this window turns its
    /// screen into a [`DisplayId`]. A `OnceLock` filled once by
    /// the window position's `open` right after construction, like the wake
    /// pump; a window built without a platform names no display.
    displays: std::sync::OnceLock<Arc<dyn HostDisplays>>,

    /// The published window state (ADR-0237). Seeded from the creation
    /// options; the first read replaces it with what AppKit reports.
    snapshot: Arc<WindowStateCell>,
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
// queue for test windows (see `owner::apple::macos::owner_lane`) — so no
// off-owner caller ever reaches AppKit bare. `request_redraw` was the first such site
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
// platform entries (ADR-0039) — their un-routed read of the window's view is the
// owner-sweep plan's recorded class-E residual, a deliberate carve-out of the
// routing sweep rather than a local lapse. [`Drop`] releases its AppKit tail
// inline on the owner; off-owner it queues the tail without blocking. If that
// queue is no longer servicing, the window stays over-retained — a leak, never
// an off-main deallocation. The close delegate's autorelease pin protects the
// native receiver while callback-local Rust wrappers drop. `OwnerLaneId` is
// consumed only on the owner thread (directly or under its queue guard).
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
    bounds: Bounds<f64>,

    /// Scale factor (1.0 for non-Retina, 2.0 for Retina)
    scale_factor: f64,

    /// The display this window is on. Cached, not queried per call: reading it
    /// messages the window's screen, which is AppKit traffic this backend keeps
    /// on the owner lane, while `PlatformWindow::display_id` is a plain `&self`
    /// accessor any thread may call. Refreshed wherever `scale_factor` is —
    /// `windowDidChangeScreen:` and `windowDidChangeBackingProperties:`.
    display: Option<DisplayId>,

    /// Cursor selected by this exact window's presentation.
    cursor: CursorIcon,

    /// The outer rectangle the window returns to from a maximized, minimized
    /// or full-screen mode. AppKit does not keep one a caller can read, so it
    /// is recorded just before the window leaves normal mode (`record_normal`)
    /// and when the window opens in another mode; `None` until then.
    normal: Option<WindowGeometry>,

    /// The order in which mode commands reach AppKit, and the mode a hidden
    /// window enters when first shown.
    sequence: ModeSequence,

    /// Last surface-visibility value dispatched through
    /// `WindowCallbacks::dispatch_visibility_status_change` (hidden-surface
    /// gating), fed by `windowDidChangeOcclusionState:`. Edge filter:
    /// AppKit documents that the notification may fire for reasons other
    /// than the visible bit flipping, so an unchanged value is never
    /// re-dispatched (`window::visibility::visibility_edge`). Starts
    /// `true` — a freshly created window is treated as visible until AppKit
    /// says otherwise, matching every other backend's initial state.
    occlusion_visible: bool,

    /// The window was opened `visible: true` with
    /// [`WindowReveal::AfterFirstFrame`] and is ordered front, but at
    /// `alphaValue` 0: the first reveal — alpha back to 1 — is deferred
    /// until the embedder reports a presented frame
    /// ([`PlatformWindow::reveal_after_first_frame`]) or asks for the window
    /// explicitly (`show`/`set_visible(true)`/`activate`). Opt-in, because
    /// only a caller that drives a frame loop can make that report; a
    /// direct consumer opening a window with the default
    /// [`WindowReveal::AtOpen`] sees it at once, alpha untouched.
    ///
    /// Ordered-but-transparent rather than hidden, and measured rather than
    /// assumed: a window that is not ordered on screen gets no Metal
    /// drawable — wgpu reports the surface occluded and every frame comes
    /// back `NotShown` — so a reveal that waited for a present into a hidden
    /// window would wait for something that cannot happen (observed
    /// 2026-09-21: 30+ withheld frames, then the embedder's fallback). A
    /// transparent window is composited normally, so the first frame
    /// presents into it, and nothing is on screen until it has.
    first_reveal_pending: bool,
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

/// Whether a window opened with `options` is ordered front at alpha 0 and
/// revealed later (see `MacOSWindowState::first_reveal_pending`): only a
/// `visible: true` window whose caller asked for
/// [`WindowReveal::AfterFirstFrame`] and so committed to reporting its first
/// presented frame. Everything else — the default `AtOpen`, or a hidden
/// window — has alpha left alone.
fn defers_first_reveal(options: &WindowOptions) -> bool {
    options.visible && options.reveal == WindowReveal::AfterFirstFrame
}

impl MacOSWindow {
    /// Clear the deferred-first-reveal flag, returning whether it was set:
    /// the one call that decides which caller performs the reveal. Taken
    /// under the state lock and never inside an owner route, so it cannot
    /// be held across an AppKit message.
    fn take_pending_first_reveal(&self) -> bool {
        std::mem::replace(&mut self.state.lock().first_reveal_pending, false)
    }

    /// Create a new macOS window
    ///
    /// `options.position` and `options.normal_geometry` are read as global
    /// logical pixels from the primary screen's top-left corner; a display
    /// named in `options.display` is resolved by the window position's `open`
    /// before the window is built.
    ///
    /// # Errors
    /// [`OpenWindowError::Backend`] when AppKit refuses to allocate the
    /// NSWindow.
    pub(crate) fn new(
        options: WindowOptions,
        windows_map: WindowRegistry,
        config: WindowConfiguration,
    ) -> Result<Arc<Self>, OpenWindowError> {
        // Production windows route every thread-affine AppKit message through
        // the AppKit main queue — the one lane all other process AppKit
        // traffic already serializes on (see `owner::apple::macos::owner_lane`).
        Self::new_inner(
            options,
            windows_map,
            config,
            crate::owner::apple::macos::owner_queue(),
            true,
        )
    }

    /// Hand this window the platform's display registry, and read which display
    /// it is on.
    ///
    /// Called once by the window position's `open`, right after construction
    /// and before the window is handed to anyone. A setter, like
    /// [`Self::install_wake_pump`]: the window is built before the platform
    /// hands it what it shares with the other windows.
    fn bind_displays(&self, displays: Arc<dyn HostDisplays>) {
        // First writer wins, like the wake pump: the slot is read on every
        // screen change and must not flip between registries.
        if self.displays.set(displays).is_ok() {
            self.refresh_display();
        }
    }

    /// The cell this window publishes its state through, so the platform can
    /// mark it stale when the machine wakes.
    fn state_cell(&self) -> &Arc<WindowStateCell> {
        &self.snapshot
    }

    /// Re-read which display this window is on; `true` when the answer
    /// changed. Owner lane only (this messages the window's screen).
    fn refresh_display(&self) -> bool {
        let Some(displays) = self.displays.get() else {
            return false;
        };
        // SAFETY: `ns_window` is alive for the lifetime of `self`; the screen
        // answer is nil or a live `NSScreen*` for the duration of this call.
        let id = unsafe {
            let screen: ObjcId = msg_send![self.ns_window, screen];
            display_of_screen(displays.as_ref(), screen)
        };
        let mut state = self.state.lock();
        let changed = state.display != id;
        state.display = id;
        changed
    }

    /// Hand this window the platform's wake pump.
    ///
    /// Called once by the window position's `open`, immediately after
    /// construction and before the window is handed to anyone who could
    /// redraw it.
    fn install_wake_pump(&self, pump: crate::owner::apple::macos::WeakWakePump) {
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
    /// that deadline once the frame has gone. See
    /// `owner::apple::macos::wake_pump` for why the second look is needed at
    /// all.
    fn arm_wake_pump(&self) {
        if let Some(pump) = self.wake_pump.get().and_then(std::sync::Weak::upgrade) {
            pump.arm();
        }
    }

    fn new_inner(
        options: WindowOptions,
        windows_map: WindowRegistry,
        config: WindowConfiguration,
        owner: &'static dispatch::Queue,
        owner_is_main: bool,
    ) -> Result<Arc<Self>, OpenWindowError> {
        // SAFETY: must run on the owner thread (enforced by the platform's
        // event-loop ownership); all messaged objects are alive: the freshly allocated
        // NSWindow is checked for NIL before further use.
        unsafe {
            // Convert logical size to NSRect
            let frame = NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(options.size.width, options.size.height),
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
            let ns_window: ObjcId = msg_send![window_class(), alloc];
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
            // afterward (including the `setNeedsDisplay:` behind every
            // redraw request). Opting out here makes the `alloc`/`init` +1 released
            // exactly once, by `Drop`, matching winit's own
            // `setReleasedWhenClosed(false)` for its wrapped `NSWindow`.
            let _: () = msg_send![ns_window, setReleasedWhenClosed: NO];

            // Background colour. AppKit's default for a fresh alloc/init'd
            // window is `windowBackgroundColor`, a near-white
            // (`rgb(240,240,240)` measured on this host) that sits behind
            // the content view for as long as the first frame is still being
            // set up — on a cold launch, up to several seconds (2.81 s
            // measured). The user-facing glitch this default produces is a
            // blank white window: the window is
            // ordered front *before* `Renderer::render_scene` ever reaches
            // `queue.present`, so the viewer sees the bare background, not
            // the application. Setting the background to
            // `underPageBackgroundColor` — the dark neutral AppKit uses for
            // content chrome — doesn't remove the gap, it just stops
            // screaming about it: the window arrives as a dark field, and
            // the compositor's first frame lands on top of it without a
            // jarring white flash in between. The color is also the exact
            // one the launch-render gate's blank-window control measures
            // against, so the control and the production path disagree by
            // construction (a deliberately dark background is no longer
            // `rgb(240,240,240)` flat).
            let bg: ObjcId = msg_send![class!(NSColor), underPageBackgroundColor];
            let _: () = msg_send![ns_window, setBackgroundColor: bg];

            // Set window title
            let title = NSString::from_str(&options.title);
            let _: () = msg_send![ns_window, setTitle: &*title];

            // The limits are on the content size, like `options.size`, and the
            // same pair is read back into the state.
            let (min, max) = content_limits_of_constraints(SizeConstraints::new(
                options.min_size,
                options.max_size,
            ));
            let _: () = msg_send![ns_window, setContentMinSize: NSSize::new(min.width, min.height)];
            let _: () = msg_send![ns_window, setContentMaxSize: NSSize::new(max.width, max.height)];

            // A window must opt in to the full-screen transition for
            // `toggleFullScreen:` to do anything. Every window does, so one
            // that becomes resizable later can enter full screen too; a window
            // that cannot be resized is refused the mode by `change_mode`.
            let behavior: NSWindowCollectionBehavior = msg_send![ns_window, collectionBehavior];
            let _: () = msg_send![
                ns_window,
                setCollectionBehavior: behavior | NSWindowCollectionBehavior::FullScreenPrimary
            ];

            // Where the window opens. Placed before it is ordered front, so
            // it never shows at the system's choice first.
            let primary_height = primary_screen_height();
            let placed_at = options
                .position
                .zip(primary_height)
                .map(|(at, height)| cocoa_top_left_point(at, height));
            let mut placed = placed_at.is_some();
            if let Some(point) = placed_at {
                let _: () =
                    msg_send![ns_window, setFrameTopLeftPoint: NSPoint::new(point.x, point.y)];
            }
            // A window that opens maximized or full screen returns to
            // `normal_geometry`: put it there first, then change the mode.
            if options.mode != WindowMode::Normal
                && let Some(normal) = options.normal_geometry
            {
                let current: NSRect = msg_send![ns_window, frame];
                let size = normal.size;
                let frame = match (normal.position, primary_height) {
                    (Some(at), Some(height)) => {
                        placed = true;
                        cocoa_frame(at, size, height)
                    }
                    // Without a position the top-left corner stays where it is.
                    _ => cocoa_frame_keeping_top(
                        crate::display::apple::macos::bounds_of(current),
                        size,
                    ),
                };
                let _: () = msg_send![
                    ns_window,
                    setFrame: NSRect::new(
                        NSPoint::new(frame.origin.x, frame.origin.y),
                        NSSize::new(frame.size.width, frame.size.height),
                    ),
                    display: NO
                ];
            }

            // Without a position of its own, the window opens centred.
            if !placed {
                let _: () = msg_send![ns_window, center];
            }

            // The content size now: `normal_geometry` may have given the frame
            // a size of its own, and no delegate exists yet to report it.
            let content_size = {
                let placed: NSRect = msg_send![ns_window, frame];
                let content: NSRect = msg_send![ns_window, contentRectForFrameRect: placed];
                Size::new(content.size.width, content.size.height)
            };

            // Get backing scale factor
            let scale: f64 = msg_send![ns_window, backingScaleFactor];

            // A window opened `visible: true` is ordered front now. With
            // `WindowReveal::AfterFirstFrame` it is ordered fully
            // transparent: AppKit shows a window the moment it is ordered,
            // and the first frame reaches the compositor only after the GPU
            // stack behind it is built and the first present lands —
            // measured at 2.81 s on a cold launch — so an opaque window
            // here shows its bare background for that whole gap. It cannot
            // simply stay hidden either: an un-ordered window gets no Metal
            // drawable (see `MacOSWindowState::first_reveal_pending`). Alpha
            // 0 gives the surface a window to present into while the viewer
            // sees nothing; `reveal_after_first_frame` restores alpha 1.
            // The default `WindowReveal::AtOpen` leaves alpha alone — the
            // caller has no first frame to report — and `visible: false`
            // stays hidden until asked, as before.
            let first_reveal_pending = defers_first_reveal(&options);
            if first_reveal_pending {
                let _: () = msg_send![ns_window, setAlphaValue: 0.0f64];
            }
            if options.visible {
                let _: () = msg_send![ns_window, makeKeyAndOrderFront: NIL];
            }

            let callbacks = Arc::new(WindowCallbacks::new());

            // Create content view for input events. `frame` is
            // objc2-foundation's `NSRect`, the type `create_content_view` takes.
            let content_view = view::create_content_view(frame, scale, Arc::downgrade(&callbacks));

            let window = Arc::new(Self {
                ns_window,
                view: content_view,
                state: Arc::new(Mutex::new(MacOSWindowState {
                    bounds: Bounds {
                        origin: Point::new(frame.origin.x, frame.origin.y),
                        size: content_size,
                    },
                    scale_factor: scale,
                    display: None,
                    cursor: CursorIcon::default(),
                    normal: None,
                    sequence: ModeSequence::default(),
                    occlusion_visible: true,
                    first_reveal_pending,
                })),
                windows_map: Arc::clone(&windows_map),
                callbacks,
                closed: Arc::new(AtomicBool::new(false)),
                loop_control: std::sync::OnceLock::new(),
                config,
                owner,
                owner_is_main,
                #[cfg(feature = "a11y")]
                accessibility: std::sync::OnceLock::new(),
                wake_pump: std::sync::OnceLock::new(),
                text_input: std::sync::OnceLock::new(),
                displays: std::sync::OnceLock::new(),
                snapshot: Arc::new(WindowStateCell::new(
                    WindowState::new(WindowGeometry::new(None, options.size), options.size)
                        .with_title(Some(options.title.clone()))
                        .with_constraints(SizeConstraints::new(options.min_size, options.max_size))
                        .with_resizable(options.resizable)
                        .with_decorated(options.decorated),
                )),
            });

            let _: () = msg_send![ns_window, setContentView: content_view];

            // Subclass the freshly installed content view for VoiceOver.
            // SAFETY: `content_view` is the live NSView this window just
            // created and installed; the window owns the capability, so the
            // adapter cannot outlive the view; and window construction runs
            // on the main thread (AppKit affinity, asserted by this whole
            // `unsafe` block's contract).
            #[cfg(feature = "a11y")]
            {
                let bridge = crate::accessibility::apple::MacosAccessibility::new(
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
            let tracked: Arc<dyn CallbackWindow> = Arc::clone(&window) as _;
            let _prev = windows_map.lock().insert(window_id, tracked);

            // The rectangle the window returns to is what it has now, placed
            // as asked; then it enters the mode it opens in. A hidden window
            // enters it when it is first shown.
            window.state.lock().normal = Some(window.outer_geometry());
            if options.mode != WindowMode::Normal {
                if options.visible {
                    // A window that cannot take the mode opens normal, as the
                    // options document; its first snapshot says so.
                    let _ = window.change_mode(options.mode);
                } else {
                    window.state.lock().sequence.enter_when_shown(options.mode);
                }
            }

            tracing::info!(
                "Created NSWindow {:p} with size {}x{} (scale: {})",
                ns_window,
                options.size.width,
                options.size.height,
                scale
            );

            Ok(window)
        }
    }

    /// Get the NSWindow handle
    pub fn ns_window(&self) -> ObjcId {
        self.ns_window
    }

    /// Attach the owning standalone loop without creating a window/control cycle.
    fn install_loop_control(
        &self,
        control: std::sync::Weak<crate::owner::apple::macos::LoopControl>,
    ) {
        let _ = self.loop_control.set(control);
    }

    /// Return this window's callback slots.
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
/// enforcement. A free function rather than a method so it can be exercised
/// on any lane without constructing a real window.
pub(crate) fn route_on_owner<R: Send>(
    owner: &'static dispatch::Queue,
    owner_is_main: bool,
    f: impl FnOnce() -> R + Send,
) -> R {
    crate::owner::apple::macos::exec_on_owner(owner, owner_is_main, f)
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
/// `display_pass` for the measurement behind that. The same call one turn
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
    if display_pass::in_display_pass() {
        defer();
        return;
    }
    send_inline();
}

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
            crate::owner::apple::macos::on_owner_thread(self.owner, self.owner_is_main),
            "BUG: set_needs_display must run on the window's owner lane"
        );
        unsafe {
            // SAFETY: per the function's contract, the caller is on the owner
            // lane (re-checked by the assertion above) and `ns_window` is live,
            // so its view hierarchy, which holds `self.view`, is too. The view
            // is NIL-checked before messaging.
            let content_view = self.view;
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
        windows_map: WindowRegistry,
        key: u64,
        hop: u8,
    ) {
        crate::owner::apple::macos::exec_async_guarded(owner, move || {
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
                if display_pass::in_display_pass() {
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
                let tracked = windows_map.lock().get(&key).cloned();
                // Every window the map holds is a `MacOSWindow`: only this
                // file's constructor adds one.
                if let Some(window) = tracked
                    .as_deref()
                    .and_then(|window| window.as_any().downcast_ref::<MacOSWindow>())
                {
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

impl crate::host_window::CoreHostWindow for MacOSWindow {}

impl crate::host_window::HostWindow for MacOSWindow {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// The window's own NSAccessibility bridge — the capability the
    /// composition root's accessibility wire discovers. Without this
    /// override the trait default (`None`) leaves every real macOS window
    /// silently invisible to VoiceOver.
    #[cfg(feature = "a11y")]
    fn accessibility(&self) -> Option<Arc<dyn flui_semantics::platform::PlatformAccessibility>> {
        self.accessibility
            .get()
            .map(|bridge| Arc::clone(bridge) as _)
    }
}

impl CallbackWindow for MacOSWindow {
    fn callbacks(&self) -> &WindowCallbacks {
        &self.callbacks
    }
}

/// What the macOS windows open with: the window configuration the platform
/// was built with. The loop keeps the open windows themselves
/// (`WindowRegistry`), because its wake pump and its stop reach them.
#[derive(Debug, Default)]
pub struct MacOSWindows {
    config: WindowConfiguration,
}

impl MacOSWindows {
    /// The windows of a platform built with `config`.
    pub(crate) fn new(config: WindowConfiguration) -> Self {
        Self { config }
    }
}

/// Turn a position and a normal rectangle measured on a display into global
/// logical pixels, which is all a window knows how to place. A display that is
/// gone leaves them global, as the options document.
fn measure_placement_globally(displays: &dyn HostDisplays, options: &mut WindowOptions) {
    let Some(display) = options.display.take() else {
        return;
    };
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Ok(snapshot) = displays.snapshot(mtm) else {
        return;
    };
    let display = snapshot.get(display);
    options.position = options.position.map(|at| globalize(at, display));
    if let Some(normal) = options.normal_geometry.as_mut() {
        normal.position = normal.position.map(|at| globalize(at, display));
    }
}

impl host::Window<MacOSLoop> for MacOSWindow {
    type Shared = MacOSWindows;

    fn open(
        owner: &MacOSLoop,
        windows: &Arc<MacOSWindows>,
        mut options: WindowOptions,
    ) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        owner
            .affinity
            .debug_assert_owner("MacOSPlatform::open_window");
        debug_assert_appkit_main_thread("MacOSPlatform::open_window");
        if !owner.loop_control.accepts_windows() {
            return Err(OpenWindowError::OwnerGone {
                rejected: Some(Box::new(options)),
            });
        }
        measure_placement_globally(owner.displays.as_ref(), &mut options);
        let window = MacOSWindow::new(options, Arc::clone(&owner.windows), windows.config.clone())?;
        owner.notices.mark_stale_on_wake(window.state_cell());
        // Installed immediately after construction, before the window can be
        // handed to anyone who might redraw it — the slot is a `OnceLock`, so
        // a later install would be silently ignored rather than racing.
        window.install_loop_control(Arc::downgrade(&owner.loop_control));
        window.install_wake_pump(Arc::downgrade(&owner.wake_pump));
        window.bind_displays(Arc::clone(&owner.displays));
        let callbacks = window.callbacks();
        callbacks.set_input_capabilities(crate::input::availability::macos());
        callbacks.set_input_wake(crate::owner::signal::OwnerSignal::callbacks_wake(
            &owner.loop_control.owner_signal,
            callbacks,
        ));

        Ok(window)
    }

    fn active(owner: &MacOSLoop, _: &MacOSWindows) -> Option<WindowId> {
        owner
            .affinity
            .debug_assert_owner("MacOSPlatform::active_window");
        debug_assert_appkit_main_thread("MacOSPlatform::active_window");
        // `keyWindow` answers `None` when no window is key. The window id is
        // the native `NSWindow` pointer, the same identity `MacOSWindow`
        // reports, so a caller can compare them.
        let key_window = owner.app.keyWindow()?;
        let ptr = objc2::rc::Retained::as_ptr(&key_window) as u64;
        Some(WindowId(ptr))
    }

    fn capabilities(_: &MacOSLoop, _: &MacOSWindows) -> WindowCapabilities {
        window_capabilities()
    }

    fn frame_inset(owner: &MacOSLoop, _: &MacOSWindows, _scale_factor: f64) -> Size<f64> {
        owner
            .affinity
            .debug_assert_owner("MacOSPlatform::window_frame_inset");
        debug_assert_appkit_main_thread("MacOSPlatform::window_frame_inset");
        // AppKit measures a frame in points, so the scale does not enter.
        let main = MainThreadMarker::new().expect(
            "BUG: the owner thread is the AppKit main thread, which the assertions above check",
        );
        decorated_frame_inset(main)
    }

    fn on_event(owner: &MacOSLoop, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        install(
            &owner.handler(|handlers| &mut handlers.window_event),
            callback,
        );
    }
}

/// What every AppKit window honours and observes, known before one exists
/// (`Platform::window_capabilities`). Each field is named, so a command added
/// to the contract is `false` here until `apply` carries it out.
const fn window_capabilities() -> WindowCapabilities {
    let mut capabilities = WindowCapabilities::NONE;
    capabilities.sets_title = true;
    capabilities.sets_position = true;
    capabilities.sets_size = true;
    capabilities.sets_constraints = true;
    capabilities.sets_resizable = true;
    capabilities.sets_decorated = true;
    capabilities.minimizes = true;
    capabilities.maximizes = true;
    capabilities.enters_fullscreen = true;
    capabilities.hides = true;
    capabilities.shows = true;
    capabilities.activates = true;
    capabilities.observes_position = true;
    capabilities.observes_minimized = true;
    capabilities.places_at_open = true;
    capabilities
}

/// The class every window is an instance of: `NSWindow`, except that it can
/// take keyboard focus without a title bar. A plain borderless `NSWindow`
/// answers `canBecomeKeyWindow` with `NO`, which would leave a window that is
/// opened undecorated, or made undecorated later, deaf to the keyboard.
fn window_class() -> &'static Class {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        extern "C-unwind" fn takes_focus(_this: &Object, _sel: Sel) -> Bool {
            YES
        }

        let mut decl = ClassBuilder::new(c"FLUIWindow", class!(NSWindow))
            .expect("BUG: FLUIWindow must be registered exactly once (guarded by Once)");
        // SAFETY: both selectors are `NSWindow` getters returning `BOOL` and
        // taking no argument, which `takes_focus` matches.
        unsafe {
            decl.add_method(
                sel!(canBecomeKeyWindow),
                takes_focus as extern "C-unwind" fn(_, _) -> Bool,
            );
            decl.add_method(
                sel!(canBecomeMainWindow),
                takes_focus as extern "C-unwind" fn(_, _) -> Bool,
            );
        }
        decl.register();
    });

    AnyClass::get(c"FLUIWindow").expect("BUG: FLUIWindow was registered by the Once block above")
}

/// How much larger a titled, closable, miniaturizable and resizable window's
/// frame is than its content, in points: the title bar. Asked of the class, so
/// it is known before any window exists. The marker is the proof that the
/// caller is on the main thread, where every AppKit message in this module runs.
fn decorated_frame_inset(_main: MainThreadMarker) -> Size<f64> {
    let mask = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
    // SAFETY: `frameRectForContentRect:styleMask:` takes a rectangle and a mask
    // by value and returns a rectangle, and it messages no window; the main
    // thread is witnessed by the marker.
    let frame: NSRect = unsafe {
        msg_send![
            class!(NSWindow),
            frameRectForContentRect: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
            styleMask: mask
        ]
    };
    Size::new(frame.size.width, frame.size.height)
}

/// The height of the primary screen (`NSScreen.screens[0]`), whose bottom-left
/// corner is the origin AppKit measures every frame from; `None` with no screen.
///
/// # Safety
///
/// Must run on the owner thread, like every AppKit message in this file.
unsafe fn primary_screen_height() -> Option<f64> {
    // SAFETY: per the function contract; `screens` is an NSArray of NSScreen
    // and `objectAtIndex:` is only sent when it holds one.
    unsafe {
        let screens: ObjcId = msg_send![class!(NSScreen), screens];
        let count: usize = msg_send![screens, count];
        if count == 0 {
            return None;
        }
        let first: ObjcId = msg_send![screens, objectAtIndex: 0usize];
        let frame: NSRect = msg_send![first, frame];
        Some(frame.size.height)
    }
}

impl PlatformWindow for MacOSWindow {
    fn id(&self) -> WindowId {
        WindowId(self.ns_window as u64)
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::PlatformHaptics>> {
        Some(Arc::new(crate::haptics::apple::MacHaptics))
    }

    /// The window's IME capability — the `NSTextInputClient` conformance the
    /// content view carries (see `text::apple::macos`). Without this override
    /// the trait default (`None`) leaves the native backend with no IME at all
    /// while the optional winit fallback has one.
    ///
    /// Built once and cached, like the accessibility bridge above, but lazily
    /// rather than in the constructor: the capability needs nothing the
    /// constructor has not already produced by the time the window value
    /// exists, and a window that never attaches a text input never builds one.
    /// `closed` is shared rather than copied so the capability observes the
    /// window's own close.
    fn text_input(&self) -> Option<Arc<dyn flui_platform::PlatformTextInput>> {
        let text_input = self.text_input.get_or_init(|| {
            Arc::new(crate::text::apple::macos::MacOSTextInput::new(
                self.view,
                Arc::clone(&self.closed),
                self.owner,
                self.owner_is_main,
            ))
        });
        use crate::text::{TextInput as _, WindowTextInput};
        text_input
            .window_text_input()
            .and_then(WindowTextInput::ime)
    }

    fn physical_size(&self) -> Size<i32> {
        let state = self.state.lock();
        let logical = state.bounds.size;
        let scale = state.scale_factor;
        Size::new(
            (logical.width * scale).round() as i32,
            (logical.height * scale).round() as i32,
        )
    }

    fn logical_size(&self) -> Size<f64> {
        let state = self.state.lock();
        state.bounds.size
    }

    fn scale_factor(&self) -> f64 {
        let state = self.state.lock();
        state.scale_factor
    }

    fn display_id(&self) -> Option<DisplayId> {
        self.state.lock().display
    }

    /// # Thread affinity — owner-routed
    ///
    /// This method is called from whatever thread completed a future, through
    /// the scheduler's `on_frame_scheduled` hook (issue #949), so its AppKit
    /// body — `setNeedsDisplay:` on the window's view — is dispatched onto the
    /// window's owner lane (the AppKit main queue for production windows, a
    /// caller-supplied serial lane for test windows; see
    /// `owner::apple::macos::owner_lane`) and executes on the owner thread,
    /// never bare on a caller on another thread.
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

    fn appearance(&self) -> WindowAppearance {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread before it is messaged.
            let window = unsafe { &*self.ns_window };
            // The window's appearance, not NSApp's: a window may override it.
            view::effective_appearance(
                window
                    .downcast_ref::<objc2_app_kit::NSWindow>()
                    .expect("BUG: MacOSWindow wraps an NSWindow"),
            )
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

    // ==================== State and commands (ADR-0237) ====================

    fn state(&self) -> WindowState {
        self.current_state()
    }

    fn capabilities(&self) -> WindowCapabilities {
        window_capabilities()
    }

    fn apply(&self, command: WindowCommand) -> Result<(), WindowCommandError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(WindowCommandError::Closed);
        }
        command.admit(&PlatformWindow::capabilities(self))?;
        match command {
            WindowCommand::SetTitle(title) => self.set_native_title(&title),
            WindowCommand::SetPosition(at) => self.set_native_position(at)?,
            WindowCommand::SetSize(size) => self.set_content_size(size),
            WindowCommand::SetConstraints(constraints) => self.set_native_constraints(constraints),
            WindowCommand::SetResizable(on) => {
                self.set_style_bit(NSWindowStyleMask::Resizable, on)?;
            }
            WindowCommand::SetDecorated(on) => {
                self.set_style_bit(NSWindowStyleMask::Titled, on)?;
            }
            WindowCommand::SetMode(mode) => self.change_mode(mode)?,
            WindowCommand::Show => self.show().map_err(|error| match error {
                flui_platform::WindowShowError::Closed => WindowCommandError::Closed,
                flui_platform::WindowShowError::Native { message } => {
                    WindowCommandError::Backend { message }
                }
                _ => WindowCommandError::Unsupported,
            })?,
            WindowCommand::Hide => self.hide()?,
            WindowCommand::Activate => self.activate(),
            // A command added to the contract that this backend has not met
            // is refused, not ignored.
            _ => return Err(WindowCommandError::Unsupported),
        }
        self.publish_state();
        Ok(())
    }

    fn show(&self) -> Result<(), flui_platform::WindowShowError> {
        // An explicit show settles a deferred first reveal: the window is
        // made opaque along with being ordered.
        let settle_reveal = self.take_pending_first_reveal();
        let shown = route_on_owner(self.owner, self.owner_is_main, || {
            if self.closed.load(Ordering::SeqCst) {
                return Err(flui_platform::WindowShowError::Closed);
            }
            // SAFETY: self retains the native window; routing establishes its
            // owner lane. Deminiaturizing and ordering do not toggle zoom/fullscreen.
            unsafe {
                if settle_reveal {
                    let _: () = msg_send![self.ns_window, setAlphaValue: 1.0f64];
                }
                let minimized: bool = msg_send![self.ns_window, isMiniaturized];
                if minimized {
                    let _: () = msg_send![self.ns_window, deminiaturize: NIL];
                }
                if self.closed.load(Ordering::SeqCst) {
                    return Err(flui_platform::WindowShowError::Closed);
                }
                let _: () = msg_send![self.ns_window, makeKeyAndOrderFront: NIL];
            }
            if self.closed.load(Ordering::SeqCst) {
                Err(flui_platform::WindowShowError::Closed)
            } else {
                Ok(())
            }
        });
        if shown.is_ok() {
            self.enter_mode_when_shown();
        }
        shown
    }

    fn reveal_after_first_frame(&self) {
        // The flag is settled under the state lock and outside the owner
        // route, so a second caller racing this one sees `false`: the
        // window becomes opaque exactly once for its deferred reveal.
        if !self.take_pending_first_reveal() || self.closed.load(Ordering::SeqCst) {
            return;
        }
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // message is sent.
            let _: () = msg_send![self.ns_window, setAlphaValue: 1.0f64];
        });
        tracing::debug!("macOS: window revealed after its first presented frame");
    }

    fn activate(&self) {
        let settle_reveal = self.take_pending_first_reveal();
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // message is sent.
            if settle_reveal {
                let _: () = msg_send![self.ns_window, setAlphaValue: 1.0f64];
            }
            let _: () = msg_send![self.ns_window, makeKeyAndOrderFront: NIL];
        });
        self.enter_mode_when_shown();
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

    crate::owner::handlers::impl_window_callback_setters!(callbacks);

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
        // the NSWindow: the FLUI view, which stays the surface view while
        // Liquid Glass is applied.
        let ns_view = NonNull::new(self.view.cast::<std::ffi::c_void>())
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
            view: self.view,
            state: Arc::clone(&self.state),
            windows_map: Arc::clone(&self.windows_map),
            callbacks: Arc::clone(&self.callbacks),
            closed: Arc::clone(&self.closed),
            loop_control: self.loop_control.clone(),
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
            // A clone shares the window, so it names the same display.
            displays: self.displays.clone(),
            snapshot: Arc::clone(&self.snapshot),
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

        // On-owner teardown executes now, including after NSApplication.run
        // returns. The close delegate pins its native receiver in the outer
        // autorelease pool, so a final callback-local wrapper drop cannot
        // deallocate the receiver under AppKit's close stack. Off-owner Drop
        // remains nonblocking; an unserviced owner queue can still leak its
        // retained native tail, never release AppKit objects on a worker.
        let owner = self.owner;
        let ns_window = OwnerLaneId(self.ns_window);
        #[cfg(feature = "a11y")]
        let a11y = self.accessibility.get().cloned(); // Option<Arc<...>>, Send
        let teardown = move || unsafe {
            // SAFETY: the tail runs on the owner thread, directly or under its
            // queue guard, so
            // the a11y unhook and the Window release both execute
            // owner-affine. `shutdown()` runs before the release in the same
            // block, preserving accesskit's documented precondition (unhook
            // the dynamic subclass while the content view is still alive).
            #[cfg(feature = "a11y")]
            if let Some(a11y) = a11y {
                // A bridge failure must not skip the native window's balancing
                // release. Keep it inside its own cleanup boundary.
                crate::owner::apple::macos::run_cleanup_guarded(|| a11y.shutdown());
            }
            // SAFETY: `send_release` consumes the whole `OwnerLaneId` (the
            // closure's capture is then that `Send` newtype, never its raw
            // field), and its own contract — live still-retained window, send
            // on the owner lane — is met here: the wrapper was built from this
            // value's `ns_window` before the last clone dropped, and this body
            // runs on the owner thread.
            ns_window.send_release();
        };
        if crate::owner::apple::macos::on_owner_thread(owner, self.owner_is_main) {
            crate::owner::apple::macos::run_cleanup_guarded(teardown);
        } else {
            crate::owner::apple::macos::exec_async_guarded(owner, move || {
                crate::owner::apple::macos::run_cleanup_guarded(teardown);
            });
        }
    }
}

// ============================================================================
// Window routing tests — always-run wrapper pin + `#[ignore]`d site tests
// ============================================================================

impl MacOSWindow {
    /// The window's published state, read from AppKit again first when
    /// something invalidated it; `on_state_changed` is raised when that
    /// produced a new revision.
    fn current_state(&self) -> WindowState {
        let published = self.snapshot.state_with(|| self.sample_state());
        if published.changed {
            self.callbacks.dispatch_state_changed();
        }
        published.state
    }

    /// The window may differ from its published state: read it again.
    fn publish_state(&self) {
        self.snapshot.invalidate();
        let _ = self.current_state();
    }

    /// The window as AppKit reports it now.
    fn sample_state(&self) -> WindowState {
        let (display, normal) = {
            let state = self.state.lock();
            (state.display, state.normal)
        };
        route_on_owner(self.owner, self.owner_is_main, || {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for
            // a main-lane owner, or dispatched onto the lane under the
            // reentrancy guard — before the messages are sent. `title`
            // returns an autoreleased NSString whose UTF8String buffer is
            // copied before returning.
            unsafe {
                let frame: NSRect = msg_send![self.ns_window, frame];
                let content: NSRect = msg_send![self.ns_window, contentRectForFrameRect: frame];
                let flags = self.read_flags();
                let on_screen: bool = msg_send![self.ns_window, isVisible];
                let focused: bool = msg_send![self.ns_window, isKeyWindow];
                let min: NSSize = msg_send![self.ns_window, contentMinSize];
                let max: NSSize = msg_send![self.ns_window, contentMaxSize];
                let ns_title: ObjcId = msg_send![self.ns_window, title];
                let title = if ns_title == NIL {
                    String::new()
                } else {
                    let c_str: *const i8 = msg_send![ns_title, UTF8String];
                    if c_str.is_null() {
                        String::new()
                    } else {
                        std::ffi::CStr::from_ptr(c_str)
                            .to_string_lossy()
                            .into_owned()
                    }
                };
                let mask: NSWindowStyleMask = msg_send![self.ns_window, styleMask];
                let mode = mode_of(flags);
                let position = primary_screen_height().and_then(|height| {
                    top_left_of_frame(crate::display::apple::macos::bounds_of(frame), height)
                });
                WindowState::new(
                    WindowGeometry::new(position, Size::new(frame.size.width, frame.size.height)),
                    Size::new(content.size.width, content.size.height),
                )
                .with_mode(mode)
                .with_normal_geometry(if mode == WindowMode::Normal {
                    None
                } else {
                    normal
                })
                .with_visible(is_shown(on_screen, flags.miniaturized))
                .with_focused(focused)
                .with_title(Some(title))
                .with_resizable(flags.resizable)
                .with_decorated(mask.contains(NSWindowStyleMask::Titled))
                .with_constraints(constraints_of_content_limits(
                    Size::new(min.width, min.height),
                    Size::new(max.width, max.height),
                ))
                .with_display(display)
            }
        })
    }

    /// What AppKit says about the window's mode, as flags.
    ///
    /// # Safety
    ///
    /// Owner lane only, like every AppKit message in this file.
    unsafe fn read_flags(&self) -> WindowFlags {
        // SAFETY: per the function contract; `ns_window` is alive for the
        // lifetime of `self`.
        unsafe {
            let mask: NSWindowStyleMask = msg_send![self.ns_window, styleMask];
            WindowFlags {
                fullscreen: mask.contains(NSWindowStyleMask::FullScreen),
                miniaturized: msg_send![self.ns_window, isMiniaturized],
                zoomed: msg_send![self.ns_window, isZoomed],
                resizable: mask.contains(NSWindowStyleMask::Resizable),
            }
        }
    }

    /// The window's outer rectangle now.
    fn outer_geometry(&self) -> WindowGeometry {
        route_on_owner(self.owner, self.owner_is_main, || {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner lane.
            unsafe {
                let frame: NSRect = msg_send![self.ns_window, frame];
                WindowGeometry::new(
                    primary_screen_height().and_then(|height| {
                        top_left_of_frame(crate::display::apple::macos::bounds_of(frame), height)
                    }),
                    Size::new(frame.size.width, frame.size.height),
                )
            }
        })
    }

    /// Remember the outer rectangle if the window is in normal mode: the
    /// window is about to leave it, and AppKit keeps no record of where it
    /// returns to.
    fn record_normal(&self) {
        self.record_normal_if(|flags| mode_of(flags) == WindowMode::Normal);
    }

    /// [`record_normal`](Self::record_normal) for `windowWillEnterFullScreen:`,
    /// which may already see the full-screen style bit while the frame is still
    /// the normal one: a window counts as normal if it is neither miniaturized
    /// nor maximized.
    fn record_normal_entering_fullscreen(&self) {
        self.record_normal_if(|flags| !(flags.miniaturized || flags.is_maximized()));
    }

    fn record_normal_if(&self, is_normal: impl FnOnce(WindowFlags) -> bool + Send) {
        let in_normal_mode = route_on_owner(self.owner, self.owner_is_main, || {
            // SAFETY: `ns_window` is alive; the body runs on the owner lane.
            is_normal(unsafe { self.read_flags() })
        });
        if in_normal_mode {
            let outer = self.outer_geometry();
            self.state.lock().normal = Some(outer);
        }
    }

    fn set_native_title(&self, title: &str) {
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

    /// Move the window's outer top-left corner.
    fn set_native_position(&self, at: Point<f64>) -> Result<(), WindowCommandError> {
        route_on_owner(self.owner, self.owner_is_main, || {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner lane.
            unsafe {
                let height =
                    primary_screen_height().ok_or_else(|| WindowCommandError::Backend {
                        message: "there is no screen to measure a position from".into(),
                    })?;
                let point = cocoa_top_left_point(at, height);
                let _: () = msg_send![
                    self.ns_window,
                    setFrameTopLeftPoint: NSPoint::new(point.x, point.y)
                ];
                Ok(())
            }
        })
    }

    /// Resize the content area, not the frame: on decorated windows a
    /// frame-sized `setFrame:` would shrink the content by the title bar.
    fn set_content_size(&self, size: Size<f64>) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for
            // a main-lane owner, or dispatched onto the lane under the
            // reentrancy guard — before the message is sent.
            let ns_size = NSSize::new(size.width, size.height);
            let _: () = msg_send![self.ns_window, setContentSize: ns_size];
        });
    }

    fn set_native_constraints(&self, constraints: SizeConstraints) {
        let (min, max) = content_limits_of_constraints(constraints);
        route_on_owner(self.owner, self.owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner lane.
            let _: () =
                msg_send![self.ns_window, setContentMinSize: NSSize::new(min.width, min.height)];
            let _: () =
                msg_send![self.ns_window, setContentMaxSize: NSSize::new(max.width, max.height)];
        });
    }

    /// Switch one style-mask bit (resizable, titled) on or off, keeping the
    /// content area where it is: removing the title bar shrinks the frame, not
    /// the content.
    fn set_style_bit(&self, bit: NSWindowStyleMask, on: bool) -> Result<(), WindowCommandError> {
        route_on_owner(self.owner, self.owner_is_main, || {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner lane.
            unsafe {
                let mask: NSWindowStyleMask = msg_send![self.ns_window, styleMask];
                if mask.contains(NSWindowStyleMask::FullScreen) {
                    return Err(WindowCommandError::Backend {
                        message: "AppKit does not restyle a window while it is full screen".into(),
                    });
                }
                // A window that is no longer resizable reports a zoomed frame
                // as normal and cannot be un-zoomed by the user, so it is
                // restyled only from the mode it can be asked to return to.
                let flags = self.read_flags();
                if bit == NSWindowStyleMask::Resizable
                    && !on
                    && mode_of(flags) != WindowMode::Normal
                {
                    return Err(WindowCommandError::Backend {
                        message: "a window is made fixed-size only in normal mode".into(),
                    });
                }
                let wanted = if on { mask | bit } else { mask - bit };
                if wanted == mask {
                    return Ok(());
                }
                let frame: NSRect = msg_send![self.ns_window, frame];
                let content: NSRect = msg_send![self.ns_window, contentRectForFrameRect: frame];
                let _: () = msg_send![self.ns_window, setStyleMask: wanted];
                let restyled: NSRect = msg_send![self.ns_window, frameRectForContentRect: content];
                let _: () = msg_send![self.ns_window, setFrame: restyled, display: YES];
                Ok(())
            }
        })
    }

    /// Order the window out. A full-screen window is refused: ordering it out
    /// leaves its empty Space behind.
    fn hide(&self) -> Result<(), WindowCommandError> {
        route_on_owner(self.owner, self.owner_is_main, || {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner lane.
            unsafe {
                if self.read_flags().fullscreen {
                    return Err(WindowCommandError::Backend {
                        message: "a full-screen window leaves full screen before it is hidden"
                            .into(),
                    });
                }
                let _: () = msg_send![self.ns_window, orderOut: NIL];
                Ok(())
            }
        })
    }

    /// Take the window to `mode` from whichever mode it is in. AppKit's zoom
    /// and full-screen requests are toggles, so each is sent only from the
    /// state it undoes, planned from the window's flags now.
    ///
    /// A step that animates (full screen, the Dock) is not interrupted: a
    /// command that arrives meanwhile, or the rest of a mode that needs the
    /// step to finish first, is held and planned again from the window's new
    /// state when its `windowDid…` notification arrives. The newest command
    /// wins, and it also replaces the mode a hidden window was to enter when
    /// first shown.
    fn change_mode(&self, mode: WindowMode) -> Result<(), WindowCommandError> {
        let flags = route_on_owner(self.owner, self.owner_is_main, || {
            // SAFETY: `ns_window` is alive; the body runs on the owner lane.
            unsafe { self.read_flags() }
        });
        if !flags.resizable && matches!(mode, WindowMode::Maximized | WindowMode::Fullscreen) {
            return Err(WindowCommandError::Backend {
                message: "AppKit does not zoom or full-screen a window that cannot be resized"
                    .into(),
            });
        }
        let steps = self
            .state
            .lock()
            .sequence
            .command(std::time::Instant::now(), flags, mode);
        if !steps.is_empty() {
            self.record_normal();
        }
        route_on_owner(self.owner, self.owner_is_main, || {
            for step in &steps {
                // SAFETY: `ns_window` is alive; the body runs on the owner
                // lane. Each message is the AppKit request the step names.
                unsafe {
                    match step {
                        ModeStep::Deminiaturize => {
                            let _: () = msg_send![self.ns_window, deminiaturize: NIL];
                        }
                        ModeStep::Miniaturize => {
                            let _: () = msg_send![self.ns_window, miniaturize: NIL];
                        }
                        ModeStep::Zoom | ModeStep::Unzoom => {
                            let _: () = msg_send![self.ns_window, zoom: NIL];
                        }
                        ModeStep::EnterFullscreen | ModeStep::ExitFullscreen => {
                            let _: () = msg_send![self.ns_window, toggleFullScreen: NIL];
                        }
                    }
                }
            }
        });
        Ok(())
    }

    /// A window that opened hidden enters the mode it opened in now that it
    /// is shown.
    fn enter_mode_when_shown(&self) {
        let waiting = self.state.lock().sequence.take_when_shown();
        if let Some(mode) = waiting
            && let Err(error) = self.change_mode(mode)
        {
            tracing::debug!(%error, "a window could not enter the mode it opened in");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let _pass = display_pass::DisplayPassGuard::enter();
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
}
