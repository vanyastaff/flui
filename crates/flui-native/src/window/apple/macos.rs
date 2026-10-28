//! The macOS window: [`MacOSWindow`] over an `NSWindow` (its state as AppKit
//! reports it, and the requests its commands send; whether the thread is
//! inside a view's display pass, the owner's `display_pass`, decides how a
//! redraw is asked for), and [`MacOSWindows`], what the windows open with.
//!
//! Three parts live beside it: [`delegate`], the window's `NSWindowDelegate`,
//! one AppKit object with the handlers of what it reports; [`ext`],
//! [`MacOSWindowExt`] and the [`AppKitWindow`] it gives, the public AppKit
//! features beyond the cross-platform window; and [`backdrop`], the view
//! behind the content ([`MacOSBackdrop`]: a material or Liquid Glass).
//!
//! The loop holds the windows it tracks as `HostWindow`s
//! (`owner::apple::macos`). A window lives on the AppKit main thread, its
//! owner; the thread that presents its frames holds its `MacOSSurface`, and
//! any thread asks it for a frame through its `MacOSRedraw` (ADR-0254 §5).

// Direct AppKit objc calls have no safe wrapper: the window is one of the
// crate's `unsafe` FFI islands, opted out at its module boundary.
#![expect(unsafe_code)]
// The window still sends raw `msg_send!` in objc2's comma-less form and reads
// its delegate's ivar by name, both deprecated by objc2 in favour of the typed
// API this file has not moved to yet.
#![expect(deprecated)]

pub(crate) mod backdrop;
pub(crate) mod delegate;
pub(crate) mod ext;

pub use backdrop::{
    GlassStyle, GlassTint, LiquidGlass, MacOSBackdrop, MacOSBackdropError, MacOSMaterial,
};
use delegate::{create_window_delegate, display_of_screen};
pub use ext::{
    AppKitWindow, MacOSCollectionBehavior, MacOSWindowExt, MacOSWindowLevel, TabJoinError,
};

use std::cell::OnceCell;
use std::mem::ManuallyDrop;
use std::rc::Rc;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use cursor_icon::CursorIcon;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{MainThreadMarker, msg_send};
use objc2::{class, sel};
use objc2_app_kit::{NSBackingStoreType, NSWindowCollectionBehavior, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::{
    AdmittedCommand, ApplyError, SizeConstraints, WindowCapabilities, WindowCommand,
    WindowGeometry, WindowMode, WindowState,
};

use crate::window::apple::geometry::{
    frame::{
        cocoa_frame, cocoa_frame_keeping_top, cocoa_top_left_point, constraints_of_content_limits,
        content_limits_of_constraints, globalize, top_left_of_frame,
    },
    mode::{
        ANIMATION_LIMIT, FixedSize, ModeSequence, ModeStep, WindowFlags, capabilities_of, is_shown,
    },
};
use crate::window::capabilities::{appkit_window_capabilities, frame_rate_absence};
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

use crate::host::{self, WindowFacets, WindowPositions};
use crate::host_window::HostWindow;
use crate::owner::apple::MacOSLoop;
use crate::owner::apple::macos::content_view::{self, ViewHooks};
use crate::owner::apple::macos::display_pass;
use crate::owner::apple::macos::{
    ContentViewRef, HostDisplays, WeakWakePump, WindowRegistry, debug_assert_appkit_main_thread,
    exec_async_on_owner, on_owner_thread,
};

use crate::{
    owner::OpenWindowError,
    owner::slot::OwnerWeak,
    owner::window_delivery::{Retirement, WindowDelivery},
    window::state::{ReportedFacts, WindowStateCell, mode_of},
};
use flui_platform::{
    CursorError, DisplayId, PlatformWindow, RedrawRequester, SurfaceTarget, WindowHandler,
    WindowId, WindowOptions, WindowReveal,
};

/// macOS window wrapper around NSWindow.
///
/// Neither `Send` nor `Sync` (ADR-0254 §5): it is made on the AppKit main
/// thread, its owner, and every AppKit message it sends is sent there, because
/// no other thread can reach it. What other threads need of it is handed out
/// apart from it: its `MacOSSurface` and its `MacOSRedraw`.
pub struct MacOSWindow {
    /// Native window handle (NSWindow*)
    ns_window: ObjcId,

    /// This window's `FLUIContentView` (NSView*): the view that receives
    /// input, IME and appearance changes, draws on `updateLayer`, carries the
    /// accessibility bridge, and is the `ns_view` of the raw window handle.
    ///
    /// Fixed for the window's lifetime and read from here, never from
    /// `-[NSWindow contentView]`: a backdrop material makes an
    /// `NSVisualEffectView` the content view and re-parents this view inside
    /// it. The wrapper owns it: it outlives every re-parenting, and the
    /// window's [`Drop`] releases it after detaching what is hooked to it and
    /// releasing the NSWindow, or keeps it if a detach failed.
    view: ManuallyDrop<Retained<AnyObject>>,

    /// Window state
    state: Mutex<MacOSWindowState>,

    /// Reference to all windows
    windows_map: WindowRegistry,

    /// Per-window callbacks (input, resize, close, ...)
    delivery: Arc<WindowDelivery>,

    /// Set once `windowWillClose:` has fired for this window, and by the
    /// window's [`Drop`] before it releases the NSWindow. Shared with the
    /// window's surface targets and with the features beside it
    /// ([`ContentViewRef`]), which outlive the window: with
    /// `setReleasedWhenClosed:NO` set at construction, `-[NSWindow close]` no
    /// longer deallocates `ns_window`, so its view stays answerable long after
    /// the window is meaningless to hand a GPU handle for, and only this flag
    /// (not a nil check) can tell the two apart.
    closed: Arc<AtomicBool>,
    loop_control: OnceLock<std::sync::Weak<crate::owner::apple::macos::LoopControl>>,

    /// The window's owner lane (see `owner::apple::macos::owner_lane`): the
    /// AppKit main queue, which runs on the thread the window lives on. What
    /// reaches the window later or from another thread — a deferred redraw, a
    /// held mode change, the features beside it — is queued here.
    owner: &'static dispatch::Queue,

    /// True when `owner` is the main queue, so "on the main thread" is
    /// equivalent to "on the owner lane" for
    /// [`crate::owner::apple::macos::on_owner_thread`].
    owner_is_main: bool,

    /// What the table's positions made for this window once its content view
    /// was installed: the accessibility bridge, subclassed onto that view;
    /// the IME capability, the `NSTextInputClient` conformance the view
    /// carries; the actuator; and the system chrome.
    facets: WindowFacets,

    /// What asks this window for a frame from any thread, and arms the
    /// platform's wake pump as it does. Set once, right after the window is
    /// made, since it reaches the window through a weak reference to it.
    redraw: OnceCell<Arc<MacOSRedraw>>,

    /// The platform's display registry, by which this window turns its
    /// screen into a [`DisplayId`]. A `OnceLock` filled once by
    /// the window position's `open` right after construction, like the wake
    /// pump; a window built without a platform names no display.
    displays: OnceLock<Arc<dyn HostDisplays>>,

    /// The published window state (ADR-0237). Seeded from the creation
    /// options; the first read replaces it with what AppKit reports.
    snapshot: Arc<WindowStateCell>,
}

/// Mutable window state
struct MacOSWindowState {
    /// Current window bounds (logical pixels)
    bounds: Bounds<f64>,

    /// Scale factor (1.0 for non-Retina, 2.0 for Retina)
    scale_factor: f64,

    /// The display this window is on. Cached, not queried per call: reading it
    /// messages the window's screen, which is AppKit traffic this backend keeps
    /// on the owner lane, while `PlatformWindow::state` is a plain `&self`
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

    /// The visible bit of the window's `occlusionState`, as
    /// `windowDidChangeOcclusionState:` last read it: a window fully covered,
    /// on another space or hidden is not visible in its snapshot. Starts
    /// `true` — a freshly created window is visible until AppKit says
    /// otherwise.
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
    // Hand-written: `ns_window` is a raw Objective-C `id` and `WindowDelivery`
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
/// `object`'s address, as AppKit messages take it.
fn objc_address(object: &AnyObject) -> ObjcId {
    std::ptr::from_ref(object).cast_mut()
}

fn defers_first_reveal(options: &WindowOptions) -> bool {
    options.visible && options.reveal == WindowReveal::AfterFirstFrame
}

impl MacOSWindow {
    /// The window's FLUI view, as the address AppKit messages take.
    fn view_ptr(&self) -> ObjcId {
        objc_address(&self.view)
    }

    /// Clear the deferred-first-reveal flag, returning whether it was set:
    /// the one call that decides which caller performs the reveal. The state
    /// lock is released before the caller sends any AppKit message, so it is
    /// never held across one.
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
        handler: Rc<dyn WindowHandler>,
        windows_map: WindowRegistry,
        windows: &MacOSWindows,
        view_hooks: ViewHooks,
    ) -> Result<Rc<Self>, OpenWindowError> {
        // A window's owner lane is the AppKit main queue — the one lane all
        // other process AppKit traffic already serializes on (see
        // `owner::apple::macos::owner_lane`) — and the window is made on the
        // main thread, which that queue runs on.
        Self::new_inner(
            options,
            handler,
            windows_map,
            windows,
            view_hooks,
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
        // The layer's reports carry the refresh of the display the window is
        // on, from the snapshot: what the display source reads off the mode.
        let refresh = id
            .zip(objc2::MainThreadMarker::new())
            .and_then(|(id, mtm)| displays.snapshot(mtm).ok()?.get(id)?.refresh_period)
            .map(flui_platform::RefreshPeriod::get);
        content_view::set_layer_refresh(&self.view, refresh);
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
    fn install_wake_pump(&self, pump: WeakWakePump) {
        // A second install is silently ignored rather than racing the first:
        // the slot is read on every redraw, so "first writer wins" is the
        // only outcome that cannot leave a window flipping between pumps.
        let _ = self.redraw().wake_pump.set(pump);
    }

    /// The window's redraw requester.
    fn redraw(&self) -> &Arc<MacOSRedraw> {
        self.redraw
            .get()
            .expect("BUG: a window's redraw requester is set as the window is made")
    }

    fn new_inner(
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
        windows_map: WindowRegistry,
        windows: &MacOSWindows,
        view_hooks: ViewHooks,
        owner: &'static dispatch::Queue,
        owner_is_main: bool,
    ) -> Result<Rc<Self>, OpenWindowError> {
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
                        crate::owner::apple::macos::bounds_of(current),
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

            let delivery = Arc::new(WindowDelivery::new(
                handler,
                crate::input::availability::macos(),
            ));

            // Create content view for input events. `frame` is
            // objc2-foundation's `NSRect`, the type `create_content_view` takes.
            let content_view = content_view::create(frame, Arc::downgrade(&delivery), view_hooks);

            // What the features beside the window make their part of it from.
            let closed = Arc::new(AtomicBool::new(false));
            let this = ContentViewRef {
                view: objc_address(&content_view),
                closed: Arc::clone(&closed),
                owner,
                owner_is_main,
            };
            let _: () = msg_send![ns_window, setContentView: &*content_view];

            // Made once the content view is installed: the bridge subclasses
            // it for VoiceOver, on the main thread, and the window's teardown
            // detaches it before releasing the window and then the view.
            let facets = windows.positions.for_window(&this);

            let window = Rc::new(Self {
                ns_window,
                view: ManuallyDrop::new(content_view),
                state: Mutex::new(MacOSWindowState {
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
                }),
                windows_map: Arc::clone(&windows_map),
                delivery,
                closed,
                loop_control: OnceLock::new(),
                owner,
                owner_is_main,
                facets,
                redraw: OnceCell::new(),
                displays: OnceLock::new(),
                snapshot: Arc::new(WindowStateCell::new(
                    WindowState::new(WindowGeometry::new(None, options.size), options.size)
                        .with_title(Some(options.title.clone()))
                        .with_constraints(SizeConstraints::new(options.min_size, options.max_size))
                        .with_resizable(options.resizable)
                        .with_decorated(options.decorated),
                )),
            });

            let redraw = Arc::new(MacOSRedraw {
                window: Arc::new(OwnerWeak::new(&window)),
                owner,
                owner_is_main,
                wake_pump: OnceLock::new(),
            });
            assert!(
                window.redraw.set(redraw).is_ok(),
                "BUG: a window's redraw requester is set once, as the window is made"
            );

            window
                .delivery
                .publish_through_owner(&window, Self::read_state);

            // Enable mouse tracking for mouse moved events
            content_view::enable_mouse_tracking(window.view_ptr());

            // Make content view first responder to receive keyboard events
            let _: Bool = msg_send![ns_window, makeFirstResponder: window.view_ptr()];

            // Set window delegate for lifecycle events
            let delegate = create_window_delegate(Rc::downgrade(&window));
            let _: () = msg_send![ns_window, setDelegate: delegate];

            // Store in windows map, which the owner thread alone reaches: the
            // window is made there.
            let window_id = ns_window as u64;
            let tracked: Rc<dyn HostWindow> = Rc::clone(&window) as _;
            if windows_map.insert(window_id, tracked).is_err() {
                tracing::error!("a macOS window was made off its owner thread and is not tracked");
            }

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
}

/// Choose which arm a redraw request takes on the owner thread — the
/// display-pass deferral, or the inline send — and start it.
///
/// Free-standing so an always-run test can pin the *choice* without an
/// NSWindow, which a bare test process cannot construct. Both arms arrive as
/// closures, so what a test observes is exactly which one started; what the
/// window's own `request_redraw` supplies for them is pinned by the bundled
/// frame-pump probe, which reads frames rather than closures.
///
/// AppKit discards a `setNeedsDisplay:` issued while it is displaying the view,
/// and the frame a display pass runs asks for its next frame from exactly there
/// (`updateLayer` → frame request → scheduler wake): that one request is dropped
/// and the pump stops for good — no frame, no wake, no frame. See
/// `display_pass` for the measurement behind that. The same call one turn
/// of the owner lane later is honoured instead, because the pass has unwound by
/// then; this is ADR-0039 §4(c) realised on AppKit — a wake that arrives while
/// the drain gate is closed defers rather than draining.
///
/// A request from any other moment is already outside the pass (a request from
/// another thread reaches the owner on a lane turn of its own), so it stays
/// inline rather than paying a turn. The marker's grain is the thread, not this
/// window's pass, so a request for another window issued while any view on this
/// thread is displaying also defers: conservative, and one lane turn late at
/// worst, never dropped.
fn dispatch_redraw_request(defer: impl FnOnce(), send_inline: impl FnOnce()) {
    if display_pass::in_display_pass() {
        defer();
        return;
    }
    send_inline();
}

impl MacOSWindow {
    /// Ask AppKit to display this window's content view.
    ///
    /// The one place `setNeedsDisplay:` is sent: a request outside a display
    /// pass sends it at once, and a deferred one a turn of the owner lane
    /// later. `deferred` records which of the two sent it, because the two are
    /// only distinguishable in a log by their timing otherwise, and telling
    /// "the frame re-armed itself and the pump continued" from "the frame
    /// re-armed itself and was discarded" is what this whole path exists to
    /// make observable.
    ///
    /// # Safety
    ///
    /// The caller must be on this window's owner lane: `ns_window` and its
    /// content view are thread-affine AppKit objects. A window is neither
    /// `Send` nor `Sync`, so whoever holds `&self` is on the thread that made
    /// it; the `debug_assert` below checks that this thread is the lane's.
    /// `&self` keeps `ns_window` live: the wrapper's balancing `release` runs
    /// only from [`Drop`].
    unsafe fn set_needs_display(&self, deferred: bool) {
        debug_assert!(
            on_owner_thread(self.owner, self.owner_is_main),
            "BUG: set_needs_display must run on the window's owner lane"
        );
        unsafe {
            // SAFETY: per the function's contract, the caller is on the owner
            // lane (checked by the assertion above) and `ns_window` is live;
            // the window holds `self.view`.
            let content_view = self.view_ptr();
            let _: () = msg_send![content_view, setNeedsDisplay: YES];
            // AppKit owns the display pass that turns this into a
            // call of `updateLayer` — nothing in this backend can observe it, so the
            // two ends of the pump are instrumented separately: this line
            // (the request went out) and the content view's `update_layer` (AppKit
            // asked for content). A request with no matching `updateLayer` is
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

    /// Ask AppKit for a frame of this window now, on its owner thread: inline,
    /// or a lane turn later from inside a display pass
    /// ([`dispatch_redraw_request`]). `handle` is this window's weak handle,
    /// which the deferred turn finds it through.
    fn redraw_on_owner(&self, handle: &Arc<OwnerWeak<Self>>) {
        dispatch_redraw_request(
            // The deferral runs a lane turn later, after this call has
            // returned, so it carries the window's weak handle rather than a
            // borrow of it.
            || defer_redraw(self.owner, Arc::clone(handle), 0),
            // SAFETY: a window is used on its owner thread only (it is neither
            // `Send` nor `Sync`), which is the owner lane's.
            || unsafe { self.set_needs_display(false) },
        );
    }
}

/// Defer a redraw request to the next turn of the owner lane, delivering it
/// there if that turn lands outside AppKit's display pass.
///
/// The caller is inside AppKit's display pass of this window's view, where
/// `setNeedsDisplay:` is discarded; this hands the same call to the lane so
/// it lands after the pass has unwound instead of during it. This is
/// ADR-0039 §4(c) realised on AppKit: the frame transaction is a region the
/// drain gate is closed for, and a wake arriving while it is closed defers
/// rather than draining. Hence asynchronous, and never blocking: a display
/// pass runs on the owner thread, so a synchronous hop would run the body
/// inline, during the pass — the exact call AppKit discards.
///
/// The turn reaches the window through its weak handle, which upgrades on the
/// owner thread alone and keeps no window alive: a window dropped before the
/// turn runs is not found, and one that closed meanwhile is found but not
/// messaged — the request has nothing left to redraw.
///
/// `hop` counts the turns this request has already taken; see the re-check
/// below for why it exists and [`MAX_DEFER_HOPS`] for why it is bounded.
fn defer_redraw(owner: &'static dispatch::Queue, window: Arc<OwnerWeak<MacOSWindow>>, hop: u8) {
    exec_async_on_owner(owner, move || {
        // This turn is supposed to run after the pass has unwound, and that is
        // what a lane turn normally is: the run loop services the lane at its
        // top level, after AppKit's display pass has returned. The premise is
        // only as strong as the drain, though. A display pass that opens a
        // nested run loop — a modal panel, a drag session, a menu track —
        // services this same lane from *inside* itself, so a turn can land in
        // a pass after all, and messaging AppKit from there would discard the
        // request again, silently: a discarded deferral leaves the same trace
        // as a working one. So the turn re-checks and re-defers rather than
        // trusting the timing.
        if display_pass::in_display_pass() {
            if hop < MAX_DEFER_HOPS {
                tracing::trace!(
                    hop,
                    "request_redraw: the deferred turn landed inside another display \
                     pass (a nested run loop is draining the owner lane); deferring again"
                );
                defer_redraw(owner, window, hop + 1);
            } else {
                // Dropped rather than sent: a `setNeedsDisplay:` issued here is
                // the call AppKit throws away, so sending it would only produce
                // a misleading "sent" trace. The pump stays stalled until the
                // nested loop returns and something requests a redraw again,
                // which is the residual this backend has anyway — the warning
                // names the situation instead of leaving it to look like a
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
        match window.upgrade() {
            Some(window) if !window.closed.load(Ordering::SeqCst) => {
                // SAFETY: the handle upgraded, so this turn runs on the
                // window's owner thread (the only one it upgrades on), under
                // the lane guard `exec_async_on_owner` installed.
                unsafe { window.set_needs_display(true) };
            }
            _ => tracing::trace!(
                "request_redraw: window closed before the deferred request ran; request dropped"
            ),
        }
    });
}

/// A macOS window's redraw requester (ADR-0254 §5): what asks the window for a
/// frame from any thread.
///
/// It holds the window's weak handle, which upgrades on the owner thread
/// alone, so it keeps no window alive and reaches nothing once the window is
/// gone. A request first arms the platform's wake pump, from whichever thread
/// asks: a frame arms its fallback deadline while it runs, and the pump is what
/// looks back at that deadline once the frame has gone (see
/// `owner::apple::macos::wake_pump`). Then, on the owner thread, it asks AppKit
/// for the frame at once; from any other thread it queues that on the owner
/// lane and returns without waiting, since AppKit's views are the main
/// thread's alone.
struct MacOSRedraw {
    /// The window, on its owner thread while it lives.
    window: Arc<OwnerWeak<MacOSWindow>>,
    /// The window's owner lane.
    owner: &'static dispatch::Queue,
    /// Whether `owner` is the main queue.
    owner_is_main: bool,
    /// The platform's wake pump. `Weak`, and a `OnceLock`: the pump is
    /// platform-owned and outlives every window, so a requester must not keep
    /// it alive — and the slot is filled once, by the window position's
    /// `open` right after construction. A window constructed without a
    /// platform leaves it empty and never arms one.
    wake_pump: OnceLock<WeakWakePump>,
}

impl RedrawRequester for MacOSRedraw {
    fn request_redraw(&self) {
        // A frame is coming, so the wake pump should look at the deadline that
        // frame is about to arm. Before the frame is asked for, not after: a
        // deferred request is a lane turn away, and the pump needs to be
        // scheduled no later than the frame itself.
        if let Some(pump) = self.wake_pump.get().and_then(std::sync::Weak::upgrade) {
            pump.arm();
        }
        if on_owner_thread(self.owner, self.owner_is_main) {
            if let Some(window) = self.window.upgrade() {
                window.redraw_on_owner(&self.window);
            }
            return;
        }
        let window = Arc::clone(&self.window);
        exec_async_on_owner(self.owner, move || {
            if let Some(owned) = window.upgrade() {
                owned.redraw_on_owner(&window);
            }
        });
    }
}

/// A macOS window's surface target (ADR-0254 §5): the window's content view,
/// as an `AppKitWindowHandle`, on the main thread, until the window closes.
///
/// It keeps the view alive for as long as it lives
/// ([`MainRetainedView`](super::MainRetainedView)), so a Metal surface made
/// from the handle never outlives the view, however long the renderer holding
/// the target outlives the window. It answers `Unavailable` off the main
/// thread, and once `closed` is set — by `windowWillClose:`, or by the
/// window's [`Drop`] for a window dropped without closing — or the window's
/// delivery is retired.
struct MacOSSurface {
    /// The window's `FLUIContentView`, the `ns_view` of its handle.
    view: super::MainRetainedView,
    /// The window's closed flag.
    closed: Arc<AtomicBool>,
    /// Whether the window's native retirement has started.
    retirement: Retirement,
}

impl MacOSSurface {
    /// Whether the window is gone, so no handle is handed out.
    fn is_gone(&self) -> bool {
        self.closed.load(Ordering::SeqCst) || self.retirement.is_retired()
    }
}

impl HasWindowHandle for MacOSSurface {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let main = MainThreadMarker::new().ok_or(raw_window_handle::HandleError::Unavailable)?;
        if self.is_gone() {
            return Err(raw_window_handle::HandleError::Unavailable);
        }
        // raw-window-handle 0.6 AppKitWindowHandle expects the NSView, not
        // the NSWindow: the FLUI view, which stays the surface view whatever
        // the window's content view is.
        let ns_view = self
            .view
            .view(main)
            .ok_or(raw_window_handle::HandleError::Unavailable)?;
        let handle = AppKitWindowHandle::new(ns_view);
        // SAFETY: the target holds a +1 reference to the view
        // (`MainRetainedView`), released no sooner than the target drops, so
        // the view outlives every handle borrowed from the target and every
        // surface made while the target lives.
        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(RawWindowHandle::AppKit(handle)) })
    }
}

impl HasDisplayHandle for MacOSSurface {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        if MainThreadMarker::new().is_none() || self.is_gone() {
            return Err(raw_window_handle::HandleError::Unavailable);
        }
        let handle = AppKitDisplayHandle::new();
        // SAFETY: AppKit display handles carry no pointer.
        Ok(unsafe {
            raw_window_handle::DisplayHandle::borrow_raw(RawDisplayHandle::AppKit(handle))
        })
    }
}

impl SurfaceTarget for MacOSSurface {
    /// AppKit has no frame callback to arm before a present: the display pass
    /// paces this window's frames. It returns at once.
    fn pre_present_notify(&self, _: flui_platform::SwapChainStatistics) {}
}

/// How many owner-lane turns a deferred redraw request may take before it is
/// dropped ([`defer_redraw`]).
///
/// One turn is the point of the deferral, so a value above 1 only exists for
/// the nested-run-loop case: each retry re-queues onto a lane that is being
/// drained from inside a display pass, and a lane drained that way hands every
/// retry straight back into a pass. The budget therefore has to be small — it
/// trades a bounded number of discarded turns for the log line that says the
/// nest exists, where an unbounded retry would spin at the lane's full rate for
/// as long as the nested loop runs.
const MAX_DEFER_HOPS: u8 = 4;

impl crate::host_window::CoreHostWindow for MacOSWindow {
    fn window_delivery(&self) -> &WindowDelivery {
        &self.delivery
    }
}

impl crate::host_window::HostWindow for MacOSWindow {
    /// The window's NSAccessibility bridge — the capability the composition
    /// root's accessibility wire discovers.
    fn accessibility(&self) -> Option<Arc<dyn flui_semantics::platform::PlatformAccessibility>> {
        Some(Arc::clone(&self.facets.accessibility) as _)
    }

    fn capture_text_sizing(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
        self.facets.text.capture_sizing()
    }

    /// None: AppKit's input method is pushed to through the content view
    /// (`text_input`), and reads no field's store.
    fn text_store_host(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Option<std::rc::Rc<dyn flui_platform::text_store::TextStoreHost>> {
        self.facets
            .text
            .window_text_input()
            .and_then(crate::text::WindowTextInput::store_host)
    }
}

/// What the macOS windows open with: the table's window-level positions. The
/// loop keeps the open windows themselves (`WindowRegistry`), because its wake
/// pump and its stop reach them.
#[derive(Debug)]
pub struct MacOSWindows {
    positions: WindowPositions<ContentViewRef>,
}

impl MacOSWindows {
    /// The windows of a platform whose table's window-level positions are
    /// `positions`.
    pub(crate) fn new(positions: WindowPositions<ContentViewRef>) -> Self {
        Self { positions }
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
    type Ref = ContentViewRef;

    fn open(
        owner: &MacOSLoop,
        windows: &Arc<MacOSWindows>,
        mut options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<Rc<dyn HostWindow>, OpenWindowError> {
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
        let window = MacOSWindow::new(
            options,
            handler,
            Arc::clone(&owner.windows),
            windows,
            owner.view_hooks,
        )?;
        owner.notices.mark_stale_on_wake(window.state_cell());
        // What another thread raises for the window waits for an owner turn.
        window
            .delivery
            .resume_through_signal(&owner.loop_control.owner_signal);
        // Installed immediately after construction, before the window can be
        // handed to anyone who might redraw it — the slot is a `OnceLock`, so
        // a later install would be silently ignored rather than racing.
        window.install_loop_control(Arc::downgrade(&owner.loop_control));
        window.install_wake_pump(Arc::downgrade(&owner.wake_pump));
        window.bind_displays(Arc::clone(&owner.displays));

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
        // Every window the platform opens routes through the AppKit main lane
        // (`MacOSWindow::new`).
        appkit_window_capabilities(true)
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
        Some(Arc::clone(&self.facets.haptics))
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        Some(Arc::clone(&self.facets.chrome))
    }

    /// AppKit starts no OS drag session from this window yet; its input
    /// capabilities say so.
    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        None
    }

    /// The window's IME capability — the `NSTextInputClient` conformance the
    /// content view carries (see `text::apple::macos`). It shares the
    /// window's `closed` flag, so it observes the window's own close.
    fn text_input(&self) -> Option<Arc<dyn flui_platform::PlatformTextInput>> {
        self.facets
            .text
            .window_text_input()
            .and_then(crate::text::WindowTextInput::ime)
    }

    /// What the window's `MacOSRedraw` does, on the owner thread: arm the
    /// wake pump, then `setNeedsDisplay:` on the window's view, a lane turn
    /// later from inside a display pass.
    fn request_redraw(&self) {
        self.redraw().request_redraw();
    }

    fn redraw_requester(&self) -> Arc<dyn RedrawRequester> {
        Arc::clone(self.redraw()) as _
    }

    fn surface_target(&self) -> Arc<dyn SurfaceTarget> {
        Arc::new(MacOSSurface {
            // SAFETY: `self.view` is the window's content view, which the
            // window holds while it lives.
            view: unsafe { super::MainRetainedView::retain(self.view_ptr()) },
            closed: Arc::clone(&self.closed),
            retirement: self.delivery.retirement(),
        })
    }

    // ==================== State and commands (ADR-0237) ====================

    fn state(&self) -> WindowState {
        self.current_state()
    }

    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(ApplyError::Closed);
        }
        let command = command.against(&self.snapshot.published_capabilities())?;
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
            WindowCommand::Show => self.show_window()?,
            WindowCommand::Hide => self.hide()?,
            WindowCommand::Activate => self.activate_window(),
            // AppKit tears the window down through `windowWillClose:`.
            WindowCommand::Close => {
                self.close_window();
                return Ok(());
            }
            // The material is no fact of the snapshot: nothing to publish.
            WindowCommand::SetMaterial(material) => return self.apply_material(material),
            // A command added to the contract that this backend has not met
            // is refused, not ignored.
            other => return Err(crate::window::state::not_carried_out(&other)),
        }
        self.publish_state();
        Ok(())
    }

    fn reveal_after_first_frame(&self) {
        // The flag is settled before the message, so a second call sees
        // `false`: the window becomes opaque exactly once for its deferred
        // reveal.
        if !self.take_pending_first_reveal() || self.closed.load(Ordering::SeqCst) {
            return;
        }
        // SAFETY: `ns_window` is alive for the lifetime of `self`, and a
        // window is used on its owner thread only.
        unsafe {
            let _: () = msg_send![self.ns_window, setAlphaValue: 1.0f64];
        }
        tracing::debug!("macOS: window revealed after its first presented frame");
    }

    /// The capability is `Unavailable`, for the reason `frame_rate_absence`
    /// gives at the version the platform's displays read: below macOS 14 there
    /// is no per-view display link to carry the request, from 14 on this host
    /// paces its frames without it. A window not yet bound to the displays
    /// knows no version, which unlocks nothing.
    fn set_frame_rate_hint(
        &self,
        _hint: flui_platform::FrameRateHint,
    ) -> Result<(), flui_platform::Unsupported> {
        let absence = self.displays.get().map_or(
            flui_platform::CapabilityAbsence::NotOnThisDevice,
            |displays| frame_rate_absence(displays.version()),
        );
        Err(absence.into())
    }

    /// The input regions keep the system's edge gestures out of a region
    /// (`InputRegionKind::SystemGestureExclusion`), and AppKit lets no window
    /// keep the system's trackpad gestures out; `ignoresMouseEvents` and a
    /// hit-test override decide where a window takes input, which is not that.
    fn set_input_regions(
        &self,
        _regions: &[flui_platform::input_regions::InputRegion],
    ) -> Result<(), flui_platform::Unsupported> {
        Err(flui_platform::Unsupported::new(
            flui_platform::UnsupportedReason::NotOnThisPlatform,
        ))
    }

    // The explicit `arrowCursor` list below is intentional documentation of
    // which icons resolve to the default arrow; it is textually identical to
    // the `_` arm's body, which objc2's `msg_send!` expansion makes clippy
    // visible (objc 0.2's differing expansion hid it).
    #[allow(clippy::match_same_arms)]
    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        self.state.lock().cursor = cursor;

        // SAFETY: every selector below is an NSCursor class constructor
        // available on the supported macOS baseline. The returned singleton
        // remains owned by AppKit and `set` only selects it for the current
        // pointer location. A window is used on its owner thread only, the
        // AppKit main thread.
        unsafe {
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
        }
    }
}

/// The bodies of the show, activate and close commands.
impl MacOSWindow {
    /// Show the window, deminiaturizing it and making it key; the zoomed or
    /// fullscreen mode is kept.
    fn show_window(&self) -> Result<(), ApplyError> {
        // An explicit show settles a deferred first reveal: the window is
        // made opaque along with being ordered.
        let settle_reveal = self.take_pending_first_reveal();
        self.order_front(settle_reveal)?;
        self.enter_mode_when_shown();
        Ok(())
    }

    /// Deminiaturize the window and order it front as the key window, made
    /// opaque first when `settle_reveal`. A window that closes meanwhile (a
    /// deminiaturize reaches the delegate) answers `Closed`.
    fn order_front(&self, settle_reveal: bool) -> Result<(), ApplyError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(ApplyError::Closed);
        }
        // SAFETY: self retains the native window, and a window is used on its
        // owner thread only. Deminiaturizing and ordering do not toggle
        // zoom/fullscreen.
        unsafe {
            if settle_reveal {
                let _: () = msg_send![self.ns_window, setAlphaValue: 1.0f64];
            }
            let minimized: bool = msg_send![self.ns_window, isMiniaturized];
            if minimized {
                let _: () = msg_send![self.ns_window, deminiaturize: NIL];
            }
            if self.closed.load(Ordering::SeqCst) {
                return Err(ApplyError::Closed);
            }
            let _: () = msg_send![self.ns_window, makeKeyAndOrderFront: NIL];
        }
        if self.closed.load(Ordering::SeqCst) {
            Err(ApplyError::Closed)
        } else {
            Ok(())
        }
    }

    fn activate_window(&self) {
        let settle_reveal = self.take_pending_first_reveal();
        // SAFETY: `ns_window` is alive for the lifetime of `self`, and a
        // window is used on its owner thread only.
        unsafe {
            if settle_reveal {
                let _: () = msg_send![self.ns_window, setAlphaValue: 1.0f64];
            }
            let _: () = msg_send![self.ns_window, makeKeyAndOrderFront: NIL];
        }
        self.enter_mode_when_shown();
    }

    fn close_window(&self) {
        // No separate flag-set needed here: AppKit's `-[NSWindow close]`
        // posts `NSWindowWillCloseNotification` (routing to the delegate's
        // `windowWillClose:`, and so to `handle_close` below) whether or
        // not `windowShouldClose:` was ever asked — the same notification a
        // user-initiated close takes. `handle_close` is therefore reached
        // by every route through this window's own `close`, and is where
        // `closed` is actually set.
        //
        // SAFETY: `ns_window` is alive for the lifetime of `self`, and a
        // window is used on its owner thread only.
        unsafe {
            let _: () = msg_send![self.ns_window, close];
        }
    }
}

impl Drop for MacOSWindow {
    fn drop(&mut self) {
        // A window is dropped where it lives: it is neither `Send` nor
        // `Sync`, and is made on the AppKit main thread.
        debug_assert!(
            on_owner_thread(self.owner, self.owner_is_main),
            "BUG: a macOS window is dropped on its owner thread"
        );
        tracing::debug!("Closing NSWindow {:p}", self.ns_window);

        // The window's surface targets and the features beside it outlive
        // it; from here on they hand out nothing of a view the release below
        // may free. A window that closed set this already.
        self.closed.store(true, Ordering::SeqCst);

        // The close delegate pins its native receiver in the outer autorelease
        // pool, so a final callback-local wrapper drop cannot deallocate the
        // receiver under AppKit's close stack.
        //
        // `detach()` runs before the release, preserving accesskit's
        // documented precondition (unhook the dynamic subclass while the
        // content view is still alive). A bridge failure must not skip the
        // native window's balancing release, and a view a bridge failed to
        // come off is kept rather than released (`take_apart`).
        let a11y = Arc::clone(&self.facets.accessibility);
        let text = Arc::clone(&self.facets.text);
        let ns_window = self.ns_window;
        // SAFETY: `Drop` runs once, and nothing reads `self.view` after this.
        let view = unsafe { ManuallyDrop::take(&mut self.view) };
        super::teardown::take_apart(
            view,
            &mut [&mut || a11y.detach(), &mut || text.detach()],
            &mut || {
                // SAFETY: the wrapper owns the `alloc`/`init` +1 of
                // `ns_window` (`releasedWhenClosed` is off), and this is the
                // one balancing release, sent on the owner thread.
                unsafe {
                    let _: () = msg_send![ns_window, release];
                }
            },
            |step| crate::owner::apple::macos::run_cleanup_guarded(step),
        );
    }
}

impl MacOSWindow {
    /// The window's published state, read from AppKit again first when
    /// something invalidated it; `WindowHandler::state_changed` is raised when that
    /// produced a new revision.
    fn current_state(&self) -> WindowState {
        let published = self.snapshot.state_with(|| self.sample_state());
        if published.changed {
            self.delivery.dispatch_state_changed();
        }
        published.state
    }

    /// The window may differ from its published state: read it again.
    fn publish_state(&self) {
        self.snapshot.invalidate();
        let _ = self.current_state();
    }

    /// Read the window again for its state publisher, announcing nothing.
    fn read_state(&self) -> crate::owner::window_delivery::Publication {
        self.snapshot.invalidate();
        self.snapshot.state_with(|| self.sample_state()).outcome()
    }

    /// The window as AppKit reports it now.
    fn sample_state(&self) -> WindowState {
        let (display, normal, scale, occlusion_visible, heading) = {
            let state = self.state.lock();
            (
                state.display,
                state.normal,
                state.scale_factor,
                state.occlusion_visible,
                state.sequence.heading(std::time::Instant::now()),
            )
        };
        let hovered = self.delivery.hovered();
        // SAFETY: `ns_window` is alive for the lifetime of `self`, and a
        // window is used on its owner thread only. `title` returns an
        // autoreleased NSString whose UTF8String buffer is copied before
        // returning.
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
            let mode = mode_of(flags.mode_flags());
            let position = primary_screen_height().and_then(|height| {
                top_left_of_frame(crate::owner::apple::macos::bounds_of(frame), height)
            });
            let state = WindowState::new(
                WindowGeometry::new(position, Size::new(frame.size.width, frame.size.height)),
                Size::new(content.size.width, content.size.height),
            )
            .with_mode(mode)
            .with_normal_geometry(if mode == WindowMode::Normal {
                None
            } else {
                normal
            })
            .with_visible(is_shown(on_screen, flags.miniaturized) && occlusion_visible)
            .with_focused(focused)
            .with_title(Some(title))
            .with_resizable(flags.resizable)
            .with_decorated(mask.contains(NSWindowStyleMask::Titled))
            .with_constraints(constraints_of_content_limits(
                Size::new(min.width, min.height),
                Size::new(max.width, max.height),
            ))
            .with_display(display);
            ReportedFacts {
                metrics: flui_foundation::geometry::WindowMetrics::from_logical(
                    Size::new(content.size.width, content.size.height),
                    scale,
                )
                .ok(),
                // The window's appearance, not NSApp's: a window may
                // override it.
                appearance: content_view::effective_appearance(
                    (&*self.ns_window)
                        .downcast_ref::<objc2_app_kit::NSWindow>()
                        .expect("BUG: MacOSWindow wraps an NSWindow"),
                ),
                // A desktop window has no safe area and AppKit never
                // suspends one.
                safe_area: flui_foundation::geometry::EdgeInsets::ZERO,
                execution: flui_platform::WindowExecutionState::Running,
                hovered,
                // The key window is the one this host reports active;
                // the main window has no report of its own here.
                active: focused,
                capabilities: capabilities_of(
                    flags,
                    heading,
                    appkit_window_capabilities(self.owner_is_main),
                ),
            }
            .onto(state)
        }
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

    // Every body below messages `ns_window`, which is alive for the lifetime
    // of `self`, on the window's owner thread: a window is used there only, as
    // it is neither `Send` nor `Sync`.

    /// The window's outer rectangle now.
    fn outer_geometry(&self) -> WindowGeometry {
        // SAFETY: see the note above these bodies.
        unsafe {
            let frame: NSRect = msg_send![self.ns_window, frame];
            WindowGeometry::new(
                primary_screen_height().and_then(|height| {
                    top_left_of_frame(crate::owner::apple::macos::bounds_of(frame), height)
                }),
                Size::new(frame.size.width, frame.size.height),
            )
        }
    }

    /// Remember the outer rectangle if the window is in normal mode: the
    /// window is about to leave it, and AppKit keeps no record of where it
    /// returns to.
    fn record_normal(&self) {
        self.record_normal_if(|flags| mode_of(flags.mode_flags()) == WindowMode::Normal);
    }

    /// [`record_normal`](Self::record_normal) for `windowWillEnterFullScreen:`,
    /// which may already see the full-screen style bit while the frame is still
    /// the normal one: a window counts as normal if it is neither miniaturized
    /// nor maximized.
    fn record_normal_entering_fullscreen(&self) {
        self.record_normal_if(|flags| !(flags.miniaturized || flags.is_maximized()));
    }

    fn record_normal_if(&self, is_normal: impl FnOnce(WindowFlags) -> bool) {
        // SAFETY: see the note above these bodies.
        if is_normal(unsafe { self.read_flags() }) {
            let outer = self.outer_geometry();
            self.state.lock().normal = Some(outer);
        }
    }

    fn set_native_title(&self, title: &str) {
        // SAFETY: see the note above these bodies; the NSString is created
        // from a valid Rust string and the window copies it.
        unsafe {
            let ns_title = NSString::from_str(title);
            let _: () = msg_send![self.ns_window, setTitle: &*ns_title];
        }
    }

    /// Move the window's outer top-left corner.
    fn set_native_position(&self, at: Point<f64>) -> Result<(), ApplyError> {
        // SAFETY: see the note above these bodies.
        unsafe {
            let height = primary_screen_height().ok_or_else(|| {
                ApplyError::failed("there is no screen to measure a position from".into())
            })?;
            let point = cocoa_top_left_point(at, height);
            let _: () = msg_send![
                self.ns_window,
                setFrameTopLeftPoint: NSPoint::new(point.x, point.y)
            ];
        }
        Ok(())
    }

    /// Resize the content area, not the frame: on decorated windows a
    /// frame-sized `setFrame:` would shrink the content by the title bar.
    fn set_content_size(&self, size: Size<f64>) {
        // SAFETY: see the note above these bodies.
        unsafe {
            let ns_size = NSSize::new(size.width, size.height);
            let _: () = msg_send![self.ns_window, setContentSize: ns_size];
        }
    }

    fn set_native_constraints(&self, constraints: SizeConstraints) {
        let (min, max) = content_limits_of_constraints(constraints);
        // SAFETY: see the note above these bodies.
        unsafe {
            let _: () =
                msg_send![self.ns_window, setContentMinSize: NSSize::new(min.width, min.height)];
            let _: () =
                msg_send![self.ns_window, setContentMaxSize: NSSize::new(max.width, max.height)];
        }
    }

    /// Switch one style-mask bit (resizable, titled) on or off, keeping the
    /// content area where it is: removing the title bar shrinks the frame, not
    /// the content.
    fn set_style_bit(&self, bit: NSWindowStyleMask, on: bool) -> Result<(), ApplyError> {
        // SAFETY: see the note above these bodies.
        unsafe {
            let mask: NSWindowStyleMask = msg_send![self.ns_window, styleMask];
            if mask.contains(NSWindowStyleMask::FullScreen) {
                return Err(ApplyError::failed(
                    "AppKit does not restyle a window while it is full screen".into(),
                ));
            }
            // A window that is no longer resizable reports a zoomed frame as
            // normal and cannot be un-zoomed by the user, so it is restyled
            // only from the mode it can be asked to return to.
            let flags = self.read_flags();
            if bit == NSWindowStyleMask::Resizable {
                let decision = {
                    let mut state = self.state.lock();
                    let sequence = &mut state.sequence;
                    if on {
                        sequence.make_resizable();
                        FixedSize::Apply
                    } else {
                        sequence.fix_size(std::time::Instant::now(), flags)
                    }
                };
                match decision {
                    FixedSize::Apply => {}
                    // Carried out once the step ends (`handle_transition_ended`).
                    FixedSize::Held => return Ok(()),
                    FixedSize::Refused(why) => return Err(ApplyError::failed(why.into())),
                }
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
        }
        Ok(())
    }

    /// Order the window out. A full-screen window is refused: ordering it out
    /// leaves its empty Space behind.
    fn hide(&self) -> Result<(), ApplyError> {
        // SAFETY: see the note above these bodies.
        unsafe {
            if self.read_flags().fullscreen {
                return Err(ApplyError::failed(
                    "a full-screen window leaves full screen before it is hidden".into(),
                ));
            }
            let _: () = msg_send![self.ns_window, orderOut: NIL];
        }
        Ok(())
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
    /// What a commanded step says of where the window is heading lasts at
    /// most `ANIMATION_LIMIT`: a step whose end never arrives gives up then,
    /// with the commands held for it, and the window is read again, so a
    /// capability that depended on it moves with the published revision and
    /// a command admitted under the old one is superseded, not failed.
    fn read_again_when_the_step_gives_up(&self) {
        let window = Arc::clone(&self.redraw().window);
        crate::owner::apple::macos::exec_after_on_owner(self.owner, ANIMATION_LIMIT, move || {
            let Some(window) = window.upgrade() else {
                return;
            };
            let gave_up = window
                .state
                .lock()
                .sequence
                .expire(std::time::Instant::now());
            if gave_up {
                window.publish_state();
            }
        });
    }

    fn change_mode(&self, mode: WindowMode) -> Result<(), ApplyError> {
        // SAFETY: see the note above these bodies.
        let flags = unsafe { self.read_flags() };
        if !flags.resizable && matches!(mode, WindowMode::Maximized | WindowMode::Fullscreen) {
            return Err(ApplyError::failed(
                "AppKit does not zoom or full-screen a window that cannot be resized".into(),
            ));
        }
        let now = std::time::Instant::now();
        let (steps, heading) = {
            let mut state = self.state.lock();
            let steps = state.sequence.command(now, flags, mode);
            (steps, state.sequence.heading(now))
        };
        if heading.is_some() {
            self.read_again_when_the_step_gives_up();
        }
        if !steps.is_empty() {
            self.record_normal();
        }
        for step in &steps {
            // SAFETY: see the note above these bodies. Each message is the
            // AppKit request the step names.
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
