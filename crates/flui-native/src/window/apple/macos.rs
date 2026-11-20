//! The macOS window: [`MacOSWindow`] over an `NSWindow` (its state as AppKit
//! reports it, and the requests its commands send; whether the thread is
//! inside a view's display pass, the owner's `display_pass`, decides how a
//! redraw is asked for), and [`MacOSWindows`], what the windows open with.
//!
//! Four parts live beside it: [`delegate`], the window's `NSWindowDelegate`,
//! one AppKit object with the handlers of what it reports; [`ext`],
//! [`MacOSWindowExt`] and the [`AppKitWindow`] it gives, the public AppKit
//! features beyond the cross-platform window; [`backdrop`], the view
//! behind the content ([`MacOSBackdrop`]: a material or Liquid Glass); and
//! [`cursor`], the `NSCursor` the window shows and the pointer it locks.
//!
//! The loop holds the windows it tracks as `HostWindow`s
//! (`owner::apple::macos`). A window lives on the AppKit main thread, its
//! owner; the thread that presents its frames holds its `MacOSSurface`, and
//! any thread asks it for a frame through its `MacOSRedraw` (ADR-0254 §5).

// The window reaches AppKit through objc2's typed API; what remains `unsafe` is
// the few sends that API marks so (an initializer, `setReleasedWhenClosed:`)
// and the raw pointers handed to other features: one of the crate's `unsafe`
// FFI islands, opted out at its module boundary.
#![expect(
    unsafe_code,
    reason = "the few objc2 sends marked `unsafe` (an initializer, `setReleasedWhenClosed:`) and the raw pointers handed to other features have no safe form"
)]

pub(crate) mod backdrop;
pub(crate) mod cursor;
pub(crate) mod delegate;
pub(crate) mod ext;

pub use backdrop::{
    GlassStyle, GlassTint, LiquidGlass, MacOSBackdrop, MacOSBackdropError, MacOSMaterial,
};
use delegate::{create_window_delegate, display_of_screen};
pub use ext::{
    AppKitWindow, AppKitWindowError, InvalidWindowAlpha, MacOSCollectionBehavior,
    MacOSMissionControl, MacOSSpaces, MacOSWindowAlpha, MacOSWindowCycle, MacOSWindowExt,
    MacOSWindowLevel, TabJoinError,
};

use std::cell::{Cell, OnceCell, RefCell};
use std::mem::ManuallyDrop;
use std::rc::Rc;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, msg_send};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSColor, NSEvent, NSScreen, NSUserInterfaceLayoutDirection,
    NSView, NSWindow, NSWindowButton, NSWindowCollectionBehavior, NSWindowStyleMask,
    NSWindowTitleVisibility,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use flui_foundation::geometry::{Bounds, Point, Size};
use flui_platform::{
    AdmittedCommand, ApplyError, SizeConstraints, WindowCapabilities, WindowCommand, WindowFrame,
    WindowGeometry, WindowMode, WindowPosition, WindowState,
};

use crate::window::apple::geometry::{
    frame::{
        ScreenSafeArea, cocoa_frame, cocoa_frame_keeping_top, cocoa_top_left_point,
        constraints_of_content_limits, content_limits_of_constraints, desktop_point, position_on,
        safe_area_on_screen, top_left_of_frame,
    },
    mode::{
        ANIMATION_LIMIT, FixedSize, ModeSequence, ModeStep, WaitEndRead, WindowFlags,
        capabilities_of,
    },
    title_bar::{ControlsLayout, FrameStyle},
};
use crate::window::capabilities::{
    appkit_presentation, appkit_window_capabilities, frame_rate_absence,
};
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle,
};

use crate::host::{self, WindowFacets, WindowPositions};
use crate::host_window::HostWindow;
use crate::owner::apple::MacOSLoop;
use crate::owner::apple::macos::content_view::{self, ViewHooks};
use crate::owner::apple::macos::display_pass;
use crate::owner::apple::macos::{
    ContentViewRef, HostDisplays, WeakWakePump, WindowRegistry, exec_async_on_owner,
    on_owner_thread,
};
use crate::owner::apple::metal_layer::{LayerRefresh, NewRefresh};
use crate::owner::apple::pump_delivery::FollowedDelivery;
use crate::owner::apple::timed_frame::{ArmedTick, TimedFrame, TimedTick};

use crate::{
    owner::OpenWindowError,
    owner::slot::{OwnerCell, OwnerWeak},
    owner::window_delivery::{Retirement, WindowDelivery},
    window::state::{
        MODE_ANSWER_LIMIT, ReportedFacts, Sample, SampledGeometry, WindowStateCell, mode_of,
        reported_frame,
    },
};
use flui_platform::{
    AspectRatio, Cursor, CursorError, CursorGrab, DisplayId, PlatformWindow, RedrawRequester,
    SurfaceTarget, WindowExtent, WindowHandler, WindowId, WindowIdMint, WindowLevel, WindowOptions,
    WindowReveal,
};

/// macOS window wrapper around NSWindow.
///
/// Neither `Send` nor `Sync` (ADR-0254 §5): it is made on the AppKit main
/// thread, its owner, and every AppKit message it sends is sent there, because
/// no other thread can reach it. What other threads need of it is handed out
/// apart from it: its `MacOSSurface` and its `MacOSRedraw`.
pub struct MacOSWindow {
    /// The native window. The wrapper owns the `alloc`/`init` +1 (the window
    /// is made with `releasedWhenClosed` off, so `-[NSWindow close]` never
    /// balances it): the window's [`Drop`] lets it go after detaching what is
    /// hooked to the content view, and before the view.
    ns_window: ManuallyDrop<Retained<NSWindow>>,

    /// The main thread this window was made on, which every message it sends
    /// is sent from: the window is neither `Send` nor `Sync`.
    main: MainThreadMarker,

    /// The id the platform issued for this window, which no other window of
    /// the platform has, whatever address AppKit gives the next `NSWindow`.
    id: WindowId,

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
    view: ManuallyDrop<Retained<content_view::FluiContentView>>,

    /// What the window keeps of itself between AppKit's notifications. Only
    /// the main thread reaches it, through the window (neither `Send` nor
    /// `Sync`) or its delegate (a main-thread class). No borrow is held across
    /// an AppKit message or a callback into the application, so a delegate
    /// method AppKit sends from inside one finds it free.
    state: RefCell<MacOSWindowState>,

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
    ///
    /// Atomic because its holders cross threads: the surface target and the
    /// text input are `Send + Sync` by their contracts (`SurfaceTarget`,
    /// `TextInput`) and may be dropped or asked anywhere, though each reads
    /// the flag only on the main thread.
    closed: Arc<AtomicBool>,
    loop_control: OnceCell<std::sync::Weak<crate::owner::apple::macos::LoopControl>>,

    /// The window's `NSWindowDelegate`, which AppKit holds only weakly: the
    /// window keeps it from right after it is made until its drop detaches
    /// it from the `NSWindow` and hands it to the autorelease pool, so a
    /// delegate method on the stack outlives the drop it caused.
    delegate: OnceCell<Retained<delegate::WindowDelegate>>,

    /// Why the machine's display tells this window nothing of its frames
    /// reaching the screen; the platform's, copied at open.
    presentation: Option<flui_platform::CapabilityAbsence>,

    /// What the table's positions made for this window once its content view
    /// was installed: the accessibility bridge, subclassed onto that view;
    /// the text position, the `NSTextInputClient` conformance the view
    /// carries; the actuator; and the system chrome.
    facets: WindowFacets,

    /// What asks this window for a frame from any thread, and arms the
    /// platform's wake pump as it does. Set once, right after the window is
    /// made, since it reaches the window through a weak reference to it.
    redraw: OnceCell<Arc<MacOSRedraw>>,

    /// The instant this window was asked for a frame at and has not been
    /// served (`PlatformWindow::request_frame_at`, ADR-0389): what the owner
    /// queue's tick for it compares against.
    timed_frame: RefCell<TimedFrame>,

    /// The platform's display registry, by which this window turns its
    /// screen into a [`DisplayId`], bound once by [`UnshownWindow::show`]
    /// before the window is shown, like the wake pump; a window built
    /// without a platform names no display.
    displays: OnceCell<Arc<dyn HostDisplays>>,

    /// The published window state (ADR-0237). Seeded from the creation
    /// options; the first read replaces it with what AppKit reports.
    snapshot: Arc<WindowStateCell>,

    /// The read the window owes when the snapshot's wait for a mode change
    /// ends (`read_when_the_mode_wait_ends`).
    wait_end_read: Cell<WaitEndRead>,
}

// Owner-local (ADR-0254 §5): the window's state is a `RefCell` because no
// other thread can hold the window.
static_assertions::assert_not_impl_any!(MacOSWindow: Send, Sync);

/// Follows the snapshot's wait for a mode change to its end as a read of the
/// window ends (`MacOSWindow::read_when_the_mode_wait_ends`), whether the
/// read returned or unwound: a read that publishes runs the handler's
/// `state_changed`, and one that panics there still leaves the end of the
/// wait read.
struct ModeWaitFollowed<'a>(&'a MacOSWindow);

impl Drop for ModeWaitFollowed<'_> {
    fn drop(&mut self) {
        self.0.read_when_the_mode_wait_ends();
    }
}

/// What a window keeps of itself between AppKit's notifications.
struct MacOSWindowState {
    /// Current window bounds (logical pixels)
    bounds: Bounds<f64>,

    /// Scale factor (1.0 for non-Retina, 2.0 for Retina)
    scale_factor: flui_foundation::geometry::DevicePixelRatio,
    /// Whether the last backing scale read was refused, so a run of refusals
    /// is reported once.
    scale_streak: crate::display::scale::ScaleStreak,

    /// The display this window is on. Cached, not queried per call: reading it
    /// messages the window's screen, and `PlatformWindow::state` reads it on
    /// every snapshot. Refreshed wherever `scale_factor` is —
    /// `windowDidChangeScreen:` and `windowDidChangeBackingProperties:`.
    display: Option<DisplayId>,

    /// The refresh last pushed to the layer's presentation reports, by the
    /// display snapshot it was read from (`sync_layer_refresh`).
    layer_refresh: LayerRefresh,

    /// Cursor selected by this exact window's presentation, made into an
    /// `NSCursor` where it is not one of AppKit's own.
    cursor: cursor::NativeCursor,

    /// The grab the window holds: `Locked` while it detached the mouse
    /// (`lock_pointer`), let go when it stops being key, is hidden,
    /// miniaturized or closed.
    cursor_grab: CursorGrab,

    /// Where a locked pointer rests, in the content view's coordinates: the
    /// cursor stays at a screen point while the mouse is detached, so it is
    /// put back here when the window moves, resizes or changes screen
    /// (`keep_lock_point`). `None` while nothing is locked.
    lock_at: Option<NSPoint>,

    /// The AppKit level the window stands at outside full screen: what
    /// `SetLevel` or `AppKitWindow::set_window_level` last set, published as
    /// the snapshot's level. A full-screen window stands at the normal level
    /// while it lasts, and takes this one again when it leaves (`apply_level`).
    level: objc2_app_kit::NSWindowLevel,

    /// Whether the user is resizing the window by its frame: from
    /// `windowWillStartLiveResize:` to `windowDidEndLiveResize:`.
    live_resize: bool,

    /// The steps and the proportion the last commands asked the user's
    /// resize to keep (`apply_resize_rules`).
    resize_increments: Option<WindowExtent>,
    aspect_ratio: Option<AspectRatio>,

    /// The outer rectangle the window returns to from a maximized, minimized
    /// or full-screen mode. AppKit does not keep one a caller can read, so it
    /// is recorded just before the window leaves normal mode (`record_normal`)
    /// and when the window opens in another mode; `None` until then.
    normal: Option<WindowGeometry>,

    /// The order in which mode commands reach AppKit, and the mode a hidden
    /// window enters when first shown.
    sequence: ModeSequence,

    /// The visible bit of the window's `occlusionState`, as
    /// `windowDidChangeOcclusionState:` last read it: a window fully covered
    /// or on another space is occluded in its snapshot. Starts `true` — a
    /// freshly created window is unoccluded until AppKit says otherwise.
    occlusion_visible: bool,

    /// The window was opened `visible: true` with
    /// [`WindowReveal::AfterFirstFrame`] and is ordered front, but at
    /// `alphaValue` 0: the first reveal — alpha back to [`Self::alpha`] — is deferred
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

    /// The alpha the application gave the window (`AppKitWindow::set_alpha`),
    /// opaque until then: what `alphaValue` is once the window is revealed.
    /// While a first reveal is pending AppKit holds 0 and this waits for it.
    alpha: MacOSWindowAlpha,

    /// The frame last asked for, at open or by `SetFrame`: AppKit's style mask
    /// tells a system title bar from none, and only this tells a title bar of
    /// the application's apart.
    frame: WindowFrame,

    /// Where the system lays the traffic lights out, read the first time the
    /// window had them and before anything moved them; `None` until then.
    system_controls: Option<ControlsLayout>,
}

impl MacOSWindowState {
    /// Whether the window's frame moves the traffic lights from where AppKit
    /// lays them out.
    fn moves_system_controls(&self) -> bool {
        matches!(
            FrameStyle::of(self.frame).controls,
            Some(flui_platform::SystemControls::Offset(_))
        )
    }
}

impl std::fmt::Debug for MacOSWindow {
    // Hand-written: `ns_window` is shown by its address (its `Debug` would
    // message the object), and `WindowDelivery` is a callback payload; neither
    // has a useful Debug form. `owner` (the dispatch lane) has no useful Debug
    // representation either, so it is skipped along with them by
    // `finish_non_exhaustive`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacOSWindow")
            .field("ns_window", &self.native_key())
            .finish_non_exhaustive()
    }
}

/// `object`'s address, as the features beside the window hold it.
fn objc_address(object: &AnyObject) -> *mut AnyObject {
    std::ptr::from_ref(object).cast_mut()
}

/// A window's options and the numbers checked from them
/// (`WindowOptions::geometry`), which travel together: the options carry
/// what the check does not read (title, frame, mode, the position made
/// global), and the checked numbers are what the window is built at.
pub(crate) struct Opening {
    options: WindowOptions,
    geometry: flui_platform::OpeningGeometry,
    /// Where the options place the window's top-left corner, as a point of
    /// AppKit's flipped desktop: their position on its screen; `None` for no
    /// position, or one on a screen the snapshot no longer holds.
    at: Option<Point<f64>>,
    /// Where the rectangle a maximized or full-screen opening returns to
    /// starts, as the same kind of point.
    returns_to: Option<Point<f64>>,
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
    /// The window's FLUI view, as the address the features beside it hold.
    fn view_ptr(&self) -> *mut AnyObject {
        objc_address(&self.view)
    }

    /// The native window's address, which keys it in the loop's registry.
    fn native_key(&self) -> u64 {
        Retained::as_ptr(&self.ns_window).addr() as u64
    }

    /// Clear the deferred-first-reveal flag, returning whether it was set:
    /// the one call that decides which caller performs the reveal. The state
    /// borrow ends before the caller sends any AppKit message, so it is
    /// never held across one.
    fn take_pending_first_reveal(&self) -> Option<MacOSWindowAlpha> {
        let mut state = self.state.borrow_mut();
        std::mem::replace(&mut state.first_reveal_pending, false).then_some(state.alpha)
    }

    /// Keep `alpha` as the application's, returning whether AppKit's
    /// `alphaValue` takes it now: not while a first reveal is pending, which
    /// reveals the window at it.
    fn keep_alpha(&self, alpha: MacOSWindowAlpha) -> bool {
        let mut state = self.state.borrow_mut();
        state.alpha = alpha;
        !state.first_reveal_pending
    }

    /// Create a new macOS window, on the main thread `mtm` proves this is.
    ///
    /// The options' position and the position of their `normal_geometry` are
    /// read from `opening` as points of AppKit's flipped desktop, which the
    /// window position's `open` turned them into through their screens
    /// before the window is built. `delivery` holds the handler the window
    /// opens with and reports its frames to the wake pump
    /// (`WakePump::window_delivery`): a delivery that does not cannot be
    /// passed.
    ///
    /// # Errors
    /// [`OpenWindowError::Backend`] when AppKit refuses to allocate the
    /// NSWindow.
    pub(crate) fn try_new(
        mtm: MainThreadMarker,
        id: WindowId,
        opening: Opening,
        delivery: FollowedDelivery,
        windows_map: WindowRegistry,
        windows: &MacOSWindows,
        view_hooks: ViewHooks,
    ) -> Result<UnshownWindow, OpenWindowError> {
        Self::new_inner(mtm, id, opening, delivery, windows_map, windows, view_hooks)
    }

    /// Hand this window the platform's display registry, and read which display
    /// it is on.
    ///
    /// Called once, by [`UnshownWindow::show`], before the window is shown or
    /// handed to anyone: the window is built before the platform hands it
    /// what it shares with the other windows.
    fn bind_displays(&self, displays: Arc<dyn HostDisplays>) {
        // The slot is read on every screen change and must not change
        // registries: a second bind is ignored.
        if self.displays.set(displays).is_ok() {
            self.refresh_display();
            self.sync_layer_refresh();
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
        let id = display_of_screen(displays.as_ref(), self.ns_window.screen().as_deref());
        let changed = {
            let mut state = self.state.borrow_mut();
            let changed = state.display != id;
            state.display = id;
            changed
        };
        if changed {
            // The display is a field of the snapshot: the next read samples.
            self.snapshot.invalidate();
        }
        changed
    }

    /// Push the refresh of the display this window is on to its layer's
    /// presentation reports, when the display snapshot or the window's
    /// display moved since the last push: a rate change on the same display
    /// reaches it as a new snapshot revision. Owner lane on the main thread,
    /// where the screens are read.
    fn sync_layer_refresh(&self) {
        let Some(displays) = self.displays.get() else {
            return;
        };
        let Ok(snapshot) = displays.snapshot(self.main) else {
            return;
        };
        let push = {
            let mut state = self.state.borrow_mut();
            let display = state.display;
            state.layer_refresh.update(&snapshot, display)
        };
        if let Some(NewRefresh(refresh)) = push {
            content_view::set_layer_refresh(&self.view, refresh);
        }
    }

    /// Hand this window the platform's wake pump.
    ///
    /// Called once, by [`UnshownWindow::show`], before the window is shown or
    /// handed to anyone who could redraw it.
    fn install_wake_pump(&self, pump: WeakWakePump) {
        // The slot is read on every redraw, from any thread, so a window never
        // changes pumps: a second install is ignored.
        let _ = self.redraw().wake_pump.set(pump);
    }

    /// The window's redraw requester.
    fn redraw(&self) -> &Arc<MacOSRedraw> {
        self.redraw
            .get()
            .expect("BUG: a window's redraw requester is set as the window is made")
    }

    fn new_inner(
        main: MainThreadMarker,
        id: WindowId,
        opening: Opening,
        delivery: FollowedDelivery,
        windows_map: WindowRegistry,
        windows: &MacOSWindows,
        view_hooks: ViewHooks,
    ) -> Result<UnshownWindow, OpenWindowError> {
        let delivery = Arc::new(delivery.into_inner());
        let flui_platform::OpeningGeometry {
            content,
            constraints,
            ..
        } = opening.geometry;
        let (placed_at, returns_to) = (opening.at, opening.returns_to);
        let mut options = opening.options;
        // A window's owner lane is the AppKit main queue — the one lane all
        // other process AppKit traffic already serializes on (see
        // `owner::apple::macos::owner_lane`) — which runs on the main thread
        // the window is made on.
        let owner = crate::owner::apple::macos::owner_queue();
        let owner_is_main = true;
        options.frame = options
            .frame
            .at_open(&appkit_window_capabilities(windows.presentation));
        let style = FrameStyle::of(options.frame);
        // Convert logical size to NSRect
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(content.width(), content.height()),
        );

        // Build window style mask
        let mut style_mask = NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable;

        // A title bar of the application's keeps the system's, under the
        // content: the frame, its shadow and its traffic lights stay
        // AppKit's, and the content size is the whole window's.
        if style.titled {
            style_mask |= NSWindowStyleMask::Titled;
        }
        if style.app_title_bar {
            style_mask |= NSWindowStyleMask::FullSizeContentView;
        }

        if options.resizable {
            style_mask |= NSWindowStyleMask::Resizable;
        }

        // Create NSWindow
        let allocated = FluiWindow::alloc(main).set_ivars(());
        // SAFETY: `initWithContentRect:styleMask:backing:defer:` is
        // `NSWindow`'s designated initializer, sent once to the instance
        // just allocated (the class has no ivars to set); it takes a
        // rectangle, a style mask, a backing store type and a BOOL by
        // value, on the main thread `main` proves.
        let created: Option<Retained<FluiWindow>> = unsafe {
            msg_send![
                super(allocated),
                initWithContentRect: frame,
                styleMask: style_mask,
                backing: NSBackingStoreType::Buffered,
                defer: false
            ]
        };
        let Some(created) = created else {
            return Err(OpenWindowError::Backend {
                message: "Failed to create NSWindow".to_string(),
            });
        };
        let ns_window: Retained<NSWindow> = Retained::into_super(created);

        // AppKit's default for an alloc/init'd window is
        // `releasedWhenClosed: YES`: `-[NSWindow close]` would then
        // release the same +1 reference this constructor's `alloc`/
        // `init` pair already owns, and the release the `Retained` makes
        // when the wrapper drops would release it a second time — an
        // over-release, and a dangling `ns_window` for every message this
        // type sends afterward (including the `setNeedsDisplay:` behind
        // every redraw request). Opting out here makes the `alloc`/`init`
        // +1 released exactly once, by `Drop`, matching winit's own
        // `setReleasedWhenClosed(false)` for its wrapped `NSWindow`.
        //
        // SAFETY: the window is released by its `Retained` alone: `false`
        // is the value that keeps `close` from releasing it as well.
        unsafe { ns_window.setReleasedWhenClosed(false) };

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
        ns_window.setBackgroundColor(Some(&NSColor::underPageBackgroundColor()));

        // Set window title
        ns_window.setTitle(&NSString::from_str(&options.title));
        style_title_bar(&ns_window, style.app_title_bar);

        // The limits are on the content size, like `options.size`, and the
        // same pair is read back into the state.
        let (min, max) = content_limits_of_constraints(constraints);
        ns_window.setContentMinSize(NSSize::new(min.width, min.height));
        ns_window.setContentMaxSize(NSSize::new(max.width, max.height));

        // A window must opt in to the full-screen transition for
        // `toggleFullScreen:` to do anything. Every window does, so one
        // that becomes resizable later can enter full screen too; a window
        // that cannot be resized is refused the mode by `change_mode`.
        ns_window.setCollectionBehavior(
            ns_window.collectionBehavior() | NSWindowCollectionBehavior::FullScreenPrimary,
        );

        // Where the window opens. Placed before it is ordered front, so
        // it never shows at the system's choice first.
        let primary_height = primary_screen_height(main);
        let placed_at = placed_at
            .zip(primary_height)
            .map(|(at, height)| cocoa_top_left_point(at, height));
        let mut placed = placed_at.is_some();
        if let Some(point) = placed_at {
            ns_window.setFrameTopLeftPoint(NSPoint::new(point.x, point.y));
        }
        // A window that opens maximized or full screen returns to
        // `normal_geometry`: put it there first, then change the mode.
        if options.mode != WindowMode::Normal
            && let Some(normal) = options.normal_geometry
        {
            let current = ns_window.frame();
            let size = normal.size();
            let frame = match (returns_to, primary_height) {
                (Some(at), Some(height)) => {
                    placed = true;
                    cocoa_frame(at, size, height)
                }
                // Without a position the top-left corner stays where it is.
                _ => cocoa_frame_keeping_top(crate::owner::apple::macos::bounds_of(current), size),
            };
            ns_window.setFrame_display(
                NSRect::new(
                    NSPoint::new(frame.origin.x, frame.origin.y),
                    NSSize::new(frame.size.width, frame.size.height),
                ),
                false,
            );
        }

        // Without a position of its own, the window opens centred.
        if !placed {
            ns_window.center();
        }

        // The content size now: `normal_geometry` may have given the frame
        // a size of its own, and no delegate exists yet to report it.
        let content_size = {
            let content = ns_window.contentRectForFrameRect(ns_window.frame());
            Size::new(content.size.width, content.size.height)
        };

        // Get backing scale factor
        let reported = ns_window.backingScaleFactor();
        // By the hosts' one rule; a refusal is reported here, nothing borrowed.
        let mut scale_streak = crate::display::scale::ScaleStreak::default();
        let (scale, refused) = scale_streak.read(reported, "NSWindow backingScaleFactor");
        if let Some(refused) = refused {
            refused.report();
        }

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
        // sees nothing; `reveal_after_first_frame` restores the window's alpha.
        // The default `WindowReveal::AtOpen` leaves alpha alone — the
        // caller has no first frame to report — and `visible: false`
        // stays hidden until asked, as before.
        let first_reveal_pending = defers_first_reveal(&options);
        if first_reveal_pending {
            ns_window.setAlphaValue(0.0);
        }

        // Create content view for input events. `frame` is
        // objc2-foundation's `NSRect`, the type `create_content_view` takes.
        let content_view = content_view::create(frame, Arc::downgrade(&delivery), view_hooks, main);

        // What the features beside the window make their part of it from.
        let closed = Arc::new(AtomicBool::new(false));
        let this = ContentViewRef {
            view: objc_address(&content_view),
            closed: Arc::clone(&closed),
            owner,
            owner_is_main,
        };
        ns_window.setContentView(Some(&content_view));
        // The scale the window's metrics, and so its drawables, are sized by.
        content_view::set_layer_contents_scale(&content_view, scale);

        // Made once the content view is installed: the bridge subclasses
        // it for VoiceOver, on the main thread, and the window's teardown
        // detaches it before releasing the window and then the view.
        let facets = windows.positions.for_window(&this);

        let window = Rc::new(Self {
            ns_window: ManuallyDrop::new(ns_window),
            main,
            id,
            view: ManuallyDrop::new(content_view),
            state: RefCell::new(MacOSWindowState {
                bounds: Bounds {
                    origin: Point::new(frame.origin.x, frame.origin.y),
                    size: content_size,
                },
                scale_factor: scale,
                scale_streak,
                display: None,
                layer_refresh: LayerRefresh::default(),
                cursor: cursor::NativeCursor::ARROW,
                cursor_grab: CursorGrab::None,
                lock_at: None,
                level: objc2_app_kit::NSNormalWindowLevel,
                live_resize: false,
                resize_increments: None,
                aspect_ratio: None,
                normal: None,
                sequence: ModeSequence::default(),
                occlusion_visible: true,
                first_reveal_pending,
                alpha: MacOSWindowAlpha::OPAQUE,
                frame: options.frame,
                system_controls: None,
            }),
            windows_map: Arc::clone(&windows_map),
            delivery,
            closed,
            loop_control: OnceCell::new(),
            delegate: OnceCell::new(),
            presentation: windows.presentation,
            facets,
            redraw: OnceCell::new(),
            timed_frame: RefCell::new(TimedFrame::default()),
            displays: OnceCell::new(),
            snapshot: Arc::new(WindowStateCell::new(
                WindowState::new(WindowGeometry::unplaced(content), content)
                    .with_title(Some(options.title.clone()))
                    .with_constraints(constraints)
                    .with_resizable(options.resizable)
                    .with_frame(options.frame),
            )),
            wait_end_read: Cell::new(WaitEndRead::default()),
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

        // The traffic lights over a title bar of the application's, hidden
        // or moved as it asked.
        window.place_system_controls();

        // Enable mouse tracking for mouse moved events
        content_view::enable_mouse_tracking(&window.view);

        // Make content view first responder to receive keyboard events
        let _ = window.ns_window.makeFirstResponder(Some(&window.view));

        // Set window delegate for lifecycle events
        let delegate = create_window_delegate(Rc::downgrade(&window), main);
        window
            .ns_window
            .setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        let _ = window.delegate.set(delegate);

        tracing::info!(
            handle = ?window.ns_window(),
            width = content.width(),
            height = content.height(),
            scale = scale.get(),
            "a window was made"
        );

        Ok(UnshownWindow {
            window,
            visible: options.visible,
            mode: options.mode,
        })
    }

    /// Get the NSWindow handle
    pub fn ns_window(&self) -> *mut AnyObject {
        Retained::as_ptr(&self.ns_window).cast_mut().cast()
    }

    /// Attach the owning standalone loop without creating a window/control cycle.
    fn install_loop_control(
        &self,
        control: std::sync::Weak<crate::owner::apple::macos::LoopControl>,
    ) {
        let _ = self.loop_control.set(control);
    }
}

/// A window made, its delegate set, and not yet shown or tracked: the only
/// way to the window is [`show`](Self::show), which hands it what the loop
/// shares with every window before the window can tell anyone anything.
#[must_use = "a window is shown and tracked by `show`"]
pub(crate) struct UnshownWindow {
    window: Rc<MacOSWindow>,
    visible: bool,
    mode: WindowMode,
}

impl UnshownWindow {
    /// Bind the window to `owner`, track it, then show it as its options
    /// asked. The order is the point: ordering a window front makes it key,
    /// and the delegate's `windowDidBecomeKey:` publishes its first snapshot
    /// and runs the application's handler, which by then reaches a window
    /// that names its display, wakes its loop and is in the window map.
    fn show(self, owner: &MacOSLoop) -> Rc<MacOSWindow> {
        let Self {
            window,
            visible,
            mode,
        } = self;
        owner.notices.mark_stale_on_wake(window.state_cell());
        // What another thread raises for the window waits for an owner turn.
        window
            .delivery
            .resume_through_signal(&owner.loop_control.owner_signal);
        window.install_loop_control(Arc::downgrade(&owner.loop_control));
        window.install_wake_pump(Arc::downgrade(&owner.wake_pump));
        window.bind_displays(Arc::clone(&owner.displays));

        // The map is the owner thread's, where the window is made.
        let tracked: Rc<dyn HostWindow> = Rc::clone(&window) as _;
        let inserted = window.windows_map.insert(window.native_key(), tracked);
        assert!(
            matches!(inserted, Ok(None)),
            "BUG: a new window's key is held by no open window (a window leaves the map before \
             AppKit can reuse its address), and the window map, made on this thread, has an \
             owner slot (none are left once owner slots are exhausted)"
        );

        // The rectangle the window returns to is what it has now, placed as
        // asked; then it is shown and enters the mode it opens in. A hidden
        // window enters it when it is first shown.
        window.state.borrow_mut().normal = window.outer_geometry();
        if visible {
            window.ns_window.makeKeyAndOrderFront(None);
            if mode != WindowMode::Normal {
                // A window that cannot take the mode opens normal, as the
                // options document; its first snapshot says so.
                let _ = window.change_mode(mode);
            }
        } else if mode != WindowMode::Normal {
            window.state.borrow_mut().sequence.enter_when_shown(mode);
        }
        window
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
    /// Main thread only, which the window being neither `Send` nor `Sync` proves.
    fn set_needs_display(&self, deferred: bool) {
        // The frame this asks for carries the display's current refresh: a
        // rate change since the last frame is a new snapshot revision.
        self.sync_layer_refresh();
        self.view.setNeedsDisplay(true);
        // AppKit owns the display pass that turns this into a
        // call of `updateLayer` — nothing in this backend can observe it, so the
        // two ends of the pump are instrumented separately: this line
        // (the request went out) and the content view's `update_layer` (AppKit
        // asked for content). A request with no matching `updateLayer` is
        // otherwise invisible from here, so when a trace subscriber is
        // attached, report the AppKit state the display pass is gated
        // on. The reads are messages, so they are behind the level
        // check rather than unconditional fields.
        if tracing::enabled!(tracing::Level::TRACE) {
            let app = NSApplication::sharedApplication(self.main);
            let view_frame = self.view.frame();
            tracing::trace!(
                window_visible = self.ns_window.isVisible(),
                occlusion = self.ns_window.occlusionState().0,
                needs_display = self.view.needsDisplay(),
                view_in_window = self.view.window().is_some(),
                is_key = self.ns_window.isKeyWindow(),
                app_active = app.isActive(),
                policy = app.activationPolicy().0,
                window_number = self.ns_window.windowNumber(),
                on_active_space = self.ns_window.isOnActiveSpace(),
                has_screen = self.ns_window.screen().is_some(),
                alpha = self.ns_window.alphaValue(),
                level = self.ns_window.level(),
                view_hidden = self.view.isHiddenOrHasHiddenAncestor(),
                vx = view_frame.origin.x,
                vy = view_frame.origin.y,
                vw = view_frame.size.width,
                vh = view_frame.size.height,
                backing_scale = self.ns_window.backingScaleFactor(),
                deferred,
                "request_redraw: setNeedsDisplay sent to content view"
            );
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
            || {
                defer_redraw(
                    crate::owner::apple::macos::owner_queue(),
                    Arc::clone(handle),
                    0,
                );
            },
            || self.set_needs_display(false),
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
fn defer_redraw(
    owner: &'static dispatch2::DispatchQueue,
    window: Arc<OwnerWeak<MacOSWindow>>,
    hop: u8,
) {
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
            // The handle upgraded, so this turn runs on the window's owner
            // thread (the only one it upgrades on), under the lane guard
            // `exec_async_on_owner` installed.
            Some(window) if !window.closed.load(Ordering::SeqCst) => {
                window.set_needs_display(true);
            }
            _ => tracing::trace!(
                "request_redraw: window closed before the deferred request ran; request dropped"
            ),
        }
    });
}

/// Run `window`'s armed `tick` for the timed frame it was asked for, on its
/// owner lane after `delay`: the live tick, served, asks the window for a
/// frame, and early, runs again for the rest; a superseded one does nothing
/// ([`TimedFrame::tick`]). A window closed or gone by then has nothing to
/// draw.
fn schedule_timed_frame(
    owner: &'static dispatch2::DispatchQueue,
    window: Arc<OwnerWeak<MacOSWindow>>,
    tick: ArmedTick,
    delay: std::time::Duration,
) {
    crate::owner::apple::macos::exec_after_on_owner(owner, delay, move || {
        let Some(owned) = window.upgrade() else {
            return;
        };
        if owned.closed.load(Ordering::SeqCst) {
            return;
        }
        let ran = owned
            .timed_frame
            .borrow_mut()
            .tick(tick, flui_platform::EventTime::now());
        match ran {
            TimedTick::Serve => PlatformWindow::request_redraw(&*owned),
            TimedTick::Wait(rest) => schedule_timed_frame(owner, window, tick, rest),
            TimedTick::Stale => {}
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
    owner: &'static dispatch2::DispatchQueue,
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
    /// paces this window's frames. The frame goes to the slot of the drawable
    /// the view's layer handed out last, which this present presents, so the
    /// drawable's presented handler reports that frame; the frames render on
    /// the main thread (the raster lane is inline on macOS), and a present
    /// made off it, or after the window is gone, names no frame and is not
    /// reported. It returns at once.
    fn pre_present_notify(
        &self,
        frame: Option<flui_foundation::FrameStamp>,
        _: flui_platform::SwapChainStatistics,
    ) {
        let Some(main) = MainThreadMarker::new() else {
            return;
        };
        if self.is_gone() {
            return;
        }
        if let Some(view) = self
            .view
            .object(main)
            .and_then(|view| view.downcast_ref::<content_view::FluiContentView>())
        {
            content_view::layer_presenting(view, frame);
        }
    }

    /// Records the keep and changes nothing else: a window still open keeps
    /// showing and taking input. Its close, whichever way it comes
    /// (`windowWillClose:`, or the window's drop without one), then orders it
    /// out and keeps its view, and so the view's Metal layer, for the life of
    /// the process (`MacOSWindow::clear_then_keep_if_kept`), when the keep
    /// came before the close let go of the handler. A keep after that does
    /// nothing.
    fn keep_past_close(&self) {
        let _first = self.retirement.keep_past_close();
    }
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

    /// The content view's client: AppKit's input method reads and edits the
    /// focused field's store through it (ADR-0090).
    fn text_store_host(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Option<std::rc::Rc<dyn flui_platform::text::store::TextStoreHost>> {
        self.facets
            .text
            .window_text_input()
            .and_then(crate::text::WindowTextInput::store_host)
    }

    /// The window server composites every window and shows a covered one's
    /// last frame again the moment it is uncovered.
    fn occluded_surface(&self) -> flui_platform::OccludedSurface {
        flui_platform::OccludedSurface::Kept
    }
}

/// What the macOS windows open with: the table's window-level positions and
/// the mint of their ids. The loop keeps the open windows themselves
/// (`WindowRegistry`), because its wake pump and its stop reach them.
#[derive(Debug)]
pub struct MacOSWindows {
    positions: WindowPositions<ContentViewRef>,
    /// Issues the id of every window the platform opens, on the main thread
    /// that built the platform, never one twice: AppKit reuses the address
    /// of a released `NSWindow`, so an address is never an id.
    ids: OwnerCell<WindowIdMint>,
    /// Why the machine's display tells no window when its frames reached the
    /// screen, read once with the platform; `None` where it tells.
    presentation: Option<flui_platform::CapabilityAbsence>,
}

impl MacOSWindows {
    /// The windows of a platform whose table's window-level positions are
    /// `positions`, on the main thread that will open them.
    pub(crate) fn new(positions: WindowPositions<ContentViewRef>) -> Self {
        Self {
            positions,
            ids: OwnerCell::new(),
            presentation: appkit_presentation(hypervisor_guest()),
        }
    }
}

/// Whether this Mac runs as a hypervisor's guest: `kern.hv_vmm_present`,
/// the sysctl Apple names for it. A system that does not answer is taken as
/// none.
fn hypervisor_guest() -> bool {
    let mut present: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>();
    // SAFETY: the name is a NUL-terminated literal; `present` and `size` are
    // live locals the call writes at most `size` bytes into, and no new value
    // is passed.
    let read = unsafe {
        libc::sysctlbyname(
            c"kern.hv_vmm_present".as_ptr(),
            (&raw mut present).cast(),
            &raw mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    read == 0 && present == 1
}

/// The options' position and the position of their normal rectangle as
/// points of AppKit's flipped desktop, each through its own screen in the
/// published snapshot, which is all a window knows how to place. A position on
/// a screen the snapshot no longer holds is no place: the system places the
/// window, as the options document.
fn desktop_placement(
    displays: &dyn HostDisplays,
    options: &WindowOptions,
) -> (Option<Point<f64>>, Option<Point<f64>>) {
    let snapshot = displays.published();
    let place = |at: WindowPosition| desktop_point(at, snapshot.get(at.display())?);
    (
        options.position.and_then(place),
        options
            .normal_geometry
            .and_then(flui_platform::WindowGeometry::position)
            .and_then(place),
    )
}

impl host::Window<MacOSLoop> for MacOSWindow {
    type Shared = MacOSWindows;
    type Ref = ContentViewRef;

    fn open(
        owner: &MacOSLoop,
        windows: &Arc<MacOSWindows>,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<Rc<dyn HostWindow>, OpenWindowError> {
        if !owner.loop_control.accepts_windows(owner.main) {
            return Err(OpenWindowError::OwnerGone {
                rejected: Some(Box::new(options)),
            });
        }
        // Refused before an id is taken, for what a command is refused for,
        // and checked as the application wrote it, before the position is
        // turned into a point of the desktop.
        let opening = options.geometry()?;
        let (at, returns_to) = desktop_placement(owner.displays.as_ref(), &options);
        let Some(id) = windows.ids.with(|ids| ids.next()) else {
            return Err(OpenWindowError::Backend {
                message: "a macOS window opens on the main thread that built its platform"
                    .to_owned(),
            });
        };
        let window = MacOSWindow::try_new(
            owner.main,
            id?,
            Opening {
                options,
                geometry: opening,
                at,
                returns_to,
            },
            // Each frame the handler runs counts into the pump's count, which
            // arms a delivered wake deadline again only after a frame ran,
            // and each run ends in a look at the deadline.
            owner
                .wake_pump
                .window_delivery(handler, crate::input::availability::macos()),
            Arc::clone(&owner.windows),
            windows,
            owner.view_hooks,
        )?;
        Ok(window.show(owner))
    }

    fn capabilities(_: &MacOSLoop, windows: &MacOSWindows) -> WindowCapabilities {
        // Every window the platform opens routes through the AppKit main lane
        // (`MacOSWindow::try_new`).
        appkit_window_capabilities(windows.presentation)
    }

    fn frame_inset(
        owner: &MacOSLoop,
        _: &MacOSWindows,
        _scale_factor: flui_foundation::geometry::DevicePixelRatio,
    ) -> Size<f64> {
        // AppKit measures a frame in points, so the scale does not enter.
        decorated_frame_inset(owner.main)
    }

    /// AppKit's display handle, which carries no pointer.
    fn display_connection(
        _: &MacOSLoop,
        _: &MacOSWindows,
    ) -> std::sync::Arc<dyn flui_platform::DisplayConnection> {
        std::sync::Arc::new(crate::display_connection::PointerlessDisplay::AppKit)
    }
}

objc2::define_class!(
    // SAFETY: `NSWindow` may be subclassed; the class adds no ivars, and
    // `MainThreadOnly` is `NSWindow`'s own thread kind.
    #[unsafe(super(objc2_app_kit::NSWindow, objc2_app_kit::NSResponder, objc2_foundation::NSObject))]
    #[thread_kind = objc2::MainThreadOnly]
    #[name = "FLUIWindow"]
    /// The class every window is an instance of: `NSWindow`, except that it
    /// can take keyboard focus without a title bar. A plain borderless
    /// `NSWindow` answers `canBecomeKeyWindow` with `NO`, which would leave a
    /// window that is opened undecorated, or made undecorated later, deaf to
    /// the keyboard.
    struct FluiWindow;

    impl FluiWindow {
        // A constant answer runs no code, so it needs no boundary.
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main_window(&self) -> bool {
            true
        }
    }
);

/// Make the title bar of `ns_window` transparent with its title hidden, for a
/// title bar of the application's drawn under it, or the system's again.
fn style_title_bar(ns_window: &NSWindow, app_title_bar: bool) {
    ns_window.setTitlebarAppearsTransparent(app_title_bar);
    ns_window.setTitleVisibility(if app_title_bar {
        NSWindowTitleVisibility::Hidden
    } else {
        NSWindowTitleVisibility::Visible
    });
}

/// How much larger a titled, closable, miniaturizable and resizable window's
/// frame is than its content, in points: the title bar. Asked of the class, so
/// it is known before any window exists. The marker is the proof that the
/// caller is on the main thread, where every AppKit message in this module runs.
fn decorated_frame_inset(main: MainThreadMarker) -> Size<f64> {
    let mask = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
    let frame = NSWindow::frameRectForContentRect_styleMask(
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
        mask,
        main,
    );
    Size::new(frame.size.width, frame.size.height)
}

/// The height of the primary screen (`NSScreen.screens[0]`), whose bottom-left
/// corner is the origin AppKit measures every frame from; `None` with no screen.
/// The marker is the proof that the caller is on the main thread, where every
/// AppKit message in this module runs.
fn primary_screen_height(main: MainThreadMarker) -> Option<f64> {
    NSScreen::screens(main)
        .firstObject()
        .map(|primary| primary.frame().size.height)
}

/// Where a window at AppKit's `level` stands among the others: above the
/// normal level is kept on top (the floating level, and the higher ones an
/// application sets through `AppKitWindow::set_window_level`), below it is
/// kept under them.
fn level_of(level: objc2_app_kit::NSWindowLevel) -> WindowLevel {
    match level.cmp(&objc2_app_kit::NSNormalWindowLevel) {
        std::cmp::Ordering::Greater => WindowLevel::AlwaysOnTop,
        std::cmp::Ordering::Less => WindowLevel::AlwaysOnBottom,
        std::cmp::Ordering::Equal => WindowLevel::Normal,
    }
}

/// The bands of `screen` its camera housing takes (`safeAreaInsets`), in
/// points in from each edge; all zero on a screen that has none. The proof
/// says the running AppKit answers the message.
fn unsafe_edges_of(
    screen: &NSScreen,
    _: crate::version::gate::Since<ScreenSafeArea>,
) -> flui_foundation::geometry::EdgeInsets {
    let insets = screen.safeAreaInsets();
    flui_foundation::geometry::EdgeInsets::new(insets.top, insets.right, insets.bottom, insets.left)
}

impl PlatformWindow for MacOSWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    /// AppKit's gesture distances are not read yet; the consumer keeps its
    /// own.
    fn gesture_geometry(
        &self,
    ) -> Result<Option<flui_platform::GestureGeometry>, flui_platform::PreferenceQueryError> {
        Ok(None)
    }

    /// The stacking of this process's windows is not read yet, so the window
    /// under a point is not known.
    fn window_at(&self, _: Point<f64>) -> flui_platform::WindowAt {
        flui_platform::WindowAt::Unknown
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::haptics::PlatformHaptics>> {
        Some(Arc::clone(&self.facets.haptics))
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        Some(Arc::clone(&self.facets.chrome))
    }

    /// AppKit starts no OS drag session from this window yet; its input
    /// capabilities say so.
    fn drag_source(&self) -> Option<Arc<dyn flui_platform::transfer::drag::PlatformDragSource>> {
        None
    }

    /// None: the window's text position is a store host (`text_store_host`),
    /// which AppKit's input method pulls from; no capability is pushed to.
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

    /// Served for this window alone: a tick on its owner lane at `at` makes
    /// the request [`request_redraw`](PlatformWindow::request_redraw) makes,
    /// once. One tick is live at a time: a request for the instant it serves
    /// arms none, and the tick of an instant replaced or withdrawn since does
    /// nothing ([`TimedFrame`]). Nothing here reaches the wake pump's
    /// deadline, whose deliveries ask every window.
    fn request_frame_at(&self, at: Option<flui_platform::EventTime>) {
        let armed = self
            .timed_frame
            .borrow_mut()
            .request(at, flui_platform::EventTime::now());
        if let Some((tick, delay)) = armed {
            let redraw = self.redraw();
            schedule_timed_frame(redraw.owner, Arc::clone(&redraw.window), tick, delay);
        }
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
            WindowCommand::SetPosition(at) => self.reframe(|| self.set_native_position(at))?,
            WindowCommand::SetSize(size) => self.reframe(|| self.set_content_size(size.get())),
            WindowCommand::SetConstraints(constraints) => {
                self.reframe(|| self.set_native_constraints(constraints));
            }
            WindowCommand::SetResizable(on) => {
                self.reframe(|| self.set_style_bit(NSWindowStyleMask::Resizable, on))?;
            }
            WindowCommand::SetFrame(frame) => self.reframe(|| self.set_frame(frame))?,
            WindowCommand::SetMode(mode) => self.change_mode(mode)?,
            WindowCommand::Show => self.show_window()?,
            // A hidden window holds no pointer.
            WindowCommand::Hide => {
                self.hide()?;
                self.let_pointer_go();
            }
            WindowCommand::Activate => self.activate_window(),
            // AppKit tears the window down through `windowWillClose:`.
            WindowCommand::Close => {
                self.close_window();
                return Ok(());
            }
            // The material is no fact of the snapshot: nothing to publish.
            WindowCommand::SetMaterial(material) => return self.apply_material(material),
            // The snapshot says what is held whatever the answer: a refused
            // lock commits nothing and is published as no grab.
            WindowCommand::SetCursorGrab(CursorGrab::Locked) => {
                let locked = self.lock_pointer();
                self.publish_state();
                return locked;
            }
            WindowCommand::SetCursorGrab(CursorGrab::None) => {
                self.let_pointer_go();
            }
            WindowCommand::SetLevel(level) => self.set_level(level),
            // What the user's resize keeps is held by AppKit, and is no fact
            // of the snapshot: nothing to publish.
            WindowCommand::SetResizeIncrements(step) => {
                self.state.borrow_mut().resize_increments = step;
                self.apply_resize_rules();
                return Ok(());
            }
            WindowCommand::SetAspectRatio(ratio) => {
                self.state.borrow_mut().aspect_ratio = ratio;
                self.apply_resize_rules();
                return Ok(());
            }
            // A command added to the contract that this backend has not met
            // is refused, not ignored.
            other => return Err(crate::window::state::not_carried_out(&other)),
        }
        self.publish_state();
        Ok(())
    }

    fn reveal_after_first_frame(&self) {
        // The flag is settled before the message, so a second call finds no
        // reveal pending: the window takes the application's alpha exactly
        // once for its deferred reveal.
        let Some(alpha) = self.take_pending_first_reveal() else {
            return;
        };
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        self.ns_window.setAlphaValue(alpha.get());
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
            flui_platform::CapabilityAbsence::NOT_ON_THIS_DEVICE,
            |displays| frame_rate_absence(displays.version()),
        );
        Err(absence.into())
    }

    /// The content view answers a left press from the move areas of a title
    /// bar the application draws (ADR-0286 §8). AppKit keeps the resize
    /// borders and draws the caption buttons itself, and lets no window keep
    /// the system's trackpad gestures out, so the other kinds are not honoured
    /// (`input::availability::macos`). A closed window keeps nothing and
    /// answers `NoWindow`.
    fn set_input_regions(
        &self,
        regions: &[flui_platform::input::regions::InputRegion],
    ) -> Result<(), flui_platform::Unsupported> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(flui_platform::Unsupported::new(
                flui_platform::UnsupportedReason::NoWindow,
            ));
        }
        let scale = self.state.borrow().scale_factor;
        content_view::set_input_regions(&self.view, regions, scale);
        Ok(())
    }

    /// Every cursor, an image included, set at once as AppKit's current
    /// cursor. A repeated one is the cursor the window already made, set
    /// again; the one it replaces is released only once the new one shows.
    /// While the window holds the pointer locked its transparent cursor stays,
    /// and the cursor asked for shows when the lock is let go.
    fn set_cursor(&self, cursor: &Cursor) -> Result<(), CursorError> {
        let (kept, scale) = {
            let state = self.state.borrow();
            let scale = state.scale_factor;
            (state.cursor.shows(cursor, scale), scale)
        };
        // Made with the state let go: AppKit is messaged.
        let made = if kept {
            None
        } else {
            Some(cursor::NativeCursor::of(cursor, scale)?)
        };
        let (shown, replaced) = {
            let mut state = self.state.borrow_mut();
            let replaced = made.map(|made| std::mem::replace(&mut state.cursor, made));
            let shown = (state.cursor_grab != CursorGrab::Locked).then(|| state.cursor.clone());
            (shown, replaced)
        };
        if let Some(shown) = shown {
            shown.ns_cursor().set();
        }
        drop(replaced);
        Ok(())
    }
}

/// The bodies of the show, activate and close commands.
impl MacOSWindow {
    /// Show the window, deminiaturizing it and making it key; the zoomed or
    /// fullscreen mode is kept.
    fn show_window(&self) -> Result<(), ApplyError> {
        // An explicit show settles a deferred first reveal: the window is
        // given the application's alpha along with being ordered.
        let reveal = self.take_pending_first_reveal();
        self.order_front(reveal)?;
        self.enter_mode_when_shown();
        Ok(())
    }

    /// Deminiaturize the window and order it front as the key window, made
    /// given `reveal` first, the application's alpha a deferred first reveal
    /// owes it. A window that closes meanwhile (a
    /// deminiaturize reaches the delegate) answers `Closed`.
    fn order_front(&self, reveal: Option<MacOSWindowAlpha>) -> Result<(), ApplyError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(ApplyError::Closed);
        }
        // Deminiaturizing and ordering do not toggle zoom/fullscreen.
        if let Some(alpha) = reveal {
            self.ns_window.setAlphaValue(alpha.get());
        }
        if self.ns_window.isMiniaturized() {
            self.ns_window.deminiaturize(None);
        }
        if self.closed.load(Ordering::SeqCst) {
            return Err(ApplyError::Closed);
        }
        self.ns_window.makeKeyAndOrderFront(None);
        if self.closed.load(Ordering::SeqCst) {
            Err(ApplyError::Closed)
        } else {
            Ok(())
        }
    }

    fn activate_window(&self) {
        let reveal = self.take_pending_first_reveal();
        if let Some(alpha) = reveal {
            self.ns_window.setAlphaValue(alpha.get());
        }
        self.ns_window.makeKeyAndOrderFront(None);
        self.enter_mode_when_shown();
    }

    /// Hold the pointer in place for the key window: detach the mouse from
    /// the cursor, hide the cursor under the window's transparent one, and
    /// have the content view's input read the mouse's motion as raw motion.
    /// A window that is not key, not shown or miniaturized takes no grab, and
    /// its snapshot says so; a window that already holds one keeps it.
    ///
    /// # Errors
    /// [`ApplyError::Platform`] when AppKit makes no transparent cursor or
    /// Quartz refuses to detach the mouse; nothing is held then.
    fn lock_pointer(&self) -> Result<(), ApplyError> {
        let (key, flags) = (self.ns_window.isKeyWindow(), self.read_flags());
        if !key
            || !flags.on_screen
            || flags.miniaturized
            || self.state.borrow().cursor_grab == CursorGrab::Locked
        {
            return Ok(());
        }
        let blank = cursor::blank_cursor().map_err(ApplyError::Platform)?;
        cursor::detach_mouse(true).map_err(ApplyError::Platform)?;
        // The pointer rests where it is over the content, or at its centre
        // when it is elsewhere, as the Win32 lock does.
        // `mouseLocation` is a class getter answering a point in screen
        // coordinates.
        let at = self.content_point_of_screen(NSEvent::mouseLocation());
        // Committed before anything else is told: the view's input reads
        // motion from the next mouse event on.
        {
            let mut state = self.state.borrow_mut();
            state.cursor_grab = CursorGrab::Locked;
            state.lock_at = at;
        }
        content_view::set_pointer_locked(&self.view, true);
        blank.set();
        self.keep_lock_point();
        Ok(())
    }

    /// The content view's point under `screen`, a point in AppKit's screen
    /// coordinates; the content's centre when `screen` is not over it.
    fn content_point_of_screen(&self, screen: NSPoint) -> Option<NSPoint> {
        let in_window = self.ns_window.convertPointFromScreen(screen);
        let in_view = self.view.convertPoint_fromView(in_window, None);
        let bounds = self.view.bounds();
        let inside = (bounds.origin.x..bounds.origin.x + bounds.size.width).contains(&in_view.x)
            && (bounds.origin.y..bounds.origin.y + bounds.size.height).contains(&in_view.y);
        let at = if inside {
            in_view
        } else {
            NSPoint::new(
                bounds.origin.x + bounds.size.width / 2.0,
                bounds.origin.y + bounds.size.height / 2.0,
            )
        };
        (at.x.is_finite() && at.y.is_finite()).then_some(at)
    }

    /// Put a locked pointer back on the point of the content it rests at:
    /// the mouse is detached, so the cursor stays at a screen point while the
    /// window moves, resizes or changes screen, and would rest outside the
    /// content. The point is kept relative to the content view, held inside
    /// it when the content shrank, and the snapshot stays `Locked`. A window
    /// that holds no lock warps nothing.
    fn keep_lock_point(&self) {
        let Some(at) = self.state.borrow().lock_at else {
            return;
        };
        let bounds = self.view.bounds();
        let held = NSPoint::new(
            at.x.clamp(
                bounds.origin.x,
                bounds.origin.x + (bounds.size.width - 1.0).max(0.0),
            ),
            at.y.clamp(
                bounds.origin.y,
                bounds.origin.y + (bounds.size.height - 1.0).max(0.0),
            ),
        );
        let in_window = self.view.convertPoint_toView(held, None);
        let screen = self.ns_window.convertPointToScreen(in_window);
        // Quartz measures from the primary screen's top-left corner.
        let global = primary_screen_height(self.main).map(|height| (screen.x, height - screen.y));
        let Some((x, y)) = global.filter(|(x, y)| x.is_finite() && y.is_finite()) else {
            return;
        };
        if let Err(error) = cursor::warp_cursor(x, y) {
            tracing::debug!(%error, "a locked pointer could not be put back over the content");
        }
    }

    /// Let the pointer go if the window holds it: attach the mouse to the
    /// cursor again and show the cursor the presentation asked for. A window
    /// that holds nothing is left as it is. Run when the application lets go,
    /// and when the window stops being key, is hidden, miniaturized or
    /// closed: the association is the process's, and no other window may be
    /// left with the mouse detached.
    /// Answers whether it let a lock go, which the caller publishes.
    fn let_pointer_go(&self) -> bool {
        let shown = {
            let mut state = self.state.borrow_mut();
            state.lock_at = None;
            if std::mem::replace(&mut state.cursor_grab, CursorGrab::None) != CursorGrab::Locked {
                return false;
            }
            state.cursor.clone()
        };
        if let Err(error) = cursor::detach_mouse(false) {
            tracing::warn!(%error, "the mouse could not be attached to the cursor again");
        }
        content_view::set_pointer_locked(&self.view, false);
        shown.ns_cursor().set();
        true
    }

    /// Keep the window above the other windows (the floating level), below
    /// them (one under the normal level, as AppKit has no level of its own
    /// for it), or at the normal level. A full-screen window takes it when it
    /// leaves full screen (`apply_level`).
    fn set_level(&self, level: WindowLevel) {
        use objc2_app_kit::{NSFloatingWindowLevel, NSNormalWindowLevel};
        let value = match level {
            WindowLevel::Normal => NSNormalWindowLevel,
            WindowLevel::AlwaysOnTop => NSFloatingWindowLevel,
            WindowLevel::AlwaysOnBottom => NSNormalWindowLevel - 1,
        };
        self.keep_level(value);
    }

    /// Stand the window at AppKit's `value` outside full screen: sent now
    /// unless the window is full screen, then recorded (AppKit's `setLevel:`
    /// refuses nothing), so the snapshot publishes what was set.
    fn keep_level(&self, value: objc2_app_kit::NSWindowLevel) {
        if !self.read_flags().fullscreen {
            self.send_level(value);
        }
        self.state.borrow_mut().level = value;
    }

    /// Send AppKit the level the window stands at now: the normal level while
    /// it is `fullscreen` (full screen wins over a level while it lasts, so a
    /// window kept above or below the others still takes its own Space), its
    /// own level otherwise. Run as the window enters full screen and as each
    /// transition ends, so the level comes back when full screen is left.
    fn apply_level(&self, fullscreen: bool) {
        let value = if fullscreen {
            objc2_app_kit::NSNormalWindowLevel
        } else {
            self.state.borrow().level
        };
        self.send_level(value);
    }

    /// `setLevel:`, unless the window stands at `value` already.
    fn send_level(&self, value: objc2_app_kit::NSWindowLevel) {
        if self.ns_window.level() != value {
            self.ns_window.setLevel(value);
        }
    }

    /// Hand AppKit what the user's resize keeps. AppKit holds the content's
    /// resize increments or its aspect ratio, setting one cancelling the
    /// other, so a proportion the application asked for is held while it
    /// lasts, and the steps once it is lifted; with neither, both are
    /// cleared (an aspect ratio of zero, steps of one point).
    fn apply_resize_rules(&self) {
        let (step, ratio) = {
            let state = self.state.borrow();
            (state.resize_increments, state.aspect_ratio)
        };
        if let Some(ratio) = ratio {
            let proportion = NSSize::new(
                f64::from(ratio.width().get()),
                f64::from(ratio.height().get()),
            );
            self.ns_window.setContentAspectRatio(proportion);
            return;
        }
        self.ns_window.setContentAspectRatio(NSSize::new(0.0, 0.0));
        let increments = step.map_or(NSSize::new(1.0, 1.0), |step| {
            NSSize::new(step.width(), step.height())
        });
        self.ns_window.setContentResizeIncrements(increments);
    }

    /// The user started or ended resizing the window by its frame
    /// (`windowWillStartLiveResize:`, `windowDidEndLiveResize:`): the
    /// snapshot's interaction says so.
    fn handle_live_resize(&self, resizing: bool) {
        self.state.borrow_mut().live_resize = resizing;
        self.publish_state();
    }

    /// The window's backing scale changed: an image cursor, measured in
    /// points at the old scale, is made again at the new one, and shown again
    /// unless the pointer is locked under the transparent cursor.
    fn rescale_cursor(&self) {
        let (made, scale) = {
            let state = self.state.borrow();
            (state.cursor.clone(), state.scale_factor)
        };
        let next = match made.rescaled(scale) {
            Some(Ok(next)) => next,
            Some(Err(error)) => {
                tracing::debug!(%error, "the cursor image could not be measured at the new scale");
                return;
            }
            None => return,
        };
        let (shown, replaced) = {
            let mut state = self.state.borrow_mut();
            let replaced = std::mem::replace(&mut state.cursor, next);
            let shown = (state.cursor_grab != CursorGrab::Locked).then(|| state.cursor.clone());
            (shown, replaced)
        };
        if let Some(shown) = shown {
            shown.ns_cursor().set();
        }
        drop((made, replaced));
    }

    fn close_window(&self) {
        // No separate flag-set needed here: AppKit's `-[NSWindow close]`
        // posts `NSWindowWillCloseNotification` (routing to the delegate's
        // `windowWillClose:`, and so to `handle_close` below) whether or
        // not `windowShouldClose:` was ever asked — the same notification a
        // user-initiated close takes. `handle_close` is therefore reached
        // by every route through this window's own `close`, and is where
        // `closed` is actually set.
        self.ns_window.close();
    }
}

impl Drop for MacOSWindow {
    fn drop(&mut self) {
        tracing::debug!(handle = ?self.ns_window(), "window closing");

        // The window's surface targets and the features beside it outlive
        // it; from here on they hand out nothing of a view the release below
        // may free. A window that closed set this already, and its close
        // (`handle_close`) let go of its handler there, whatever step of it
        // panicked.
        if !self.closed.swap(true, Ordering::SeqCst) {
            // A handler method queued ahead of the release may panic: the
            // drop still finishes.
            crate::owner::panic_boundary::contain_owner_callback(|| self.clear_then_keep_if_kept());
        }

        // A window dropped without closing may still hold the pointer: the
        // mouse's association is the process's, so it is attached again.
        if self.state.get_mut().cursor_grab == CursorGrab::Locked
            && let Err(error) = cursor::detach_mouse(false)
        {
            tracing::warn!(%error, "the mouse could not be attached to the cursor again");
        }

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
        // AppKit holds the delegate weakly: it is detached before the window
        // lets go of it, and released by the pool, since the drop may run
        // inside one of its methods (a handler that let the last wrapper go).
        if let Some(delegate) = self.delegate.take() {
            self.ns_window.setDelegate(None);
            let _ = Retained::autorelease_ptr(delegate);
        }
        // SAFETY: `Drop` runs once, and nothing reads `self.view` after this.
        let view = unsafe { ManuallyDrop::take(&mut self.view) };
        // SAFETY: `Drop` runs once, and nothing reads `self.ns_window` after
        // this.
        let mut ns_window = Some(unsafe { ManuallyDrop::take(&mut self.ns_window) });
        super::teardown::take_apart(
            view,
            &mut [&mut || a11y.detach(), &mut || text.detach()],
            // The wrapper owns the `alloc`/`init` +1 of the window
            // (`releasedWhenClosed` is off): this is the one balancing
            // release, on the owner thread.
            &mut || drop(ns_window.take()),
            |step| crate::owner::apple::macos::run_cleanup_guarded(step),
        );
    }
}

impl MacOSWindow {
    /// Let go of the handler where the window closes (`windowWillClose:`, or
    /// its drop without one), then settle its fate
    /// ([`super::clear_then_keep_if_kept`]): when a keep was recorded for it
    /// (`SurfaceTarget::keep_past_close`, ADR-0305 §9), the window is ordered
    /// out and its view, and so the view's Metal layer, outlives the window's
    /// own release.
    fn clear_then_keep_if_kept(&self) {
        // SAFETY: `self.view` is the window's content view, which the window
        // holds while it lives.
        let view = unsafe { super::MainRetainedView::retain(self.view_ptr()) };
        super::clear_then_keep_if_kept(&self.delivery, view, |view| {
            // The view's window is gone once it answers none.
            if let Some(window) = view.downcast_ref::<NSView>().and_then(NSView::window) {
                window.orderOut(None);
            }
        });
    }

    /// The window's published state for a caller that reads it
    /// (`PlatformWindow::state`), read from AppKit again first when something
    /// invalidated it. A revision the read publishes is announced at the
    /// owner's turn, never from the read.
    fn current_state(&self) -> WindowState {
        let _followed = ModeWaitFollowed(self);
        self.snapshot.read(&self.delivery, || self.sample_state())
    }

    /// AppKit reported a change, or a command was just carried out: read the
    /// window again, and raise `WindowHandler::state_changed` when that
    /// produced a new revision.
    fn publish_state(&self) {
        let _followed = ModeWaitFollowed(self);
        self.snapshot
            .publish_report(&self.delivery, || self.sample_state());
    }

    /// Read the window again for its state publisher, announcing nothing.
    fn read_state(&self) -> crate::owner::window_delivery::Publication {
        let _followed = ModeWaitFollowed(self);
        self.snapshot
            .refresh(&self.delivery, || self.sample_state())
    }

    /// Have the window read again at the instant the snapshot's wait for a
    /// mode change ends (`WindowStateCell::mode_deadline`), which nothing
    /// from AppKit marks: a step AppKit refused, and a step published under
    /// the mode the window left, are published as read then. Called wherever
    /// that instant may have moved: after a mode request, and after every
    /// read ([`ModeWaitFollowed`]), since one that finds the mode asked
    /// shortens the wait and one made at a transition's end finds it
    /// lengthened. One timer is live at a time ([`WaitEndRead`]), and it
    /// holds the window weakly.
    fn read_when_the_mode_wait_ends(&self) {
        let end = self.snapshot.mode_deadline();
        let mut read = self.wait_end_read.get();
        let delay = read.follow(end, std::time::Instant::now());
        self.wait_end_read.set(read);
        let (Some(end), Some(delay)) = (end, delay) else {
            return;
        };
        let window = Arc::clone(&self.redraw().window);
        crate::owner::apple::macos::exec_after_on_owner(
            crate::owner::apple::macos::owner_queue(),
            delay,
            move || {
                let Some(window) = window.upgrade() else {
                    return;
                };
                let mut read = window.wait_end_read.get();
                let live = read.fired(end);
                window.wait_end_read.set(read);
                if live {
                    window.publish_state();
                }
            },
        );
    }

    /// The window as AppKit reports it now; rectangles that describe no
    /// window cost the sample its geometry and nothing else.
    fn sample_state(&self) -> Sample {
        // With what the window holds: the grab it committed, and the level it
        // stands at outside full screen, which a full-screen window keeps
        // published while AppKit shows it at the normal level.
        let (display, normal, scale, occlusion_visible, heading, asked_frame, held) = {
            let state = self.state.borrow();
            (
                state.display,
                state.normal,
                state.scale_factor,
                state.occlusion_visible,
                state.sequence.heading(std::time::Instant::now()),
                state.frame,
                (state.cursor_grab, state.level, state.live_resize),
            )
        };
        let (grab, level, live_resize) = held;
        let hovered = self.delivery.hovered();
        {
            let frame = self.ns_window.frame();
            let content = self.ns_window.contentRectForFrameRect(frame);
            let flags = self.read_flags();
            let focused = self.ns_window.isKeyWindow();
            let min = self.ns_window.contentMinSize();
            let max = self.ns_window.contentMaxSize();
            let title = self.ns_window.title().to_string();
            let mask = self.ns_window.styleMask();
            let mode = mode_of(flags.mode_flags());
            let position = self.position_of_frame(frame, display);
            let geometry =
                WindowGeometry::try_new(position, Size::new(frame.size.width, frame.size.height))
                    .and_then(|outer| {
                        Ok(SampledGeometry {
                            outer,
                            content: flui_platform::WindowExtent::try_new(Size::new(
                                content.size.width,
                                content.size.height,
                            ))?,
                            normal: if mode == WindowMode::Normal {
                                None
                            } else {
                                normal
                            },
                        })
                    });
            // The window's appearance, not NSApp's: a window may override it.
            let appearance = content_view::effective_appearance(&**self.ns_window);
            let capabilities = capabilities_of(
                flags,
                heading,
                appkit_window_capabilities(self.presentation),
            );
            let insets = self.insets_of_content(content);
            Sample::read(geometry, |state| {
                let state = state
                    .with_mode(mode)
                    .with_visible(flags.is_shown())
                    .with_title(Some(title))
                    .with_resizable(flags.resizable)
                    .with_frame(reported_frame(
                        mask.contains(NSWindowStyleMask::Titled),
                        asked_frame,
                    ))
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
                    brightness: appearance,
                    insets,
                    // AppKit never suspends a window.
                    execution: flui_platform::WindowExecutionState::Running,
                    // Covered or on another space.
                    occluded: !occlusion_visible,
                    hovered,
                    // The key window is the one this host reports active;
                    // the main window has no report of its own here.
                    focus: flui_platform::WindowFocus::reported(focused, focused),
                    capabilities,
                    // A live resize is AppKit's to report; its title-bar drag
                    // is not read yet (docs/gaps.md), so outside a resize the
                    // host cannot tell.
                    interaction: if live_resize {
                        flui_platform::WindowInteraction::UserResizing
                    } else {
                        flui_platform::WindowInteraction::Unknown
                    },
                    cursor_grab: grab,
                    level: level_of(level),
                }
                .onto(state)
            })
        }
    }

    /// The insets of a window whose content AppKit reports at `content`: the
    /// part of it under the camera housing of the screen it is on
    /// ([`safe_area_on_screen`]), which is its whole safe area; a Mac has no
    /// soft keyboard and no gesture edges.
    ///
    /// Read from the screen at every sample, with the content rectangle it is
    /// measured against: the window is sampled when it is resized or moved,
    /// changes screen or backing scale, and ends a full-screen transition,
    /// which are the changes that move it under the housing or out from under
    /// it. No inset before macOS 12, which reports none, nor for a window on
    /// no screen.
    fn insets_of_content(&self, content: NSRect) -> flui_platform::WindowInsets {
        let Some(reports_safe_area) = self
            .displays
            .get()
            .and_then(|displays| displays.version().since::<ScreenSafeArea>())
        else {
            return flui_platform::WindowInsets::ZERO;
        };
        let Some(screen) = self.ns_window.screen() else {
            return flui_platform::WindowInsets::ZERO;
        };
        let safe_area = safe_area_on_screen(
            crate::owner::apple::macos::bounds_of(content),
            crate::owner::apple::macos::bounds_of(screen.frame()),
            unsafe_edges_of(&screen, reports_safe_area),
        );
        flui_platform::WindowInsets::ZERO
            .try_with_safe_area(safe_area)
            .expect("BUG: `safe_area_on_screen` answers lengths that are finite and not negative")
    }

    /// What AppKit says about the window's mode, as flags.
    fn read_flags(&self) -> WindowFlags {
        let mask = self.ns_window.styleMask();
        WindowFlags {
            on_screen: self.ns_window.isVisible(),
            fullscreen: mask.contains(NSWindowStyleMask::FullScreen),
            miniaturized: self.ns_window.isMiniaturized(),
            zoomed: self.ns_window.isZoomed(),
            resizable: mask.contains(NSWindowStyleMask::Resizable),
        }
    }

    /// The top-left of a window whose frame AppKit reports as `frame`, on
    /// `display`, the screen the window is on: its point in AppKit's flipped
    /// desktop less the screen's corner. `None` where the window is on no
    /// screen the published snapshot holds, or a number is not finite.
    fn position_of_frame(
        &self,
        frame: NSRect,
        display: Option<DisplayId>,
    ) -> Option<WindowPosition> {
        let snapshot = self.displays.get()?.published();
        let screen = snapshot.get(display?)?;
        let primary_height = primary_screen_height(self.main)?;
        let desktop =
            top_left_of_frame(crate::owner::apple::macos::bounds_of(frame), primary_height)?;
        position_on(desktop, screen)
    }

    /// The window's outer rectangle now, its position on the screen it is
    /// on; `None` for a frame that describes no window.
    fn outer_geometry(&self) -> Option<WindowGeometry> {
        let display = self.state.borrow().display;
        let frame = self.ns_window.frame();
        WindowGeometry::try_new(
            self.position_of_frame(frame, display),
            Size::new(frame.size.width, frame.size.height),
        )
        .ok()
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
        if is_normal(self.read_flags())
            && let Some(outer) = self.outer_geometry()
        {
            self.state.borrow_mut().normal = Some(outer);
        }
    }

    fn set_native_title(&self, title: &str) {
        self.ns_window.setTitle(&NSString::from_str(title));
    }

    /// Move the window's outer top-left corner to `at`, a point of a screen in
    /// its points from its corner, whichever screen the window is on now.
    ///
    /// # Errors
    /// [`ApplyError::DisplayGone`] for a screen the published snapshot no
    /// longer holds; [`ApplyError::Platform`] for a point past the finite
    /// numbers or no primary screen to measure from.
    fn set_native_position(&self, at: WindowPosition) -> Result<(), ApplyError> {
        let snapshot = self
            .displays
            .get()
            .map_or_else(flui_platform::DisplaySnapshot::empty, |displays| {
                displays.published()
            });
        let screen = snapshot
            .get(at.display())
            .ok_or(ApplyError::DisplayGone(at.display()))?;
        let desktop = desktop_point(at, screen).ok_or(ApplyError::Platform(
            flui_platform::PlatformFailure::OutOfRange,
        ))?;
        let height = primary_screen_height(self.main).ok_or_else(|| {
            ApplyError::failed("there is no screen to measure a position from".into())
        })?;
        let point = cocoa_top_left_point(desktop, height);
        self.ns_window
            .setFrameTopLeftPoint(NSPoint::new(point.x, point.y));
        Ok(())
    }

    /// Resize the content area, not the frame: on decorated windows a
    /// frame-sized `setFrame:` would shrink the content by the title bar.
    /// AppKit takes `setContentSize:` as given, below the window's minimum
    /// too, so the size is first held to the window's limits, as a resize by
    /// the user is.
    fn set_content_size(&self, size: Size<f64>) {
        let min = self.ns_window.contentMinSize();
        let max = self.ns_window.contentMaxSize();
        let held = crate::window::state::held_to(
            size,
            constraints_of_content_limits(
                Size::new(min.width, min.height),
                Size::new(max.width, max.height),
            ),
        );
        self.ns_window
            .setContentSize(NSSize::new(held.width, held.height));
    }

    fn set_native_constraints(&self, constraints: SizeConstraints) {
        let (min, max) = content_limits_of_constraints(constraints);
        self.ns_window
            .setContentMinSize(NSSize::new(min.width, min.height));
        self.ns_window
            .setContentMaxSize(NSSize::new(max.width, max.height));
    }

    /// Switch one style-mask bit (resizable, titled) on or off, keeping the
    /// content area where it is: removing the title bar shrinks the frame, not
    /// the content.
    fn set_style_bit(&self, bit: NSWindowStyleMask, on: bool) -> Result<(), ApplyError> {
        let mask = self.ns_window.styleMask();
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
                let mut state = self.state.borrow_mut();
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
        self.restyle(mask, wanted);
        Ok(())
    }

    /// Give the window the style mask `wanted` in place of `mask`, keeping the
    /// content area where it is, and ask for a frame.
    fn restyle(&self, mask: NSWindowStyleMask, wanted: NSWindowStyleMask) {
        if wanted == mask {
            return;
        }
        let content = self
            .ns_window
            .contentRectForFrameRect(self.ns_window.frame());
        self.ns_window.setStyleMask(wanted);
        let restyled = self.ns_window.frameRectForContentRect(content);
        // `display: false`: `true` would draw a whole frame on this command's
        // stack, before the resize reaches the host. The frame is asked
        // for instead, so it comes on the next pass like any other.
        self.ns_window.setFrame_display(restyled, false);
        self.redraw().request_redraw();
    }

    /// Give the window `frame`: the system's title bar, one of the
    /// application's drawn under the system's made transparent
    /// (`fullSizeContentView`), or none, keeping the content area where it is,
    /// and lay the traffic lights out as the frame asks.
    fn set_frame(&self, frame: WindowFrame) -> Result<(), ApplyError> {
        let style = FrameStyle::of(frame);
        let mask = self.ns_window.styleMask();
        if mask.contains(NSWindowStyleMask::FullScreen) {
            return Err(ApplyError::failed(
                "AppKit does not restyle a window while it is full screen".into(),
            ));
        }
        let with = |mask: NSWindowStyleMask, bit, on: bool| {
            if on { mask | bit } else { mask - bit }
        };
        let wanted = with(
            with(mask, NSWindowStyleMask::Titled, style.titled),
            NSWindowStyleMask::FullSizeContentView,
            style.app_title_bar,
        );
        self.state.borrow_mut().frame = frame;
        self.restyle(mask, wanted);
        style_title_bar(&self.ns_window, style.app_title_bar);
        self.place_system_controls();
        Ok(())
    }

    /// Lay the traffic lights out as the window's frame asks: hidden or moved
    /// over a title bar of the application's, where the system puts them
    /// otherwise. A window without a title bar has none.
    ///
    /// AppKit lays the buttons out from the bottom edge of the title bar's
    /// container, so moving them down grows the container
    /// ([`ControlsLayout::moved`]); it lays them out again when the window
    /// resizes or leaves full screen, so `windowDidResize:` asks this again.
    fn place_system_controls(&self) {
        /// Close, miniaturize, zoom.
        const BUTTONS: [NSWindowButton; 3] = [
            NSWindowButton::CloseButton,
            NSWindowButton::MiniaturizeButton,
            NSWindowButton::ZoomButton,
        ];
        let mask = self.ns_window.styleMask();
        if !mask.contains(NSWindowStyleMask::Titled) || mask.contains(NSWindowStyleMask::FullScreen)
        {
            return;
        }
        // `standardWindowButton:` answers a button the window owns or none;
        // its superview is the title bar view and that one's superview the
        // title bar's container, both the window's for as long as it is
        // titled.
        let found = BUTTONS.map(|kind| self.ns_window.standardWindowButton(kind));
        let [Some(close), Some(miniaturize), Some(zoom)] = found else {
            return;
        };
        let buttons = [close, miniaturize, zoom];
        // SAFETY: `superview` is not retained by the view's own answer, so it
        // must still be alive when the call retains it: the button is held by
        // the title bar view, which the window's frame view holds, and
        // nothing between the two reads reorders the hierarchy, on the main
        // thread.
        let bar = unsafe { buttons[0].superview() };
        // SAFETY: as above, for the title bar view's own superview.
        let container = bar.and_then(|bar| unsafe { bar.superview() });
        let Some(container) = container else {
            return;
        };
        // Read before the state is borrowed: no borrow is held across an
        // AppKit message.
        let unread = self.state.borrow().system_controls.is_none();
        let read = unread.then(|| ControlsLayout {
            bar_height: container.frame().size.height,
            buttons: buttons.each_ref().map(|button| {
                let frame = button.frame();
                Point::new(frame.origin.x, frame.origin.y)
            }),
        });
        let (system, frame) = {
            let mut state = self.state.borrow_mut();
            // The first read the window had stays: AppKit's layout before
            // anything moved the traffic lights. Only this keeps one, and it
            // read one above whenever none was kept.
            let system = *state.system_controls.get_or_insert_with(|| {
                read.expect("BUG: the traffic lights' layout is read whenever none is kept")
            });
            (system, state.frame)
        };
        let direction = self.ns_window.windowTitlebarLayoutDirection();
        let controls = FrameStyle::of(frame)
            .controls
            .unwrap_or(flui_platform::SystemControls::Shown);
        let layout = system.for_controls(
            controls,
            direction == NSUserInterfaceLayoutDirection::RightToLeft,
        );
        for button in &buttons {
            button.setHidden(layout.is_none());
        }
        let Some(layout) = layout else {
            return;
        };
        let window = self.ns_window.frame();
        let mut bar = container.frame();
        bar.origin.y = layout.bar_bottom(window.size.height);
        bar.size.height = layout.bar_height;
        container.setFrame(bar);
        for (button, origin) in buttons.iter().zip(layout.buttons) {
            button.setFrameOrigin(NSPoint::new(origin.x, origin.y));
        }
    }

    /// Order the window out. A full-screen window is refused: ordering it out
    /// leaves its empty Space behind.
    fn hide(&self) -> Result<(), ApplyError> {
        if self.read_flags().fullscreen {
            return Err(ApplyError::failed(
                "a full-screen window leaves full screen before it is hidden".into(),
            ));
        }
        self.ns_window.orderOut(None);
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
        crate::owner::apple::macos::exec_after_on_owner(
            crate::owner::apple::macos::owner_queue(),
            ANIMATION_LIMIT,
            move || {
                let Some(window) = window.upgrade() else {
                    return;
                };
                let gave_up = window
                    .state
                    .borrow_mut()
                    .sequence
                    .expire(std::time::Instant::now());
                if gave_up {
                    window.publish_state();
                }
            },
        );
    }

    /// The window asked AppKit for `asked`, or AppKit started a transition
    /// that lands there (full screen, a zoom): the snapshot keeps the mode
    /// the window left until a read finds `asked`, for at most `limit`
    /// (`WindowStateCell::expect_mode`), and the window is read again when
    /// the wait ends, so a step AppKit refused is published then. Every
    /// request that changes a mode calls it, one for the mode the window is
    /// in too, so a newer request replaces the wait of an older one.
    fn wait_out_mode(&self, asked: WindowMode, limit: std::time::Duration) {
        self.snapshot
            .expect_mode(asked, std::time::Instant::now() + limit);
        self.read_when_the_mode_wait_ends();
    }

    /// Carry out `command`, one that gives the window a frame of its own (a
    /// position, a size, limits, a style). AppKit reads zoomed off the
    /// frame, so such a command can take the window out of its zoom, and
    /// AppKit carries it out at once: a command that left the window in
    /// another mode than it found it ends the wait of a mode request before
    /// it, since the window is where this command put it and nothing more is
    /// to come.
    fn reframe<T>(&self, command: impl FnOnce() -> T) -> T {
        let found_in = mode_of(self.read_flags().mode_flags());
        let outcome = command();
        if mode_of(self.read_flags().mode_flags()) != found_in {
            self.snapshot.end_mode_wait();
        }
        outcome
    }

    fn change_mode(&self, mode: WindowMode) -> Result<(), ApplyError> {
        let flags = self.read_flags();
        if !flags.resizable && matches!(mode, WindowMode::Maximized | WindowMode::Fullscreen) {
            return Err(ApplyError::failed(
                "AppKit does not zoom or full-screen a window that cannot be resized".into(),
            ));
        }
        let now = std::time::Instant::now();
        let (steps, heading) = {
            let mut state = self.state.borrow_mut();
            let steps = state.sequence.command(now, flags, mode);
            (steps, state.sequence.heading(now))
        };
        if heading.is_some() {
            self.read_again_when_the_step_gives_up();
            self.wait_out_mode(mode, ANIMATION_LIMIT);
        } else {
            self.wait_out_mode(mode, MODE_ANSWER_LIMIT);
        }
        if !steps.is_empty() {
            self.record_normal();
        }
        for step in &steps {
            // Each message is the AppKit request the step names.
            match step {
                ModeStep::Deminiaturize => self.ns_window.deminiaturize(None),
                ModeStep::Miniaturize => self.ns_window.miniaturize(None),
                ModeStep::Zoom | ModeStep::Unzoom => self.ns_window.zoom(None),
                ModeStep::EnterFullscreen | ModeStep::ExitFullscreen => {
                    self.ns_window.toggleFullScreen(None);
                }
            }
        }
        Ok(())
    }

    /// A window that opened hidden enters the mode it opened in now that it
    /// is shown.
    fn enter_mode_when_shown(&self) {
        let waiting = self.state.borrow_mut().sequence.take_when_shown();
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

    /// The level each command sets reads back as the level it asked for, and
    /// a level only the AppKit extension sets reads as the side it is on.
    #[test]
    fn the_level_appkit_holds_reads_back_as_the_snapshots() {
        use objc2_app_kit::{NSFloatingWindowLevel, NSNormalWindowLevel, NSStatusWindowLevel};
        for (level, expected) in [
            (NSNormalWindowLevel, WindowLevel::Normal),
            (NSFloatingWindowLevel, WindowLevel::AlwaysOnTop),
            (NSNormalWindowLevel - 1, WindowLevel::AlwaysOnBottom),
            (NSStatusWindowLevel, WindowLevel::AlwaysOnTop),
        ] {
            assert_eq!(level_of(level), expected, "{level}");
        }
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
