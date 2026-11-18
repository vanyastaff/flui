//! The native X11 host's windows: [`X11Window`], one top-level window on the
//! platform's connection, and [`X11Windows`], the platform's windows, which
//! the loop holds as its `LoopWindows` (`owner::linux::x11`).
//!
//! # What a window publishes
//!
//! A window's snapshot is what the server and the window manager hold, read
//! back, never what the window last asked for: its content's geometry and its
//! place on the root window, the frame around it (`_NET_FRAME_EXTENTS`), its
//! map state, its mode properties (`WM_STATE`, `_NET_WM_STATE`), the input
//! focus and the active window (`_NET_ACTIVE_WINDOW`), asked in one batch and
//! then answered. An event that may change any of them (a configure, a map or
//! unmap, a property, the focus, the visibility) only marks the window; the
//! loop has each marked window read itself once a turn, after the turn's
//! events (`LoopWindows::read_stale`), so a stream of events costs a window
//! one read a turn, and the handler hears `state_changed` for a new revision.
//! Between two reads a handler sees the last snapshot, unless it reads the
//! state itself. What a window honours follows what the window manager
//! supports (`support`), so a window on a server with no window manager
//! neither claims to minimize nor waits for a maximize nobody carries out.
//!
//! # A window manager that goes
//!
//! A window manager that speaks EWMH names a window of its own in
//! `_NET_SUPPORTING_WM_CHECK`; one that speaks only ICCCM owns the screen's
//! `WM_S<n>` selection (ICCCM §2.8), as every window manager does, and keeps
//! `WM_STATE` on the windows it manages. The platform hears the end of that
//! window, or of the selection owner's (`StructureNotify` on it, as GDK
//! selects it in `fetch_net_wm_check_window`): a window manager that was
//! killed or crashed changes no property of the root window, but the server
//! destroys its windows. What it supported is read again then, and every
//! window with it, as it is when a window manager announces itself (the
//! `MANAGER` message to the root). Without a window manager the mode and
//! frame properties on a window are a dead one's
//! (`WindowManagerSupport::kept`): the server maps the windows it had
//! iconified, so a window reads normal, with no frame, and is focused by the
//! server directly. A window manager that answers no
//! `_NET_SUPPORTING_WM_CHECK` still runs: its `WM_STATE` stands and it
//! iconifies, and only what EWMH would say is unknown.
//!
//! # Commands
//!
//! The server carries out the title, the place, the size, the limits, the
//! resize steps and the proportion (`WM_NORMAL_HINTS`), the decorations
//! (`_MOTIF_WM_HINTS`), showing and hiding (mapping and withdrawing); the
//! window manager the modes and the level, asked with the EWMH client
//! messages, or, for a window it does not manage yet (one not mapped), in the
//! window's own `_NET_WM_STATE`, which it reads when it maps the window. A
//! window opened in a mode is mapped only once that property holds it, so it
//! never appears in normal mode first. The cursor and a hold of the pointer
//! are the input's (`LoopInput`), which lets a hold go when the window loses
//! the focus, is unmapped or closes.
//!
//! # Frames
//!
//! A frame request from the owner thread or from any other through the
//! window's redraw requester marks the window and rings the loop's doorbell;
//! the loop delivers the frame on its next turn. An `Expose` asks for one, as
//! does a change of the content size.
//!
//! # What the window manager asks
//!
//! Beside its close, a window lists `_NET_WM_PING` and, where the server has
//! the SYNC extension, `_NET_WM_SYNC_REQUEST` in `WM_PROTOCOLS`, and names its
//! machine and process (`WM_CLIENT_MACHINE`, and `_NET_WM_PID` where a window
//! manager's id for the process is the same, `protocols::ClientProcess`). A
//! ping goes back to the root, and is sent, as the event loop hears it on the
//! owner thread: a window whose owner hangs answers nothing, and its window
//! manager can offer to end it, as a Wayland compositor can by
//! `xdg_wm_base.ping`. A synchronised resize is answered on the window's XSync
//! counters (`protocols::ResizeSync`): at the turn's read when the configure
//! asks no frame, otherwise once the handler of the frame the loop delivers
//! after it returns. That is when the frame has been handed to the GPU, not
//! when it is shown: the host hears no presentation, and a FIFO swap chain
//! presents at a later refresh (a raster thread may still be drawing), so a
//! window manager may draw its frame once around content of the old size.
//!
//! # Close
//!
//! The window manager's close (`WM_DELETE_WINDOW`) asks the handler, which may
//! keep the window open; a [`WindowCommand::Close`] does not ask. Either way
//! the window is withdrawn at once, the handler hears `closed`, the window
//! leaves the platform's table, and the X window is destroyed only once the
//! handler, and with it a renderer whose surface names the window, is let go
//! of. A window destroyed by someone else is closed the same way, with
//! nothing left to destroy.
//!
//! # Identity
//!
//! The server names a window by its X id, which it may give another window
//! once this one is destroyed; the platform names it by a [`WindowId`] it
//! never gives twice. The X id routes an event to the window that holds it
//! now, and is forgotten as the window closes, before the X window is
//! destroyed; everything else names the window by its identity.

mod manager_hints;

use std::cell::{Cell, RefCell};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use flui_foundation::geometry::{Bounds, DevicePixelRatio, Point, Size, WindowMetrics};
use flui_foundation::panic::FirstPanic;
use flui_platform::input_regions::ResizeEdge;
use flui_platform::{
    AdmittedCommand, ApplyError, AspectRatio, Brightness, CloseDecision, Cursor, CursorError,
    CursorGrab, DisplaySnapshot, PlatformTextInput, PlatformWindow, SizeConstraints, Unsupported,
    UnsupportedReason, WindowCapabilities, WindowCommand, WindowExtent, WindowFrame,
    WindowGeometry, WindowHandler, WindowId, WindowIdMint, WindowLevel, WindowMode, WindowOptions,
    WindowPosition, WindowState,
};
use flui_semantics::platform::PlatformAccessibility;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle, XcbDisplayHandle, XcbWindowHandle,
};
use rustc_hash::FxHashMap;
use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::sync::{self as xsync, ConnectionExt as _};
use x11rb::protocol::xproto::{
    self, AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConfigureWindowAux,
    ConnectionExt as _, CreateWindowAux, EventMask, InputFocus, MapState, PropMode, StackMode,
    UnmapNotifyEvent, Visibility, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

use super::hints::WM_HINTS;
use super::mode::{
    LevelAtoms, NetWmState, NetWmStateAtoms, WmState, level_of, mode_flags, with_mode,
};
use super::placement::{
    FrameExtents, device_extent, display_of, logical_geometry, outer_bounds, window_scale,
    x11_coordinates,
};
use super::protocols::{ClientProcess, CounterWrites, NameEncoding, ResizeSync, SyncRequest};
use super::support::{SupportAtoms, WindowManager, WindowManagerSupport, capabilities};
use crate::host::{self, ScreenBounds, WindowFacets, WindowPositions};
use crate::host_window::HostWindow;
use crate::input::regions::ProjectedInputRegions;
use crate::input::regions::presses::{
    ChromePress, ChromePresses, PressButton, ResizeBand, device_point,
};
use crate::owner::OpenWindowError;
use crate::owner::OpenWindows;
use crate::owner::linux::x11::{
    Doorbell, HostDisplays, InputTarget, LoopDrag, LoopInput, LoopText, LoopWindows, PeerHeard,
    PeerInterest, PeerSelect, WindowRef, WindowScale, X11Connection, X11Loop,
};
use crate::owner::slot::OwnerWeak;
use crate::owner::wake_deadline::FrameCount;
use crate::owner::window_delivery::{NativeFate, Publication, Retirement, WindowDelivery};
use crate::text::WindowTextInput;
use crate::window::desktop::DisplayFrame;
use crate::window::linux::class::AppClass;
use crate::window::plan::{FrameInset, OpenPlan};
use crate::window::state::{
    ReportedFacts, Sample, SampledGeometry, WindowStateCell, mode_of, not_carried_out,
};

/// `_NET_WM_STATE` and `WM_STATE` are read this far, in 4-byte units.
const PROPERTY_READ_LENGTH: u32 = 64;

/// How long a window opened at a remembered outer rectangle waits for the
/// window manager to say what frame it will have.
const FRAME_EXTENTS_WAIT: Duration = Duration::from_millis(50);

/// `WM_STATE`'s iconic state, which `WM_CHANGE_STATE` asks a window manager
/// for (ICCCM 4.1.4).
const ICONIC_STATE: u32 = 3;

/// The source of a `_NET_WM_STATE` or `_NET_ACTIVE_WINDOW` request: an
/// application (EWMH).
const SOURCE_APPLICATION: u32 = 1;

/// What a `_NET_WM_STATE` request does to the states it names (EWMH).
#[derive(Clone, Copy)]
enum StateAction {
    Remove = 0,
    Add = 1,
}

/// The atoms the windows read and write, as the server numbered them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WindowAtoms {
    wm_protocols: xproto::Atom,
    wm_delete_window: xproto::Atom,
    wm_state: xproto::Atom,
    wm_change_state: xproto::Atom,
    net_wm_name: xproto::Atom,
    utf8_string: xproto::Atom,
    net_wm_pid: xproto::Atom,
    net_wm_state: xproto::Atom,
    states: NetWmStateAtoms,
    /// The `_NET_WM_STATE` atoms a window's level is kept by.
    levels: LevelAtoms,
    net_active_window: xproto::Atom,
    net_frame_extents: xproto::Atom,
    net_request_frame_extents: xproto::Atom,
    net_supported: xproto::Atom,
    net_supporting_wm_check: xproto::Atom,
    net_wm_window_type: xproto::Atom,
    net_wm_window_type_normal: xproto::Atom,
    motif_wm_hints: xproto::Atom,
    /// The application id a window claims (GTK's, which shells read beside
    /// `WM_CLASS`).
    gtk_application_id: xproto::Atom,
    /// The request a window manager moves or resizes a window by, from a press
    /// on a title bar or border the application draws.
    net_wm_moveresize: xproto::Atom,
    /// The screen's `WM_S<n>` selection, which a window manager owns while it
    /// runs (ICCCM §2.8).
    wm_selection: xproto::Atom,
    /// The screen's `_NET_WM_CM_S<n>` selection, which a compositing manager
    /// owns while it runs (EWMH).
    cm_selection: xproto::Atom,
    /// The message a window manager that took the selection sends the root.
    manager: xproto::Atom,
    /// The protocol a window manager asks whether the window's process still
    /// answers by (EWMH).
    net_wm_ping: xproto::Atom,
    /// The protocol a window manager asks for a synchronised resize by.
    net_wm_sync_request: xproto::Atom,
    /// The property naming the counters a synchronised resize waits on.
    net_wm_sync_request_counter: xproto::Atom,
    /// What a window manager that synchronises frames lists in
    /// `_NET_SUPPORTED`.
    net_wm_frame_drawn: xproto::Atom,
}

impl WindowAtoms {
    /// Intern the atoms on `connection`: every request first, then every
    /// reply.
    ///
    /// # Errors
    /// [`PlatformError::Init`](crate::PlatformError::Init) when the server
    /// does not answer.
    pub(crate) fn intern(connection: &X11Connection) -> Result<Self, crate::PlatformError> {
        const NAMES: [&[u8]; 29] = [
            b"WM_PROTOCOLS",
            b"WM_DELETE_WINDOW",
            b"WM_STATE",
            b"WM_CHANGE_STATE",
            b"_NET_WM_NAME",
            b"UTF8_STRING",
            b"_NET_WM_PID",
            b"_NET_WM_STATE",
            b"_NET_WM_STATE_HIDDEN",
            b"_NET_WM_STATE_MAXIMIZED_HORZ",
            b"_NET_WM_STATE_MAXIMIZED_VERT",
            b"_NET_WM_STATE_FULLSCREEN",
            b"_NET_WM_STATE_ABOVE",
            b"_NET_WM_STATE_BELOW",
            b"_NET_ACTIVE_WINDOW",
            b"_NET_FRAME_EXTENTS",
            b"_NET_REQUEST_FRAME_EXTENTS",
            b"_NET_SUPPORTED",
            b"_NET_SUPPORTING_WM_CHECK",
            b"_NET_WM_WINDOW_TYPE",
            b"_NET_WM_WINDOW_TYPE_NORMAL",
            b"_MOTIF_WM_HINTS",
            b"_GTK_APPLICATION_ID",
            b"_NET_WM_MOVERESIZE",
            b"MANAGER",
            b"_NET_WM_PING",
            b"_NET_WM_SYNC_REQUEST",
            b"_NET_WM_SYNC_REQUEST_COUNTER",
            b"_NET_WM_FRAME_DRAWN",
        ];
        let failed = |error: &dyn std::fmt::Display| crate::PlatformError::Init {
            message: format!("cannot intern the X11 window atoms: {error}"),
        };
        let xcb = connection.xcb();
        let cookies = NAMES
            .iter()
            .map(|name| xcb.intern_atom(false, name))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| failed(&error))?;
        let selection_name = format!("WM_S{}", connection.screen_number());
        let selection = xcb
            .intern_atom(false, selection_name.as_bytes())
            .map_err(|error| failed(&error))?;
        let compositing_name = format!("_NET_WM_CM_S{}", connection.screen_number());
        let compositing = xcb
            .intern_atom(false, compositing_name.as_bytes())
            .map_err(|error| failed(&error))?;
        let mut atoms = [0; NAMES.len()];
        for (atom, cookie) in atoms.iter_mut().zip(cookies) {
            *atom = cookie.reply().map_err(|error| failed(&error))?.atom;
        }
        let wm_selection = selection.reply().map_err(|error| failed(&error))?.atom;
        let cm_selection = compositing.reply().map_err(|error| failed(&error))?.atom;
        let [
            wm_protocols,
            wm_delete_window,
            wm_state,
            wm_change_state,
            net_wm_name,
            utf8_string,
            net_wm_pid,
            net_wm_state,
            hidden,
            maximized_horz,
            maximized_vert,
            fullscreen,
            above,
            below,
            net_active_window,
            net_frame_extents,
            net_request_frame_extents,
            net_supported,
            net_supporting_wm_check,
            net_wm_window_type,
            net_wm_window_type_normal,
            motif_wm_hints,
            gtk_application_id,
            net_wm_moveresize,
            manager,
            net_wm_ping,
            net_wm_sync_request,
            net_wm_sync_request_counter,
            net_wm_frame_drawn,
        ] = atoms;
        Ok(Self {
            wm_protocols,
            wm_delete_window,
            wm_state,
            wm_change_state,
            net_wm_name,
            utf8_string,
            net_wm_pid,
            net_wm_state,
            states: NetWmStateAtoms {
                hidden,
                maximized_horz,
                maximized_vert,
                fullscreen,
            },
            levels: LevelAtoms { above, below },
            net_active_window,
            net_frame_extents,
            net_request_frame_extents,
            net_supported,
            net_supporting_wm_check,
            net_wm_window_type,
            net_wm_window_type_normal,
            motif_wm_hints,
            gtk_application_id,
            net_wm_moveresize,
            wm_selection,
            cm_selection,
            manager,
            net_wm_ping,
            net_wm_sync_request,
            net_wm_sync_request_counter,
            net_wm_frame_drawn,
        })
    }

    /// The `_NET_SUPPORTED` atoms the window's capabilities read.
    fn support(&self) -> SupportAtoms {
        SupportAtoms {
            maximized_horz: self.states.maximized_horz,
            maximized_vert: self.states.maximized_vert,
            fullscreen: self.states.fullscreen,
            moveresize: self.net_wm_moveresize,
            above: self.levels.above,
            below: self.levels.below,
            frame_drawn: self.net_wm_frame_drawn,
        }
    }

    /// Whether a change of property `atom` on a window can change its
    /// snapshot.
    fn concerns_window(&self, atom: xproto::Atom) -> bool {
        atom == self.net_wm_state || atom == self.wm_state || atom == self.net_frame_extents
    }

    /// Whether a change of property `atom` on the root window can change what
    /// the window manager supports.
    fn concerns_support(&self, atom: xproto::Atom) -> bool {
        atom == self.net_supported || atom == self.net_supporting_wm_check
    }
}

/// What a press on a window's chrome is answered from (ADR-0286 §10).
#[derive(Debug, Default)]
struct Chrome {
    /// The regions of the last composited frame, in device pixels.
    projected: ProjectedInputRegions,
    /// The last press on the bar, and one armed to move the window.
    presses: ChromePresses,
    /// The button of the press the window took, whose release it takes too.
    held: Option<u32>,
}

/// The two XSync counters a window names in `_NET_WM_SYNC_REQUEST_COUNTER`,
/// in that order: the one a window manager without frame synchronisation
/// waits on, and the extended one (`protocols`).
#[derive(Debug, Clone, Copy)]
struct SyncCounters {
    basic: xsync::Counter,
    extended: xsync::Counter,
}

impl SyncCounters {
    /// Two counters made on `xcb` at 0, or `None` when the connection has no
    /// ids left or failed. Both ids are taken before either counter is made,
    /// and the first is destroyed again when the second cannot be: none is
    /// left on the server for nobody.
    fn create(xcb: &x11rb::xcb_ffi::XCBConnection) -> Option<Self> {
        let zero = xsync::Int64 { hi: 0, lo: 0 };
        let basic = xcb.generate_id().ok()?;
        let extended = xcb.generate_id().ok()?;
        xcb.sync_create_counter(basic, zero).ok()?;
        if xcb.sync_create_counter(extended, zero).is_err() {
            let _: Result<(), _> = xcb
                .sync_destroy_counter(basic)
                .map(x11rb::cookie::VoidCookie::ignore_error);
            return None;
        }
        Some(Self { basic, extended })
    }
}

/// The SYNC version a connection asks for: 3.1, what libXext asks for GDK and
/// Qt. The counters a window makes are in every version.
const SYNC_VERSION: (u8, u8) = (3, 1);

/// Initialise the SYNC extension on `xcb`, as the extension asks before any
/// other of its requests: whether the server has it and answered.
fn initialize_sync(xcb: &x11rb::xcb_ffi::XCBConnection) -> bool {
    use x11rb::connection::RequestConnection as _;

    let present = matches!(
        xcb.extension_information(xsync::X11_EXTENSION_NAME),
        Ok(Some(_))
    );
    present
        && xcb
            .sync_initialize(SYNC_VERSION.0, SYNC_VERSION.1)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some()
}

/// The `_NET_WM_MOVERESIZE` direction that moves a window.
const MOVERESIZE_MOVE: u32 = 8;

/// The `_NET_WM_MOVERESIZE` direction that resizes a window from `edge`, `None`
/// for an edge it names none for.
const fn moveresize_direction(edge: ResizeEdge) -> Option<u32> {
    Some(match edge {
        ResizeEdge::TopLeft => 0,
        ResizeEdge::Top => 1,
        ResizeEdge::TopRight => 2,
        ResizeEdge::Right => 3,
        ResizeEdge::BottomRight => 4,
        ResizeEdge::Bottom => 5,
        ResizeEdge::BottomLeft => 6,
        ResizeEdge::Left => 7,
        _ => return None,
    })
}

/// An XInput 2 fixed-point coordinate.
fn fp1616(value: i32) -> f64 {
    f64::from(value) / 65536.0
}

/// What a window keeps of what it was asked, which the server does not hold
/// in a form it reads back.
#[derive(Debug)]
struct Record {
    title: String,
    constraints: SizeConstraints,
    resizable: bool,
    /// The frame the window was last asked for: the window manager's
    /// decorations are on for the system's alone.
    frame: WindowFrame,
    /// The application wants the window shown: mapped, or waiting for its
    /// first frame to be.
    shown: bool,
    /// The window waits unmapped for its first frame.
    reveal_pending: bool,
    /// The window was asked to minimize while unmapped: it is, once mapped.
    minimize_on_map: bool,
    /// The outer size a window opened at a remembered rectangle has to end up
    /// with once its frame is known; taken when it is first mapped.
    outer_to_fit: Option<Size<f64>>,
    /// The last visibility the server reported: fully covered.
    obscured: bool,
    /// The rectangle the window returns to from another mode: the outer
    /// geometry it had the last time it was in normal mode.
    normal_geometry: Option<WindowGeometry>,
    /// The content size last published, which a frame follows the change of.
    content: Option<Size<i32>>,
    /// The focus the accessibility bridge was last told of.
    focus_told: bool,
    /// Where the window was on the screen at the last read of the server.
    bounds: Option<ScreenBounds>,
    /// Where the accessibility bridge was last told the window is.
    bounds_told: Option<ScreenBounds>,
    /// The window manager's support last read, kept for a window whose
    /// platform is gone.
    support: WindowManagerSupport,
    /// The scale of the display the window was last read on, or opened on.
    scale: DevicePixelRatio,
    /// The window's size hints are device pixels at a scale it has left:
    /// the next read writes them again at the new one.
    hints_rescaled: bool,
    /// The steps the user's resize takes, in logical pixels
    /// (`WM_NORMAL_HINTS`).
    resize_increments: Option<WindowExtent>,
    /// The proportion the user's resize keeps (`WM_NORMAL_HINTS`).
    aspect_ratio: Option<AspectRatio>,
}

/// One X11 window of the platform's connection.
///
/// Lives on the owner thread (ADR-0254 §5): the thread that presents its
/// frames reaches it through its [`SurfaceTarget`](flui_platform::SurfaceTarget)
/// and another thread asks it for a frame through its redraw requester.
pub struct X11Window {
    id: WindowId,
    xid: xproto::Window,
    connection: Arc<X11Connection>,
    atoms: WindowAtoms,
    /// The platform's windows, which hold this one until it closes.
    windows: Weak<X11Windows>,
    delivery: Arc<WindowDelivery>,
    /// What the table's positions made for the window: its AT-SPI bridge, its
    /// text input, and the haptics and chrome X11 does not have.
    facets: WindowFacets,
    cell: Arc<WindowStateCell>,
    /// A frame was asked for and not delivered yet.
    redraw: Arc<AtomicBool>,
    doorbell: Doorbell,
    displays: Arc<dyn HostDisplays>,
    /// The windows' input, which delivers this window's pointers and keys
    /// and shows its cursor.
    input: Rc<dyn LoopInput>,
    /// The windows' drag and drop, which offers this window the drags over
    /// it.
    drag: Arc<dyn LoopDrag>,
    /// The windows' text input, which serves this window's field to the
    /// input method and hands it the window's keys.
    text: Rc<dyn LoopText>,
    /// The scale the input reads this window's positions at.
    input_scale: WindowScale,
    record: RefCell<Record>,
    /// The chrome regions a press is answered from, and the window's presses
    /// on them (ADR-0286 §10).
    chrome: RefCell<Chrome>,
    /// The counters a synchronised resize waits on, where the server has the
    /// SYNC extension.
    counters: Option<SyncCounters>,
    /// When those counters are set; never borrowed across code the window
    /// does not own.
    resize_sync: RefCell<ResizeSync>,
    /// An event said the window may differ from its snapshot, and the loop
    /// has not had it read itself since.
    unread: Cell<bool>,
    /// A revision was published with nobody told: a command read the window
    /// it was half way through changing, where no handler may run. The next
    /// read that may announce tells the handler.
    unannounced: Cell<bool>,
    closed: Cell<bool>,
    /// Where the X window is in its end.
    native: Cell<NativeWindow>,
}

/// Where an X window is in its end. Letting go of the handler is owed in
/// every state but [`Released`](Self::Released): a window the server destroyed
/// still holds a handler, and a renderer whose surface names a window that is
/// gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeWindow {
    /// On the server, and the handler held.
    Live,
    /// Destroyed by another client (`DestroyNotify`); the handler still held.
    DestroyedByServer,
    /// The handler's release is scheduled (`WindowDelivery::clear_then`),
    /// and with it the X window's destruction where the server still has it.
    Released,
}

impl std::fmt::Debug for X11Window {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("X11Window")
            .field("id", &self.id)
            .field("xid", &self.xid)
            .field("closed", &self.closed.get())
            .finish_non_exhaustive()
    }
}

impl X11Window {
    fn xcb(&self) -> &x11rb::xcb_ffi::XCBConnection {
        self.connection.xcb()
    }

    /// The window manager's support, read by the platform, or the last read
    /// once the platform is gone.
    fn support(&self) -> WindowManagerSupport {
        match self.windows.upgrade() {
            Some(windows) => windows.support(),
            None => self.record.borrow().support,
        }
    }

    /// The colour scheme the system applies, as the platform last heard it.
    fn appearance(&self) -> Brightness {
        self.windows
            .upgrade()
            .map_or(Brightness::Light, |windows| windows.appearance.get())
    }

    /// The display snapshot, or none when it cannot be read.
    fn display_snapshot(&self) -> Option<DisplaySnapshot> {
        self.displays.snapshot().ok()
    }

    /// The focus the window last told its accessibility bridge of.
    #[cfg(test)]
    pub(crate) fn told_focus(&self) -> bool {
        self.record.borrow().focus_told
    }

    /// What the window's state publisher does (a hover the input reports):
    /// the window is marked like for any event of the server's, and the
    /// loop's read this turn publishes and announces the revision. A pointer
    /// crossing the window's edge costs no read of its own.
    fn publish_at_the_turns_read(&self) -> Publication {
        self.mark_stale();
        Publication::Held
    }

    /// The window a command acts on: as it is now, read from the server if
    /// an event marked the window since its last snapshot. A command that
    /// went by the last snapshot would act on what the window was a turn
    /// ago (leave an iconic window unmapped, say, or ask a window manager
    /// that is gone for a mode).
    ///
    /// Nobody is told of a revision this read publishes: the command is half
    /// way through, with its own state not committed yet, and a handler that
    /// ran here could command the window again under it. The revision is
    /// announced by the next read that may ([`PlatformWindow::state`]): the
    /// command's own at its end, or the next turn's, which the read asks for
    /// so that a command that ends early (refused, failed, unwound) leaves
    /// nobody waiting for something else to wake the loop.
    fn state_now(&self) -> WindowState {
        let published = self.cell.read_unannounced(&self.delivery, || self.sample());
        if published.changed {
            self.unannounced.set(true);
            self.owe_a_read();
            // Nothing is reported here, where a `tracing` subscriber would
            // run inside the command: a ring fails only with the loop's own
            // descriptor, and then no turn is left to announce anything.
            let _: Result<(), crate::PlatformError> = self.doorbell.ring();
        }
        published.state
    }

    /// The mode a command acts on: [`state_now`](Self::state_now)'s.
    fn mode_now(&self) -> WindowMode {
        self.state_now().mode
    }

    /// The loop has the window follow its state at the turn's read.
    fn owe_a_read(&self) {
        self.unread.set(true);
        if let Some(windows) = self.windows.upgrade() {
            windows.unread.set(true);
        }
    }

    /// An event of the server's says the window may differ from its last
    /// snapshot: the next read of its state asks the server, and the loop
    /// has the window follow it once this turn
    /// (`LoopWindows::read_stale`). No request is made here, so what a
    /// stream of events costs does not grow with it.
    fn mark_stale(&self) {
        self.cell.invalidate();
        self.owe_a_read();
    }

    /// The window differs from its last snapshot by something it just did
    /// itself (a command, its opening, its reveal): read it again now and
    /// follow what it reads.
    fn refresh_state(&self) {
        self.cell.invalidate();
        self.follow_state();
    }

    /// Follow the window's state: raise `state_changed` for a new revision,
    /// or for one a command published with nobody told, from the host's own
    /// turn; tell the bridge of a change of focus or of where the window is
    /// on the screen, and ask for a frame when the content changed size. The
    /// server is asked only if the snapshot is stale.
    fn follow_state(&self) {
        self.unread.set(false);
        let published = self.cell.read_unannounced(&self.delivery, || self.sample());
        let unannounced = self.unannounced.take();
        let state = published.state;
        // The window's own record and what follows from it come first, and
        // the handler, code the host does not own, last: a command the
        // handler applies follows the window again inside it, and must find
        // this read already committed, not have its newer focus put back by
        // this one afterwards.
        let (focus_changed, new_bounds, resized, rescaled) = {
            let mut record = self.record.borrow_mut();
            let focus_changed = std::mem::replace(&mut record.focus_told, state.focus.is_focused())
                != state.focus.is_focused();
            let bounds = record.bounds;
            let new_bounds =
                bounds.filter(|_| std::mem::replace(&mut record.bounds_told, bounds) != bounds);
            // A change of a size known before: the first one an `Expose`
            // paints.
            let content = state.metrics.map(WindowMetrics::physical);
            let resized =
                record.content.is_some() && content.is_some() && record.content != content;
            if content.is_some() {
                record.content = content;
            }
            (
                focus_changed,
                new_bounds,
                resized,
                std::mem::take(&mut record.hints_rescaled),
            )
        };
        // The limits, the steps and the proportion are device pixels.
        if rescaled && !self.closed.get() {
            self.write_size_hints(None);
            if let Err(error) = self.xcb().flush() {
                tracing::debug!(%error, "cannot set an X11 window's size hints");
            }
        }
        if focus_changed {
            self.facets
                .accessibility
                .focus_changed(state.focus.is_focused());
        }
        if let Some(bounds) = new_bounds {
            self.facets.accessibility.bounds_changed(bounds);
        }
        if resized {
            self.request_redraw();
        }
        // A synchronised resize's configure is read: answered now if no frame
        // is asked for, else after the frame (`deliver_frame`).
        let asked = self.redraw.load(Ordering::Acquire);
        let writes = self.resize_sync.borrow_mut().read(asked);
        self.write_counters(writes);
        if published.changed || unannounced {
            self.delivery.dispatch_state_changed();
        }
    }

    /// Set the window's counters to `writes`, where it has counters, and send
    /// them at once: the GPU driver may present on a connection of its own,
    /// so a compositor that reads the extended counter sees a frame begin
    /// before the frame is handed to the GPU, and end as soon as its handler
    /// returns.
    fn write_counters(&self, writes: CounterWrites) {
        let Some(counters) = self.counters else {
            return;
        };
        let writes = [
            (counters.basic, writes.basic),
            (counters.extended, writes.extended),
        ];
        let mut sent = false;
        for (counter, value) in writes {
            let Some(value) = value else {
                continue;
            };
            let (hi, lo) = value.halves();
            match self
                .xcb()
                .sync_set_counter(counter, xsync::Int64 { hi, lo })
            {
                Ok(_) => sent = true,
                Err(error) => {
                    tracing::debug!(%error, "cannot set an X11 window's resize counter");
                }
            }
        }
        if sent && let Err(error) = self.xcb().flush() {
            tracing::debug!(%error, "cannot send an X11 window's resize counter");
        }
    }

    /// Deliver the frame the window asked for, if it did and is open. A
    /// synchronised resize waiting for it is answered after it, and the
    /// extended counter a window manager that synchronises frames (`paced`)
    /// reads is odd while it draws, on every exit of the handler: one that
    /// panicked leaves neither a resize waiting nor a window the compositor
    /// stops showing. The handler's panic goes on after.
    fn deliver_frame(&self, paced: bool) {
        if !self.take_redraw() || self.closed.get() {
            return;
        }
        let (frame, begun) = self.resize_sync.borrow_mut().frame_begins(paced);
        self.write_counters(begun);
        let mut failure = FirstPanic::new();
        failure.contain(|| self.delivery.dispatch_request_frame());
        let ended = self.resize_sync.borrow_mut().frame_ends(frame);
        // A window the handler closed has nothing left to synchronise.
        if !self.closed.get() {
            self.write_counters(ended);
        }
        failure.resume();
    }

    /// The window manager asks, in a `WM_PROTOCOLS` message, for what the
    /// window takes: its close, an answer to its ping, or a synchronised
    /// resize.
    fn protocol_message(self: &Rc<Self>, message: &ClientMessageEvent) {
        if message.format != 32 {
            return;
        }
        let data = message.data.as_data32();
        let protocol = data[0];
        if protocol == self.atoms.wm_delete_window {
            self.close_requested();
        } else if protocol == self.atoms.net_wm_ping {
            // EWMH: the message goes back to the root with nothing changed
            // but its window. Answered here on the owner thread, so a window
            // whose owner hangs answers nothing and its window manager can
            // offer to end it; and sent now, not behind the turn's
            // application work and frames.
            self.send_to_root(ClientMessageEvent::new(
                32,
                self.connection.root(),
                message.type_,
                data,
            ));
            if let Err(error) = self.xcb().flush() {
                tracing::debug!(%error, "cannot send an X11 window's answer to a ping");
            }
        } else if protocol == self.atoms.net_wm_sync_request
            && self.counters.is_some()
            && let Some(request) = SyncRequest::from_message(data)
        {
            self.resize_sync.borrow_mut().request(request);
        }
    }

    /// The window as the server and the window manager hold it now; a
    /// rectangle that describes no window costs the sample its geometry and
    /// nothing else.
    fn sample(&self) -> Sample {
        let read = self.read_server();
        let support = self.support();
        let capabilities = capabilities(support);
        let snapshot = self.display_snapshot();
        let mut record = self.record.borrow_mut();
        let Some(read) = read else {
            // The window is gone from the server, or the connection failed:
            // what it last published stands, short of what it was asked.
            let mut state = self.cell.published();
            state.visible = false;
            state.focus = flui_platform::WindowFocus::Inactive;
            return Sample::from(state);
        };
        // What a window manager wrote on the window stands while one runs.
        let frame = support.kept(read.frame);
        let mode = mode_of(support.kept(mode_flags(read.wm_state, read.net_state)));
        let level = support.kept(read.level);
        let cursor_grab = self.input.pointer_grab(self.window_ref());
        let outer = outer_bounds(read.origin, read.content, frame);
        // The frame extents are not negative, so the content lies within
        // the outer rectangle unless that saturated; such a read is not told.
        if let Ok(bounds) = ScreenBounds::try_new(outer, Bounds::new(read.origin, read.content)) {
            record.bounds = Some(bounds);
        }
        // The display the window is on: the output it overlaps most, else the
        // nearest; its position is measured there.
        let display = snapshot
            .as_ref()
            .and_then(|snapshot| display_of(snapshot, outer));
        let scale = window_scale(display, record.scale);
        record.hints_rescaled |= record.scale != scale;
        record.scale = scale;
        self.input_scale.set(scale.get());
        // Remembered for a window whose platform is gone.
        record.support = support;
        // Shown while mapped, waiting for its first frame to be, or iconic: a
        // window manager unmaps the windows it iconifies (ICCCM 4.1.4). One
        // the application hid is withdrawn, iconic or not, and stays hidden
        // while the window manager still says iconic of it.
        let visible = record.reveal_pending
            || read.map_state != MapState::UNMAPPED
            || (record.shown && mode == WindowMode::Minimized);
        // Mapped under an unmapped ancestor (a window manager's frame on
        // another workspace) or fully covered.
        let occluded = read.map_state == MapState::UNVIEWABLE || record.obscured;
        let geometry = logical_geometry(outer, display, scale).and_then(|geometry| {
            let content = flui_platform::WindowExtent::try_new(Size::new(
                scale.to_logical(f64::from(read.content.width)),
                scale.to_logical(f64::from(read.content.height)),
            ))?;
            let normal = match mode {
                WindowMode::Normal => {
                    record.normal_geometry = Some(geometry);
                    None
                }
                _ => record.normal_geometry,
            };
            Ok(SampledGeometry {
                outer: geometry,
                content,
                normal,
            })
        });
        if mode == WindowMode::Normal
            && support.extended()
            && record.frame.has_system_title_bar()
            && let Some(windows) = self.windows.upgrade()
        {
            windows.frame_inset.record(frame.added());
        }
        let metrics = WindowMetrics::try_new(read.content, scale.get()).ok();
        Sample::read(geometry, |state| {
            let state = state
                .with_mode(mode)
                .with_visible(visible)
                .with_title(Some(record.title.clone()))
                .with_constraints(record.constraints)
                .with_resizable(record.resizable)
                .with_frame(record.frame)
                .with_display(display.map(DisplayFrame::display));
            ReportedFacts {
                metrics,
                brightness: self.appearance(),
                insets: flui_platform::WindowInsets::ZERO,
                execution: flui_platform::WindowExecutionState::Running,
                occluded,
                hovered: self.delivery.hovered(),
                // The window manager names the active window after the focus
                // moved, so the two can disagree for a read: the focus wins.
                focus: flui_platform::WindowFocus::reported(
                    read.focused,
                    if support.extended() {
                        read.active
                    } else {
                        read.focused
                    },
                ),
                capabilities,
                // A move or resize the window manager runs for the user is
                // not read yet (docs/gaps.md).
                interaction: flui_platform::WindowInteraction::Unknown,
                cursor_grab,
                level,
            }
            .onto(state)
        })
    }

    /// Ask the server for what the snapshot reads, every request first, then
    /// every reply. `None` when the window is gone or the connection failed.
    fn read_server(&self) -> Option<ServerRead> {
        let xcb = self.xcb();
        let root = self.connection.root();
        let atoms = &self.atoms;
        let geometry = xcb.get_geometry(self.xid).ok()?;
        let origin = xcb.translate_coordinates(self.xid, root, 0, 0).ok()?;
        let attributes = xcb.get_window_attributes(self.xid).ok()?;
        let net_state = xcb
            .get_property(
                false,
                self.xid,
                atoms.net_wm_state,
                AtomEnum::ATOM,
                0,
                PROPERTY_READ_LENGTH,
            )
            .ok()?;
        let wm_state = xcb
            .get_property(false, self.xid, atoms.wm_state, atoms.wm_state, 0, 2)
            .ok()?;
        let frame = xcb
            .get_property(
                false,
                self.xid,
                atoms.net_frame_extents,
                AtomEnum::CARDINAL,
                0,
                4,
            )
            .ok()?;
        let focus = xcb.get_input_focus().ok()?;
        let active = xcb
            .get_property(false, root, atoms.net_active_window, AtomEnum::WINDOW, 0, 1)
            .ok()?;

        let geometry = geometry.reply().ok()?;
        let origin = origin.reply().ok()?;
        let attributes = attributes.reply().ok()?;
        let words = |reply: xproto::GetPropertyReply| -> Vec<u32> {
            reply.value32().map_or_default(Iterator::collect)
        };
        let net_state = words(net_state.reply().ok()?);
        let wm_state = words(wm_state.reply().ok()?);
        let frame = words(frame.reply().ok()?);
        let focus = focus.reply().ok()?.focus;
        let active = words(active.reply().ok()?);
        Some(ServerRead {
            origin: Point::new(i32::from(origin.dst_x), i32::from(origin.dst_y)),
            content: Size::new(i32::from(geometry.width), i32::from(geometry.height)),
            frame: FrameExtents::from_property(&frame).unwrap_or_default(),
            map_state: attributes.map_state,
            wm_state: WmState::from_property(&wm_state),
            net_state: NetWmState::from_atoms(&net_state, &atoms.states),
            level: level_of(&net_state, &atoms.levels),
            focused: focus == self.xid,
            active: active.first() == Some(&self.xid),
        })
    }

    /// The scale of the display the window was last read on: the primary's
    /// when it is on none, the one it opened on before it was read.
    fn scale(&self) -> DevicePixelRatio {
        self.record.borrow().scale
    }

    /// Ask the window manager to add or remove up to two `_NET_WM_STATE`
    /// states of the window, which it manages (EWMH).
    fn send_state(&self, action: StateAction, first: xproto::Atom, second: xproto::Atom) {
        self.send_to_root(ClientMessageEvent::new(
            32,
            self.xid,
            self.atoms.net_wm_state,
            [action as u32, first, second, SOURCE_APPLICATION, 0],
        ));
    }

    /// Send `message` about the window to the root window, where the window
    /// manager hears it.
    fn send_to_root(&self, message: impl Into<[u8; 32]>) {
        let sent = self.xcb().send_event(
            false,
            self.connection.root(),
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            message,
        );
        if let Err(error) = sent {
            tracing::debug!(%error, "cannot send an X11 window's request to the window manager");
        }
    }

    /// An XInput 2 pointer event over the window's chrome: whether the window
    /// took it from the input (ADR-0286 §10). A primary press on a move region
    /// arms a move the window manager starts once the pointer travels, a
    /// second one soon after toggles maximize, a press on the resize band or a
    /// resize region hands the window manager a resize; the release of a taken
    /// press is taken too. The window manager is asked with nothing borrowed.
    fn chrome_event(&self, event: &Event) -> bool {
        match event {
            Event::XinputButtonPress(press) => self.chrome_press(press),
            Event::XinputMotion(motion) => {
                self.chrome_motion(motion);
                false
            }
            Event::XinputButtonRelease(release) => {
                let mut chrome = self.chrome.borrow_mut();
                if chrome.held != Some(release.detail) {
                    return false;
                }
                chrome.held = None;
                chrome.presses.released();
                true
            }
            _ => false,
        }
    }

    fn chrome_press(&self, press: &x11rb::protocol::xinput::ButtonPressEvent) -> bool {
        use x11rb::protocol::xinput::PointerEventFlags;

        if self.closed.get() {
            return false;
        }
        let emulated = u32::from(press.flags) & u32::from(PointerEventFlags::POINTER_EMULATED) != 0;
        let button = match press.detail {
            1 => PressButton::Primary,
            3 => PressButton::Secondary,
            _ => return false,
        };
        // A touch's emulated pointer is the application's: touches reach it as
        // their own.
        if emulated {
            return false;
        }
        let Some(point) = device_point(fp1616(press.event_x), fp1616(press.event_y)) else {
            return false;
        };
        let (frame, scale, content, resizable) = {
            let record = self.record.borrow();
            (record.frame, record.scale, record.content, record.resizable)
        };
        let titled = match frame {
            WindowFrame::AppTitleBar(_) => true,
            // One with no frame draws its own resize areas, and one with the
            // window manager's has its title bar there.
            WindowFrame::None => false,
            _ => return false,
        };
        // One read for the whole press: its resize band and its toggle go by
        // the same mode, the window's now.
        let mode = self.mode_now();
        let band = titled
            .then(|| {
                let resizes = resizable && mode == WindowMode::Normal;
                content.and_then(|size| ResizeBand::of(size, scale.get(), resizes))
            })
            .flatten();
        let answer = {
            let mut chrome = self.chrome.borrow_mut();
            chrome.held = None;
            let Chrome {
                projected, presses, ..
            } = &mut *chrome;
            presses.press(
                projected.sent(),
                point,
                button,
                band,
                Instant::now(),
                scale.get(),
            )
        };
        match answer {
            // The window manager's menu is asked for with a message only some
            // window managers take (`_GTK_SHOW_WINDOW_MENU`); the press stays
            // the application's.
            ChromePress::App | ChromePress::WindowMenu => return false,
            ChromePress::Move => {}
            ChromePress::Resize(edge) => {
                let Some(direction) = moveresize_direction(edge) else {
                    return false;
                };
                self.move_resize(direction, press);
            }
            ChromePress::ToggleMaximize => {
                let mode = match mode {
                    WindowMode::Maximized => WindowMode::Normal,
                    _ => WindowMode::Maximized,
                };
                if let Err(error) = self.enter_mode(mode) {
                    tracing::debug!(%error, "cannot toggle an X11 window's maximize");
                }
                if let Err(error) = self.xcb().flush() {
                    tracing::debug!(%error, "cannot toggle an X11 window's maximize");
                }
                self.mark_stale();
            }
        }
        self.chrome.borrow_mut().held = Some(press.detail);
        true
    }

    /// The pointer moved with a taken press on the bar held: the window manager
    /// moves the window once it has travelled far enough.
    fn chrome_motion(&self, motion: &x11rb::protocol::xinput::ButtonPressEvent) {
        let Some(point) = device_point(fp1616(motion.event_x), fp1616(motion.event_y)) else {
            return;
        };
        let scale = self.record.borrow().scale;
        let starts = {
            let mut chrome = self.chrome.borrow_mut();
            chrome.held == Some(1) && chrome.presses.moved(point, scale.get())
        };
        if starts {
            self.move_resize(MOVERESIZE_MOVE, motion);
        }
    }

    /// Hand the window manager a move or a resize in `direction`, from the
    /// pointer where `event` saw it: the pointer's grab is let go of first,
    /// so the window manager can take it.
    fn move_resize(&self, direction: u32, event: &x11rb::protocol::xinput::ButtonPressEvent) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a root coordinate of the X server fits the i16 range it came from"
        )]
        let root = |value: i32| fp1616(value).round() as i32;
        // A hold of the pointer the window asked for ends with the grab.
        self.input.let_pointer_go(self.window_ref());
        if let Err(error) = self.xcb().ungrab_pointer(x11rb::CURRENT_TIME) {
            tracing::debug!(%error, "cannot let go of the pointer for the window manager");
        }
        self.send_to_root(ClientMessageEvent::new(
            32,
            self.xid,
            self.atoms.net_wm_moveresize,
            [
                root(event.root_x).cast_unsigned(),
                root(event.root_y).cast_unsigned(),
                direction,
                event.detail,
                // A request of an application.
                1,
            ],
        ));
        if let Err(error) = self.xcb().flush() {
            tracing::debug!(%error, "cannot ask the window manager to move the window");
        }
    }

    /// Whether the window is unmapped and waits to be shown: the window
    /// manager does not manage it, and reads a mode from its properties when
    /// it maps it.
    fn unmapped(&self) -> bool {
        let record = self.record.borrow();
        record.reveal_pending || !record.shown
    }

    /// Whether the window is open and the application wants it shown,
    /// mapped or waiting for its first frame to be.
    fn shown(&self) -> bool {
        !self.closed.get() && self.record.borrow().shown
    }

    /// Write `mode` into `_NET_WM_STATE` of the unmapped window, when the
    /// window manager supports it: it reads the mode there as it maps the
    /// window, and ignores a request about a window it does not manage.
    fn seed_mode(&self, mode: WindowMode) {
        let support = self.support();
        let supported = match mode {
            WindowMode::Maximized => support.maximizes,
            WindowMode::Fullscreen => support.fullscreen,
            _ => true,
        };
        if !supported {
            return;
        }
        if let Some(atoms) = with_mode(&self.own_net_wm_state(), mode, &self.atoms.states)
            && let Err(error) = self.xcb().change_property32(
                PropMode::REPLACE,
                self.xid,
                self.atoms.net_wm_state,
                AtomEnum::ATOM,
                &atoms,
            )
        {
            tracing::debug!(%error, ?mode, "cannot set an X11 window's mode before it is mapped");
        }
    }

    /// The atoms of the window's `_NET_WM_STATE` now; none where it cannot
    /// be read.
    fn own_net_wm_state(&self) -> Vec<u32> {
        self.xcb()
            .get_property(
                false,
                self.xid,
                self.atoms.net_wm_state,
                AtomEnum::ATOM,
                0,
                PROPERTY_READ_LENGTH,
            )
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .and_then(|reply| Some(reply.value32()?.collect()))
            .unwrap_or_default()
    }

    /// Enter `mode`. Leaving fullscreen first keeps entering a mode from
    /// depending on what the window manager makes of two states at once.
    fn enter_mode(&self, mode: WindowMode) -> Result<(), ApplyError> {
        if !matches!(
            mode,
            WindowMode::Normal
                | WindowMode::Minimized
                | WindowMode::Maximized
                | WindowMode::Fullscreen
        ) {
            return Err(not_carried_out(&WindowCommand::SetMode(mode)));
        }
        if self.unmapped() {
            self.record.borrow_mut().minimize_on_map = mode == WindowMode::Minimized;
            self.seed_mode(mode);
            return Ok(());
        }
        let states = self.atoms.states;
        let minimized = self.mode_now() == WindowMode::Minimized;
        match mode {
            WindowMode::Minimized => self.send_to_root(ClientMessageEvent::new(
                32,
                self.xid,
                self.atoms.wm_change_state,
                [ICONIC_STATE, 0, 0, 0, 0],
            )),
            WindowMode::Maximized => {
                self.send_state(StateAction::Remove, states.fullscreen, 0);
                if minimized {
                    self.map();
                }
                self.send_state(
                    StateAction::Add,
                    states.maximized_horz,
                    states.maximized_vert,
                );
            }
            WindowMode::Fullscreen => {
                if minimized {
                    self.map();
                }
                self.send_state(StateAction::Add, states.fullscreen, 0);
            }
            _ => {
                self.send_state(StateAction::Remove, states.fullscreen, 0);
                self.send_state(
                    StateAction::Remove,
                    states.maximized_horz,
                    states.maximized_vert,
                );
                // ICCCM: an iconic window returns to normal by being mapped.
                if minimized {
                    self.map();
                }
            }
        }
        Ok(())
    }

    /// Map the window, fitted first to the rectangle it was opened at, and
    /// minimized after when that is what it was last asked to be. Returns
    /// whether a launch forwarded while no window was shown brought it
    /// forward, at that launch's time.
    fn map(&self) -> bool {
        self.fit_to_remembered_rectangle();
        if let Err(error) = self.xcb().map_window(self.xid) {
            tracing::debug!(%error, "cannot map an X11 window");
        }
        let minimize = std::mem::take(&mut self.record.borrow_mut().minimize_on_map);
        if minimize {
            self.send_to_root(ClientMessageEvent::new(
                32,
                self.xid,
                self.atoms.wm_change_state,
                [ICONIC_STATE, 0, 0, 0, 0],
            ));
            return false;
        }
        let forwarded = self
            .windows
            .upgrade()
            .and_then(|windows| windows.forwarded_activation.take());
        forwarded.inspect(|time| self.activate_at(*time)).is_some()
    }

    /// Give a window opened at a remembered outer rectangle the content size
    /// that makes its outer size that. The first window of a run has no frame
    /// measured yet, so the window manager is asked for the one it will add
    /// (`_NET_REQUEST_FRAME_EXTENTS`), waiting briefly: without it,
    /// un-maximizing the window would leave it larger than it was saved by
    /// the frame, and saving that again grows it every run.
    fn fit_to_remembered_rectangle(&self) {
        let Some(outer) = self.record.borrow_mut().outer_to_fit.take() else {
            return;
        };
        let Some(windows) = self.windows.upgrade() else {
            return;
        };
        if windows.frame_inset.is_known()
            || !self.record.borrow().frame.has_system_title_bar()
            || !self.support().extended()
        {
            return;
        }
        let Some(frame) = self.request_frame_extents() else {
            return;
        };
        windows.frame_inset.record(frame.added());
        let scale = self.scale();
        let inset = windows.frame_inset.logical(scale);
        let size = |outer: f64, inset: f64| device_extent((outer - inset).max(1.0), scale);
        if let (Some(width), Some(height)) = (
            size(outer.width, inset.width),
            size(outer.height, inset.height),
        ) && let Err(error) = self.xcb().configure_window(
            self.xid,
            &ConfigureWindowAux::new()
                .width(u32::from(width))
                .height(u32::from(height)),
        ) {
            tracing::debug!(%error, "cannot fit an X11 window to its remembered rectangle");
        }
    }

    /// The frame the window manager will put around the unmapped window,
    /// waiting at most [`FRAME_EXTENTS_WAIT`] for it to say.
    fn request_frame_extents(&self) -> Option<FrameExtents> {
        self.send_to_root(ClientMessageEvent::new(
            32,
            self.xid,
            self.atoms.net_request_frame_extents,
            [0_u32; 5],
        ));
        self.xcb().flush().ok()?;
        let deadline = Instant::now() + FRAME_EXTENTS_WAIT;
        loop {
            let frame = self
                .xcb()
                .get_property(
                    false,
                    self.xid,
                    self.atoms.net_frame_extents,
                    AtomEnum::CARDINAL,
                    0,
                    4,
                )
                .ok()?
                .reply()
                .ok()
                .and_then(|reply| {
                    FrameExtents::from_property(&reply.value32()?.collect::<Vec<_>>())
                });
            if frame.is_some() {
                return frame;
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Show the window: mapped, out of the minimized mode, and focused. A
    /// window that was unmapped starts uncovered; X reports the new mapping's
    /// visibility.
    fn show(&self) {
        let was_unmapped = {
            let mut record = self.record.borrow_mut();
            let was_unmapped = record.reveal_pending || !record.shown;
            record.reveal_pending = false;
            record.shown = true;
            if was_unmapped {
                record.obscured = false;
            }
            was_unmapped
        };
        let brought_forward =
            (was_unmapped || self.mode_now() == WindowMode::Minimized) && self.map();
        if !brought_forward {
            self.activate();
        }
    }

    /// Withdraw the window (ICCCM 4.1.4): unmapped, with the synthetic
    /// `UnmapNotify` that tells the window manager to let go of it. A hidden
    /// window has no first frame to wait for.
    fn hide(&self) {
        {
            let mut record = self.record.borrow_mut();
            record.reveal_pending = false;
            record.shown = false;
        }
        // A hidden window holds no pointer.
        self.input.let_pointer_go(self.window_ref());
        if let Err(error) = self.xcb().unmap_window(self.xid) {
            tracing::debug!(%error, "cannot unmap an X11 window");
        }
        self.send_to_root(UnmapNotifyEvent {
            response_type: xproto::UNMAP_NOTIFY_EVENT,
            sequence: 0,
            event: self.connection.root(),
            window: self.xid,
            from_configure: false,
        });
    }

    /// Give the window the focus: through the window manager where one speaks
    /// EWMH (`_NET_ACTIVE_WINDOW`, which also raises it), else from the server
    /// directly.
    fn activate(&self) {
        self.activate_at(x11rb::CURRENT_TIME);
    }

    /// [`activate`](Self::activate) at `time`, the server time of the user
    /// action that asked for it: a window manager that prevents focus
    /// stealing compares it with the user's last input.
    fn activate_at(&self, time: u32) {
        if self.support().extended() {
            self.send_to_root(ClientMessageEvent::new(
                32,
                self.xid,
                self.atoms.net_active_window,
                [SOURCE_APPLICATION, time, 0, 0, 0],
            ));
            return;
        }
        let raised = self.xcb().configure_window(
            self.xid,
            &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
        );
        let focused = self
            .xcb()
            .set_input_focus(InputFocus::PARENT, self.xid, time);
        if let Err(error) = raised.and(focused) {
            tracing::debug!(%error, "cannot focus an X11 window");
        }
    }

    /// Resize the content to `size`, in normal mode: a maximized or
    /// fullscreen window has no size of its own to set, so it returns to
    /// normal mode at the size asked for, as on every desktop host.
    fn resize(&self, size: Size<f64>) -> Result<(), ApplyError> {
        let scale = self.scale();
        let (Some(width), Some(height)) = (
            device_extent(size.width, scale),
            device_extent(size.height, scale),
        ) else {
            return Err(ApplyError::failed(format!(
                "{size:?} is not a size an X11 window takes"
            )));
        };
        let mode = self.mode_now();
        if !self.unmapped() && matches!(mode, WindowMode::Maximized | WindowMode::Fullscreen) {
            let states = self.atoms.states;
            self.send_state(StateAction::Remove, states.fullscreen, 0);
            self.send_state(
                StateAction::Remove,
                states.maximized_horz,
                states.maximized_vert,
            );
        }
        self.xcb()
            .configure_window(
                self.xid,
                &ConfigureWindowAux::new()
                    .width(u32::from(width))
                    .height(u32::from(height)),
            )
            .map_err(|error| ApplyError::failed(error.to_string()))?;
        if !self.record.borrow().resizable {
            self.write_size_hints(None);
        }
        Ok(())
    }

    /// Move the window's outer top-left corner to `at`, a point of a display
    /// that converts through that display's own origin and scale. With the
    /// north-west gravity its hints give, the window manager places the frame
    /// there (ICCCM 4.1.2.3).
    ///
    /// # Errors
    /// [`ApplyError::DisplayGone`] for a display the snapshot no longer holds,
    /// [`PlatformFailure::OutOfRange`](flui_platform::PlatformFailure::OutOfRange)
    /// for a point past X11's 16-bit coordinates, both before any request.
    fn place(&self, at: WindowPosition) -> Result<(), ApplyError> {
        let frame = self
            .display_snapshot()
            .and_then(|snapshot| DisplayFrame::find(&snapshot, at.display()))
            .ok_or(ApplyError::DisplayGone(at.display()))?;
        let Some((x, y)) = x11_coordinates(frame, at.at()) else {
            return Err(ApplyError::Platform(
                flui_platform::PlatformFailure::OutOfRange,
            ));
        };
        self.xcb()
            .configure_window(
                self.xid,
                &ConfigureWindowAux::new().x(i32::from(x)).y(i32::from(y)),
            )
            .map_err(|error| ApplyError::failed(error.to_string()))?;
        Ok(())
    }

    /// Ask for a frame, delivered on the loop's next turn.
    fn mark_redraw(&self) {
        if self.closed.get() {
            return;
        }
        self.redraw.store(true, Ordering::Release);
        if let Err(error) = self.doorbell.ring() {
            tracing::error!(%error, "a frame request could not wake the X11 loop");
        }
    }

    /// The frame asked for since the last call, if any.
    fn take_redraw(&self) -> bool {
        self.redraw.swap(false, Ordering::AcqRel)
    }

    /// Close the window, once: withdrawn at once, `closed` delivered, out of
    /// the platform's table, and the X window destroyed after the handler is
    /// let go of. A [`WindowCommand::Close`] takes this path unasked, the
    /// window manager's close after the handler granted it.
    fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        {
            let mut record = self.record.borrow_mut();
            record.shown = false;
            record.reveal_pending = false;
        }
        // The steps below run user code (a tracing subscriber, `closed` and a
        // delivery queued ahead of it, the facets' detach). Every step runs
        // whichever one before it panics: `closed` is already set, so nothing
        // later releases what this close skips. The first panic resumes at
        // the end.
        let mut failure = FirstPanic::new();
        // The visible effect does not wait for the handler.
        if self.native.get() == NativeWindow::Live
            && let Err(error) = self.xcb().unmap_window(self.xid)
        {
            failure.contain(|| tracing::debug!(%error, "cannot unmap a closing X11 window"));
        }
        failure.contain(|| self.delivery.dispatch_close());
        if let Some(windows) = self.windows.upgrade() {
            windows.forget(self.id, self.xid);
        }
        failure.contain(|| self.facets.text.detach());
        failure.contain(|| self.facets.accessibility.detach());
        failure.contain(|| self.forget_features());
        failure.resume();
    }

    /// Tell the input, the drag and the text input the window is gone, then retire it. Each
    /// may run user code (a request's waker, a handler): the retirement runs
    /// whatever fails first, and the first failure goes on after it.
    fn forget_features(&self) {
        let mut failure = FirstPanic::new();
        failure.contain(|| self.input.window_closed(self.window_ref()));
        failure.contain(|| self.drag.window_closed(self.window_ref()));
        failure.contain(|| self.text.window_closed(self.window_ref()));
        failure.contain(|| self.retire());
        failure.resume();
    }

    /// What a feature reaches this window by.
    fn window_ref(&self) -> WindowRef {
        WindowRef {
            xid: self.xid,
            id: self.id,
        }
    }

    /// Let go of the handler (`WindowDelivery::clear_then`), once, and destroy
    /// the X window after it, unless the server already has: the handler owns
    /// the renderer whose surface names the window, and a surface must not
    /// outlive its window. The retirement the release starts makes the
    /// surface target and the redraw requester answer for a window that is
    /// gone.
    ///
    /// A window whose surface target was asked, while it was open or before
    /// this release, to keep it past the close ([`X11Surface`]'s
    /// `keep_past_close`) is left unmapped instead, never destroyed: a raster
    /// thread that did not answer may still present over it. The platform's
    /// table no longer names it, so the events the server still sends for it
    /// reach no window.
    fn retire(&self) {
        let retirement = self.delivery.retirement();
        let live = match self.native.replace(NativeWindow::Released) {
            NativeWindow::Released => return,
            NativeWindow::Live => true,
            NativeWindow::DestroyedByServer => false,
        };
        let connection = Arc::clone(&self.connection);
        let xid = self.xid;
        // The counters are this connection's, which the server keeps until it
        // closes: they go with the window, whoever destroyed it.
        let counters = self.counters;
        self.delivery.clear_then(move || {
            // A window the server destroyed has nothing left to destroy or
            // keep: a keep the release makes does nothing to it.
            let fate = retirement.settle();
            let xcb = connection.xcb();
            let ignored = x11rb::cookie::VoidCookie::ignore_error;
            let mut sent = Ok(());
            if let Some(counters) = counters {
                for counter in [counters.basic, counters.extended] {
                    sent = sent.and_then(|()| xcb.sync_destroy_counter(counter).map(ignored));
                }
            }
            if live {
                sent = sent.and_then(|()| match fate {
                    NativeFate::Destroy => xcb.destroy_window(xid).map(drop),
                    NativeFate::Keep => xcb.unmap_window(xid).map(drop),
                });
            }
            if let Err(error) = sent.and_then(|()| xcb.flush()) {
                tracing::debug!(%error, "cannot destroy or unmap a retired X11 window");
            }
        });
    }

    /// The window manager asks the window to close (`WM_DELETE_WINDOW`): the
    /// handler decides. A request raised while a handler method runs is
    /// answered later, and closes the window then if granted.
    fn close_requested(self: &Rc<Self>) {
        let window = OwnerWeak::new(self);
        let decision = self.delivery.dispatch_close_request(move || {
            if let Some(window) = window.upgrade() {
                window.close();
            }
        });
        if decision == CloseDecision::Close {
            self.close();
        }
    }

    /// An event the server sent about this window.
    fn handle(self: &Rc<Self>, event: &Event) {
        match event {
            Event::Expose(expose) if expose.count == 0 => self.mark_redraw(),
            Event::MapNotify(_) => {
                let minimize = std::mem::take(&mut self.record.borrow_mut().minimize_on_map);
                if minimize {
                    self.send_to_root(ClientMessageEvent::new(
                        32,
                        self.xid,
                        self.atoms.wm_change_state,
                        [ICONIC_STATE, 0, 0, 0, 0],
                    ));
                }
                self.mark_stale();
            }
            Event::VisibilityNotify(visibility) => {
                self.record.borrow_mut().obscured = visibility.state == Visibility::FULLY_OBSCURED;
                self.mark_stale();
            }
            Event::PropertyNotify(property) if self.atoms.concerns_window(property.atom) => {
                self.mark_stale();
            }
            Event::ClientMessage(message) if message.type_ == self.atoms.wm_protocols => {
                self.protocol_message(message);
            }
            Event::DestroyNotify(_) => {
                // Destroyed by another client (`xkill`): there is nothing
                // left to destroy, but the window still closes and lets go of
                // its handler.
                if self.native.get() == NativeWindow::Live {
                    self.native.set(NativeWindow::DestroyedByServer);
                }
                self.close();
            }
            Event::ConfigureNotify(_) => {
                self.resize_sync.borrow_mut().configured();
                self.mark_stale();
            }
            Event::UnmapNotify(_)
            | Event::ReparentNotify(_)
            | Event::FocusIn(_)
            | Event::FocusOut(_) => self.mark_stale(),
            _ => {}
        }
    }

    /// Let go of the window at the platform's end: closed to commands, its
    /// handler released and its X window destroyed after, without `closed`,
    /// which only a close delivers.
    fn release(&self) {
        self.closed.set(true);
        self.forget_features();
    }
}

/// What the server holds of a window, read back in one batch.
struct ServerRead {
    /// The content's top-left corner on the root window.
    origin: Point<i32>,
    content: Size<i32>,
    frame: FrameExtents,
    map_state: MapState,
    wm_state: Option<WmState>,
    net_state: NetWmState,
    /// The level `_NET_WM_STATE` keeps the window at.
    level: WindowLevel,
    focused: bool,
    active: bool,
}

impl Drop for X11Window {
    fn drop(&mut self) {
        // The handler may own the window's renderer, whose surface names the
        // window: it goes first, then the X window. A delivery queued ahead
        // of the release may panic: the drop still finishes (a panic out of
        // a drop while unwinding aborts).
        self.closed.set(true);
        crate::owner::panic_boundary::contain_owner_callback(|| self.retire());
    }
}

/// An X11 window's surface target (ADR-0254 §5): the platform's connection
/// as its display and the window as its window, until the window's
/// retirement.
struct X11Surface {
    connection: Arc<X11Connection>,
    xid: NonZeroU32,
    visual: Option<NonZeroU32>,
    retirement: Retirement,
}

impl HasWindowHandle for X11Surface {
    #[expect(
        unsafe_code,
        reason = "a raw window handle is borrowed for as long as the target lives"
    )]
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        if self.retirement.is_retired() {
            return Err(HandleError::Unavailable);
        }
        let mut handle = XcbWindowHandle::new(self.xid);
        handle.visual_id = self.visual;
        // SAFETY: the X window lives until the window's retirement has let go
        // of its handler (`X11Window::retire` destroys it in the retirement's
        // completion), and the handler owns the renderer that holds this
        // target and every surface made from the handle; once retirement has
        // started, no handle is handed out.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Xcb(handle)) })
    }
}

impl HasDisplayHandle for X11Surface {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        if self.retirement.is_retired() {
            return Err(HandleError::Unavailable);
        }
        xcb_display(&self.connection)
    }
}

/// The X11 host's display connection (ADR-0305 §2): the connection its windows
/// are made on, so its handle is every window target's.
pub(crate) struct X11Display(pub(crate) Arc<X11Connection>);

impl HasDisplayHandle for X11Display {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        xcb_display(&self.0)
    }
}

impl flui_platform::DisplayConnection for X11Display {}

/// The XCB display handle of `connection`, borrowed for as long as the
/// connection is.
#[expect(
    unsafe_code,
    reason = "a raw display handle is borrowed for as long as the connection lives"
)]
fn xcb_display(connection: &X11Connection) -> Result<DisplayHandle<'_>, HandleError> {
    let raw = std::ptr::NonNull::new(connection.xcb().get_raw_xcb_connection());
    let screen = i32::try_from(connection.screen_number()).map_err(|_| HandleError::Unavailable)?;
    // SAFETY: the libxcb connection the pointer names closes only when the
    // last `Arc<X11Connection>` drops, and the borrow cannot outlive the
    // `&X11Connection` it is made from.
    Ok(unsafe {
        DisplayHandle::borrow_raw(RawDisplayHandle::Xcb(XcbDisplayHandle::new(raw, screen)))
    })
}

impl flui_platform::SurfaceTarget for X11Surface {
    /// X11 has no frame callback to arm: a present is paced by the swap
    /// chain, and this returns at once. The swap chain's completion events go
    /// to the GPU driver's own connection, so the host learns nothing of when
    /// the frame is shown.
    fn pre_present_notify(
        &self,
        _: Option<flui_foundation::FrameStamp>,
        _: flui_platform::SwapChainStatistics,
    ) {
    }

    /// Records the keep and changes nothing else: a window still open keeps
    /// showing and taking input. Its retirement, whichever way it comes (a
    /// user's close, a command, the platform's release at quit), then unmaps
    /// the X window instead of destroying it (`X11Window::retire`), and this
    /// target, leaked, keeps the connection the window and the surface's
    /// display live on. A keep after the retirement destroyed the window, or
    /// after another client did, does nothing.
    fn keep_past_close(&self) {
        let _first = self.retirement.keep_past_close();
    }
}

/// An X11 window's redraw requester: any thread marks the window and rings
/// the loop's doorbell. It holds no window, so a requester a waker keeps
/// never keeps a closed window alive.
struct X11Redraw {
    redraw: Arc<AtomicBool>,
    doorbell: Doorbell,
    retirement: Retirement,
}

impl flui_platform::RedrawRequester for X11Redraw {
    fn request_redraw(&self) {
        if self.retirement.is_retired() {
            return;
        }
        self.redraw.store(true, Ordering::Release);
        if let Err(error) = self.doorbell.ring() {
            tracing::error!(%error, "a frame request could not wake the X11 loop");
        }
    }
}

impl PlatformWindow for X11Window {
    fn id(&self) -> WindowId {
        self.id
    }

    /// The X server states no gesture distances; the consumer keeps its own.
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

    fn request_redraw(&self) {
        self.mark_redraw();
    }

    fn redraw_requester(&self) -> Arc<dyn flui_platform::RedrawRequester> {
        Arc::new(X11Redraw {
            redraw: Arc::clone(&self.redraw),
            doorbell: self.doorbell.clone(),
            retirement: self.delivery.retirement(),
        })
    }

    fn surface_target(&self) -> Arc<dyn flui_platform::SurfaceTarget> {
        let xid = NonZeroU32::new(self.xid).expect("BUG: the server gives no window the id 0");
        Arc::new(X11Surface {
            connection: Arc::clone(&self.connection),
            xid,
            visual: NonZeroU32::new(self.connection.screen().root_visual),
            retirement: self.delivery.retirement(),
        })
    }

    /// A revision this read publishes is announced at the owner's turn, never
    /// from the read.
    fn state(&self) -> WindowState {
        let published = self.cell.read_unannounced(&self.delivery, || self.sample());
        // A revision a command published with nobody told is told with this
        // read's, at the owner's turn, never from the read.
        let unannounced = self.unannounced.take();
        if published.changed || unannounced {
            self.delivery.state_changed_after_read();
        }
        published.state
    }

    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        if self.closed.get() {
            return Err(ApplyError::Closed);
        }
        // Opened under what the window honours now: an event that marked it
        // (the window manager's end) may have published other capabilities
        // since the publication the command was admitted against (ADR-0341).
        let state = self.state_now();
        let command = command.against(&state.capabilities)?;
        match command {
            WindowCommand::SetTitle(title) => {
                self.write_title(&title);
                self.record.borrow_mut().title = title;
            }
            WindowCommand::SetPosition(at) => self.place(at)?,
            WindowCommand::SetSize(size) => self.resize(size.get())?,
            WindowCommand::SetConstraints(constraints) => {
                self.record.borrow_mut().constraints = constraints;
                self.write_size_hints(None);
            }
            WindowCommand::SetResizable(resizable) => {
                self.record.borrow_mut().resizable = resizable;
                self.write_size_hints(None);
            }
            WindowCommand::SetFrame(frame) => {
                let decorated = frame.has_system_title_bar();
                self.record.borrow_mut().frame = frame;
                self.write_decorations(decorated);
            }
            WindowCommand::SetMode(mode) => self.enter_mode(mode)?,
            WindowCommand::Show => self.show(),
            WindowCommand::Hide => self.hide(),
            WindowCommand::Activate => self.activate(),
            WindowCommand::Close => {
                self.close();
                return Ok(());
            }
            WindowCommand::SetCursorGrab(CursorGrab::None) => {
                self.input.let_pointer_go(self.window_ref());
            }
            // Only a window with the keyboard focus holds the pointer: one
            // without it holds none, which its snapshot shows.
            // A hold the server refuses leaves none: the one the window held
            // was let go of first, and the snapshot says so before the
            // refusal is answered.
            WindowCommand::SetCursorGrab(grab) => {
                if state.focus.is_focused()
                    && let Err(refused) = self.input.hold_pointer(self.window_ref(), grab)
                {
                    self.refresh_state();
                    return Err(ApplyError::Platform(refused));
                }
            }
            WindowCommand::SetLevel(level) => self.set_level(level),
            WindowCommand::SetResizeIncrements(step) => {
                self.record.borrow_mut().resize_increments = step;
                self.write_size_hints(None);
            }
            WindowCommand::SetAspectRatio(ratio) => {
                self.record.borrow_mut().aspect_ratio = ratio;
                self.write_size_hints(None);
            }
            // `admit` refused the material: its capability is unavailable.
            other => return Err(not_carried_out(&other)),
        }
        if let Err(error) = self.xcb().flush() {
            return Err(ApplyError::failed(error.to_string()));
        }
        self.refresh_state();
        Ok(())
    }

    fn reveal_after_first_frame(&self) {
        {
            let mut record = self.record.borrow_mut();
            if self.closed.get() || !std::mem::take(&mut record.reveal_pending) {
                return;
            }
            record.shown = true;
        }
        self.map();
        if let Err(error) = self.xcb().flush() {
            tracing::debug!(%error, "cannot map an X11 window after its first frame");
        }
        self.refresh_state();
    }

    /// The pointer and its cursor are the input feature's: it loads a system
    /// cursor from the theme, makes an image a Render cursor, and sets the
    /// cursor on the window.
    fn set_cursor(&self, cursor: &Cursor) -> Result<(), CursorError> {
        if self.closed.get() {
            return Err(Unsupported::new(UnsupportedReason::NoWindow).into());
        }
        self.input.set_cursor(self.window_ref(), cursor)
    }

    /// X11 takes no refresh-rate request.
    fn set_frame_rate_hint(&self, _hint: flui_platform::FrameRateHint) -> Result<(), Unsupported> {
        Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform))
    }

    /// `x11:<XID>`: the server's id names the window to every client while
    /// it exists, so the name needs nothing held.
    fn portal_parent(
        &self,
        answer: flui_platform::capability::Resolver<flui_platform::capability::PortalParent>,
    ) {
        let parent = std::num::NonZeroU32::new(self.xid).map_or_else(
            flui_platform::capability::PortalParent::none,
            flui_platform::capability::PortalParent::x11,
        );
        // The owner retiring first leaves nothing waiting for the answer.
        let _ = answer.resolve(parent);
    }

    /// The chrome regions a press on a window without decorations is answered
    /// from (ADR-0286 §10), projected at the window's scale. X11 has no system
    /// edge gestures, so those regions are kept and never read.
    fn set_input_regions(
        &self,
        regions: &[flui_platform::input_regions::InputRegion],
    ) -> Result<(), Unsupported> {
        if self.closed.get() {
            return Err(Unsupported::new(UnsupportedReason::NoWindow));
        }
        let scale = self.record.borrow().scale;
        // Projected with nothing borrowed: a dropped region is reported
        // through `tracing`, whose subscriber is the application's code.
        let mut projected = std::mem::take(&mut self.chrome.borrow_mut().projected);
        let _changed = projected.update(regions, scale, None);
        self.chrome.borrow_mut().projected = projected;
        Ok(())
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        self.facets
            .text
            .window_text_input()
            .and_then(WindowTextInput::ime)
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::haptics::PlatformHaptics>> {
        Some(Arc::clone(&self.facets.haptics))
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        Some(Arc::clone(&self.facets.chrome))
    }

    /// Its sessions run over XDND on the loop's transfer.
    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        Some(Arc::clone(&self.drag).drag_source(self.window_ref()))
    }
}

impl crate::host_window::CoreHostWindow for X11Window {
    fn window_delivery(&self) -> &WindowDelivery {
        &self.delivery
    }
}

impl HostWindow for X11Window {
    /// The window's AT-SPI bridge, which announced the window on the bus as
    /// it opened.
    fn accessibility(&self) -> Option<Arc<dyn PlatformAccessibility>> {
        Some(Arc::clone(&self.facets.accessibility) as _)
    }

    fn capture_text_sizing(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Result<crate::TextSizingCaptureState, crate::TextSizingCaptureError> {
        self.facets.text.capture_sizing()
    }

    fn text_store_host(
        &self,
        _owner: crate::host_window::OwnerThreadToken,
    ) -> Option<Rc<dyn flui_platform::text_store::TextStoreHost>> {
        self.facets
            .text
            .window_text_input()
            .and_then(WindowTextInput::store_host)
    }
}

/// The X11 windows' table: each open window by its identity, the window each
/// X id names now, and the identities issued.
#[derive(Default)]
struct WindowTable {
    windows: FxHashMap<WindowId, Rc<X11Window>>,
    by_xid: FxHashMap<xproto::Window, WindowId>,
    /// Issues the id of every window, never one twice.
    ids: WindowIdMint,
}

/// The platform's X11 windows: what they open with and share, and the table
/// the loop reaches them through.
///
/// Built on the thread that builds the platform, which owns the windows and
/// alone reaches them (ADR-0327 §5). Public where it is defined, as the host's table
/// names it, and nameable only inside the crate.
pub struct X11Windows {
    positions: WindowPositions<WindowRef>,
    connection: Arc<X11Connection>,
    atoms: WindowAtoms,
    /// The class every window carries in `WM_CLASS`.
    class: AppClass,
    /// The machine and process every window names, read once.
    process: ClientProcess,
    /// The server has the SYNC extension, initialised on the connection: each
    /// window makes the counters a synchronised resize waits on.
    synchronises: bool,
    table: RefCell<WindowTable>,
    /// A window closed since the loop last asked.
    closed: Cell<bool>,
    frames: FrameCount,
    frame_inset: Arc<FrameInset>,
    support: Cell<WindowManagerSupport>,
    /// The window manager's own window (`_NET_SUPPORTING_WM_CHECK`, or the
    /// owner of the screen's `WM_S<n>` selection), whose end the platform hears:
    /// a window manager that is killed changes no property of the root window,
    /// but the server destroys its windows.
    window_manager: Cell<Option<xproto::Window>>,
    /// An event said what the window manager supports may have changed, and
    /// it has not been read since.
    support_unread: Cell<bool>,
    /// A window is marked and has not read itself since.
    unread: Cell<bool>,
    appearance: Cell<Brightness>,
    /// The window that had the focus last, whether or not it still has: the
    /// one a forwarded launch brings forward.
    last_focused: Cell<Option<WindowId>>,
    /// The time of a launch forwarded while no window was shown: the next
    /// window mapped comes forward at it, as the window the application
    /// shows from the reopen it was asked.
    forwarded_activation: Cell<Option<u32>>,
}
// Owner-local (ADR-0327 §5): the table is the owner thread's alone.
static_assertions::assert_not_impl_any!(X11Windows: Send, Sync);

impl std::fmt::Debug for X11Windows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("X11Windows").finish_non_exhaustive()
    }
}

impl X11Windows {
    /// The windows of `connection`, each handing out what `positions` make,
    /// with the atoms interned on it, and each carrying `class`, the machine
    /// `uname` names and the process (`ClientProcess`). Hears the root window's property changes from
    /// here on (the window manager's support and the active window) and the
    /// messages sent to its structure (a window manager announcing itself,
    /// ICCCM §2.8), initialises the SYNC extension where the server has it,
    /// and reads what the window manager supports.
    pub(crate) fn new(
        positions: WindowPositions<WindowRef>,
        connection: Arc<X11Connection>,
        atoms: WindowAtoms,
        class: AppClass,
    ) -> Self {
        // The root window's mask is this connection's: a feature that needs
        // more of the root's events adds them here, since a second
        // `ChangeWindowAttributes` replaces the first.
        let watched = connection
            .xcb()
            .change_window_attributes(
                connection.root(),
                &ChangeWindowAttributesAux::new()
                    .event_mask(EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY),
            )
            .map(x11rb::cookie::VoidCookie::ignore_error);
        if let Err(error) = watched {
            tracing::debug!(%error, "the X11 root window's properties go unheard");
        }
        let windows = Self {
            positions,
            atoms,
            class,
            process: ClientProcess::read(
                rustix::system::uname().nodename().to_bytes(),
                std::fs::read_link("/proc/self/ns/pid")
                    .ok()
                    .as_deref()
                    .and_then(std::path::Path::to_str),
                std::process::id(),
            ),
            synchronises: initialize_sync(connection.xcb()),
            table: RefCell::default(),
            closed: Cell::new(false),
            frames: FrameCount::default(),
            frame_inset: Arc::new(FrameInset::default()),
            support: Cell::new(WindowManagerSupport::default()),
            window_manager: Cell::new(None),
            support_unread: Cell::new(false),
            unread: Cell::new(false),
            appearance: Cell::new(Brightness::Light),
            last_focused: Cell::new(None),
            forwarded_activation: Cell::new(None),
            connection,
        };
        windows.read_support();
        windows
    }

    /// Read which window manager runs and what it supports: its supporting
    /// window, which names itself in turn while a window manager that speaks
    /// EWMH runs, and its `_NET_SUPPORTED`; without one, the owner of the
    /// screen's `WM_S<n>` selection, which every window manager holds (ICCCM
    /// §2.8). The end of the window found is heard from here on.
    fn read_support(&self) {
        let (manager, watched) = match self.extended_manager() {
            Some(check) => (WindowManager::Extended, Some(check)),
            None => match self.selection_owner() {
                Some(owner) => (WindowManager::Icccm, Some(owner)),
                None => (WindowManager::Absent, None),
            },
        };
        let previous = self.window_manager.replace(watched);
        if let Some(previous) = previous.filter(|previous| Some(*previous) != watched) {
            // The window of a window manager that is replaced, or no longer
            // names itself; one the server destroyed took its selection.
            self.forget_window_manager(previous);
        }
        let supported: Vec<u32> = self
            .connection
            .xcb()
            .get_property(
                false,
                self.connection.root(),
                self.atoms.net_supported,
                AtomEnum::ATOM,
                0,
                1024,
            )
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .and_then(|reply| Some(reply.value32()?.collect()))
            .unwrap_or_default();
        self.support.set(
            WindowManagerSupport::read(manager, &supported, &self.atoms.support())
                .with_compositing(self.compositing_manager_runs()),
        );
    }

    /// Whether a compositing manager owns the screen's `_NET_WM_CM_S<n>`
    /// selection (EWMH). Read with the window manager's support, again when a
    /// manager announces that it took the selection; a compositing manager
    /// that quits announces nothing, and the windows keep saying they cannot
    /// tell a covering until the support is next read.
    fn compositing_manager_runs(&self) -> bool {
        self.connection
            .xcb()
            .get_selection_owner(self.atoms.cm_selection)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|reply| reply.owner != x11rb::NONE)
    }

    /// The supporting window of a window manager that speaks EWMH: the one
    /// the root's `_NET_SUPPORTING_WM_CHECK` names, which names itself in
    /// turn, its end heard.
    fn extended_manager(&self) -> Option<xproto::Window> {
        let xcb = self.connection.xcb();
        let atoms = &self.atoms;
        let window_of = |window: xproto::Window| -> Option<xproto::Window> {
            xcb.get_property(
                false,
                window,
                atoms.net_supporting_wm_check,
                AtomEnum::WINDOW,
                0,
                1,
            )
            .ok()?
            .reply()
            .ok()?
            .value32()?
            .next()
        };
        let names_itself = |check: xproto::Window| window_of(check) == Some(check);
        // The property is any client's to write: the root, or a window this
        // connection made, named there is no window manager's, and selecting
        // its events would replace the events the platform hears of it.
        window_of(self.connection.root())
            .filter(|check| self.connection.is_peers(*check))
            .filter(|check| names_itself(*check))
            .filter(|check| self.hear_end_of(*check, names_itself))
    }

    /// The window that owns the screen's `WM_S<n>` selection, which a window
    /// manager holds while it runs (ICCCM §2.8), its end heard.
    fn selection_owner(&self) -> Option<xproto::Window> {
        let owner = || {
            self.connection
                .xcb()
                .get_selection_owner(self.atoms.wm_selection)
                .ok()?
                .reply()
                .ok()
                .map(|reply| reply.owner)
                .filter(|owner| *owner != x11rb::NONE)
        };
        owner()
            .filter(|owner| self.connection.is_peers(*owner))
            .filter(|found| self.hear_end_of(*found, |window| owner() == Some(window)))
    }

    /// Hear the end of `window`, a peer's, which `still` says is the window
    /// manager's: whether it is heard. The window's events are selected once
    /// per window manager. It may go, and its id be given to another client's
    /// window, between the read and the selection, so `still` reads it again
    /// after (as GDK's `fetch_net_wm_check_window` does). Its events are
    /// selected beside what the platform's other features hear there
    /// (`X11Connection::hear_peer`), and a window found not to be the window
    /// manager's is let go of again.
    fn hear_end_of(
        &self,
        window: xproto::Window,
        still: impl FnOnce(xproto::Window) -> bool,
    ) -> bool {
        if self.window_manager.get() == Some(window) {
            return true;
        }
        // A window whose end goes unheard (a peer's no longer there, a call
        // off the owner thread) names no window manager the platform follows.
        let heard = matches!(
            self.connection.hear_peer(
                window,
                PeerInterest::WindowManager,
                EventMask::STRUCTURE_NOTIFY,
            ),
            PeerHeard::Heard
        );
        if heard && still(window) {
            return true;
        }
        self.forget_window_manager(window);
        false
    }

    /// Stop hearing `window`, no longer the window manager's: what the
    /// platform's other features hear there stays selected.
    fn forget_window_manager(&self, window: xproto::Window) {
        if let PeerSelect::Sent(sent) = self
            .connection
            .forget_peer(window, PeerInterest::WindowManager)
        {
            let _: Result<(), _> = sent.map(x11rb::cookie::VoidCookie::ignore_error);
        }
    }

    /// What the window manager supports now. An event that said it may have
    /// changed left it unread: whoever asks first reads it, so nobody in or
    /// after that event's turn acts on what a window manager that went
    /// supported (asks it for the focus, publishes the mode it left).
    fn support(&self) -> WindowManagerSupport {
        if self.support_unread.take() {
            self.read_support();
        }
        self.support.get()
    }

    /// The next identity, never one given before.
    fn mint_id(&self) -> Result<WindowId, OpenWindowError> {
        Ok(self.table.borrow_mut().ids.next()?)
    }

    /// The window an X id names now.
    fn window_of(&self, xid: xproto::Window) -> Option<Rc<X11Window>> {
        let table = self.table.borrow();
        let id = table.by_xid.get(&xid)?;
        table.windows.get(id).cloned()
    }

    /// The open windows as they are now: the caller reaches each with no
    /// borrow of the table held, since a window's handler may open or close
    /// windows.
    fn open_windows(&self) -> Vec<Rc<X11Window>> {
        self.table.borrow().windows.values().cloned().collect()
    }

    /// Take a closing window out of the table.
    fn forget(&self, id: WindowId, xid: xproto::Window) {
        let window = {
            let mut table = self.table.borrow_mut();
            if table.by_xid.get(&xid) == Some(&id) {
                table.by_xid.remove(&xid);
            }
            table.windows.remove(&id)
        };
        if let Some(window) = window {
            // The table's handle goes outside its borrow: the caller holds
            // another.
            drop(window);
            self.closed.set(true);
        }
    }

    /// Open a window on the owner thread of `owner`, delivering to
    /// `handler`.
    fn open(
        self: &Arc<Self>,
        owner: &X11Loop,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<Rc<dyn HostWindow>, OpenWindowError> {
        if !owner.is_owner_thread() {
            return Err(OpenWindowError::Unavailable {
                message: "an X11 window opens on the thread that built its platform, which its \
                          handler lives on; another thread opens one through its owner"
                    .into(),
            });
        }
        // Refused before an id is taken, for what a command is refused for.
        let opening = options.geometry()?;
        let constraints = opening.constraints;
        let id = self.mint_id()?;
        let support = self.support();
        // A title bar of the application's opens with the window manager's frame
        // where the window manager cannot move one, so the plan takes that
        // frame's inset.
        let window_capabilities = capabilities(support);
        let frame = options.frame.at_open(&window_capabilities);
        let options = options.frame(frame);
        let snapshot = owner.displays().snapshot().ok();
        let plan = OpenPlan::new(
            &options,
            &opening,
            &window_capabilities,
            snapshot.as_ref(),
            |scale| self.frame_inset.logical(scale),
        );
        let scale = plan.scale;
        let (Some(width), Some(height)) = (
            device_extent(plan.content_size.width, scale),
            device_extent(plan.content_size.height, scale),
        ) else {
            return Err(OpenWindowError::Backend {
                message: format!("{:?} is not a size an X11 window takes", plan.content_size),
            });
        };
        let position = plan
            .position
            .and_then(|at| Some((i16::try_from(at.x).ok()?, i16::try_from(at.y).ok()?)));
        let xcb = self.connection.xcb();
        let failed = |error: &dyn std::fmt::Display| OpenWindowError::Backend {
            message: format!("cannot create an X11 window: {error}"),
        };
        let xid = xcb.generate_id().map_err(|error| failed(&error))?;
        let events = EventMask::STRUCTURE_NOTIFY
            | EventMask::PROPERTY_CHANGE
            | EventMask::FOCUS_CHANGE
            | EventMask::VISIBILITY_CHANGE
            | EventMask::EXPOSURE;
        xcb.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            xid,
            self.connection.root(),
            position.map_or(0, |(x, _)| x),
            position.map_or(0, |(_, y)| y),
            width,
            height,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            // No background: the server never clears what a frame drew.
            &CreateWindowAux::new()
                .event_mask(events)
                .background_pixmap(x11rb::NONE),
        )
        .map_err(|error| failed(&error))?
        .check()
        .map_err(|error| failed(&error))?;

        let window_ref = WindowRef { xid, id };
        let facets = self.positions.for_window(&window_ref);
        let capabilities = capabilities(support);
        // Until the first read of the server: the window as it was created, at
        // the place the plan put it on its display, or at none where the plan
        // placed nothing (a display the snapshot lacks, a host that does not
        // place at open) or the place left the server's 16-bit coordinates
        // and the window was created at the root's corner; an empty window
        // where that describes none.
        let placed = position.and(plan.placed);
        let initial = WindowState::new(
            WindowGeometry::try_new(placed, plan.content_size).unwrap_or(WindowGeometry::EMPTY),
            flui_platform::WindowExtent::try_new(plan.content_size).unwrap_or_default(),
        )
        .with_capabilities(capabilities);
        let opens_in = match plan.mode {
            WindowMode::Maximized if support.maximizes => WindowMode::Maximized,
            WindowMode::Fullscreen if support.fullscreen => WindowMode::Fullscreen,
            _ => WindowMode::Normal,
        };
        let input = Rc::clone(owner.input());
        let delivery = Arc::new(WindowDelivery::counting_frames(
            handler,
            input.capabilities(),
            self.frames.counter(),
        ));
        delivery.resume_through_signal(owner.signal());
        let window = Rc::new(X11Window {
            id,
            xid,
            connection: Arc::clone(&self.connection),
            atoms: self.atoms,
            windows: Arc::downgrade(self),
            delivery,
            facets,
            cell: Arc::new(WindowStateCell::new(initial)),
            redraw: Arc::new(AtomicBool::new(false)),
            doorbell: owner.doorbell().clone(),
            displays: Arc::clone(owner.displays()),
            input,
            drag: Arc::clone(owner.drag()),
            text: Rc::clone(owner.text()),
            input_scale: WindowScale::new(scale.get()),
            record: RefCell::new(Record {
                title: options.title.clone(),
                constraints,
                resizable: options.resizable,
                // No title bar of the application's here yet: one asked for
                // opens with the system's frame.
                frame: options.frame,
                shown: options.visible,
                reveal_pending: plan.hidden_until_first_frame,
                minimize_on_map: false,
                outer_to_fit: plan
                    .normal_geometry
                    .filter(|_| plan.unmapped_until_shown())
                    .map(WindowGeometry::size),
                obscured: false,
                normal_geometry: plan.normal_geometry,
                content: None,
                focus_told: false,
                bounds: None,
                bounds_told: None,
                support,
                scale,
                hints_rescaled: false,
                resize_increments: None,
                aspect_ratio: None,
            }),
            unread: Cell::new(false),
            unannounced: Cell::new(false),
            closed: Cell::new(false),
            chrome: RefCell::default(),
            counters: self
                .synchronises
                .then(|| SyncCounters::create(xcb))
                .flatten(),
            resize_sync: RefCell::default(),
            native: Cell::new(NativeWindow::Live),
        });
        owner.notices().mark_stale_on_wake(&window.cell);
        window
            .delivery
            .publish_through_owner(&window, X11Window::publish_at_the_turns_read);

        window.write_properties(position, &self.class, &self.process);
        if opens_in != WindowMode::Normal {
            window.seed_mode(opens_in);
        }
        {
            let mut table = self.table.borrow_mut();
            table.windows.insert(id, Rc::clone(&window));
            table.by_xid.insert(xid, id);
        }
        let target = InputTarget {
            window: window.window_ref(),
            delivery: Arc::clone(&window.delivery),
            scale: window.input_scale.clone(),
        };
        window.drag.window_opened(target.clone());
        window.text.window_opened(
            window.window_ref(),
            window.input_scale.clone(),
            Arc::clone(&window.facets.text).x11_text(),
        );
        window.input.window_opened(target);
        // Mapped now unless it waits for its first frame or the application
        // asked for it hidden; one opened in a mode has it in its properties
        // by now.
        if options.visible && !plan.hidden_until_first_frame {
            window.map();
        }
        if let Err(error) = xcb.flush() {
            tracing::debug!(%error, "cannot send an X11 window's opening to the server");
        }
        window.refresh_state();
        Ok(window)
    }

    /// The window an event names, and the events that may concern every
    /// window: the root window's, and the end of the window manager's own
    /// window.
    fn route(&self, event: &Event) {
        let root = self.connection.root();
        if let Event::PropertyNotify(property) = event
            && property.window == root
        {
            if self.atoms.concerns_support(property.atom) {
                self.support_unread.set(true);
                self.mark_all();
            } else if property.atom == self.atoms.net_active_window {
                self.mark_all();
            }
            return;
        }
        if let Event::ClientMessage(message) = event
            && message.window == root
            && message.type_ == self.atoms.manager
            && [self.atoms.wm_selection, self.atoms.cm_selection]
                .contains(&message.data.as_data32()[1])
        {
            // A window manager or a compositing manager took the screen's
            // selection (ICCCM §2.8, EWMH).
            self.support_unread.set(true);
            self.mark_all();
            return;
        }
        if let Event::DestroyNotify(destroyed) = event
            && Some(destroyed.window) == self.window_manager.get()
        {
            // The window manager went, with or without a word on the root
            // window: nothing it supported stands, and nothing it left on a
            // window says anything.
            self.window_manager.set(None);
            self.support_unread.set(true);
            self.mark_all();
            return;
        }
        let xid = match event {
            Event::Expose(event) => event.window,
            Event::ConfigureNotify(event) => event.window,
            Event::MapNotify(event) => event.window,
            Event::UnmapNotify(event) => event.window,
            Event::ReparentNotify(event) => event.window,
            Event::DestroyNotify(event) => event.window,
            Event::VisibilityNotify(event) => event.window,
            Event::PropertyNotify(event) => event.window,
            Event::FocusIn(event) | Event::FocusOut(event) => event.event,
            Event::ClientMessage(event) => event.window,
            _ => return,
        };
        if let Some(window) = self.window_of(xid) {
            if matches!(event, Event::FocusIn(_)) {
                self.last_focused.set(Some(window.id));
            }
            window.handle(event);
        }
    }

    /// Every window may differ from its snapshot.
    fn mark_all(&self) {
        for window in self.table.borrow().windows.values() {
            window.mark_stale();
        }
    }
}

impl X11Window {
    /// Write what the window manager reads as it maps the window: the
    /// protocols the window takes (its close, the ping, and a synchronised
    /// resize where it has counters, which it names), its title, `class` and
    /// the application id it claims, that it takes the input focus, the
    /// machine and the id of its `process` where it has them
    /// (`ClientProcess` decides), type, size hints and decorations.
    fn write_properties(
        &self,
        position: Option<(i16, i16)>,
        class: &AppClass,
        process: &ClientProcess,
    ) {
        let xcb = self.xcb();
        let atoms = &self.atoms;
        let (title, decorated) = {
            let record = self.record.borrow();
            (record.title.clone(), record.frame.has_system_title_bar())
        };
        let claimed = class.application_id().map(|app_id| {
            xcb.change_property8(
                PropMode::REPLACE,
                self.xid,
                atoms.gtk_application_id,
                atoms.utf8_string,
                app_id.as_bytes(),
            )
        });
        let protocols = [
            atoms.wm_delete_window,
            atoms.net_wm_ping,
            atoms.net_wm_sync_request,
        ];
        let protocols = match self.counters {
            Some(_) => &protocols[..],
            None => &protocols[..2],
        };
        let counters = self.counters.map(|counters| {
            xcb.change_property32(
                PropMode::REPLACE,
                self.xid,
                atoms.net_wm_sync_request_counter,
                AtomEnum::CARDINAL,
                &[counters.basic, counters.extended],
            )
        });
        let machine = process.machine().map(|machine| {
            let encoding = match machine.encoding() {
                NameEncoding::Latin1 => u32::from(AtomEnum::STRING),
                NameEncoding::Utf8 => atoms.utf8_string,
            };
            xcb.change_property8(
                PropMode::REPLACE,
                self.xid,
                AtomEnum::WM_CLIENT_MACHINE,
                encoding,
                machine.name(),
            )
        });
        let id = process.id().map(|id| {
            xcb.change_property32(
                PropMode::REPLACE,
                self.xid,
                atoms.net_wm_pid,
                AtomEnum::CARDINAL,
                &[id.get()],
            )
        });
        let written = [
            xcb.change_property32(
                PropMode::REPLACE,
                self.xid,
                AtomEnum::WM_HINTS,
                AtomEnum::WM_HINTS,
                &WM_HINTS,
            ),
            xcb.change_property32(
                PropMode::REPLACE,
                self.xid,
                atoms.wm_protocols,
                AtomEnum::ATOM,
                protocols,
            ),
            xcb.change_property8(
                PropMode::REPLACE,
                self.xid,
                AtomEnum::WM_CLASS,
                AtomEnum::STRING,
                &class.wm_class(),
            ),
            xcb.change_property32(
                PropMode::REPLACE,
                self.xid,
                atoms.net_wm_window_type,
                AtomEnum::ATOM,
                &[atoms.net_wm_window_type_normal],
            ),
        ];
        let failed = written
            .into_iter()
            .chain(claimed)
            .chain(counters)
            .chain(machine)
            .chain(id)
            .find_map(Result::err);
        if let Some(error) = failed {
            tracing::debug!(%error, "cannot set an X11 window's properties");
        }
        self.write_title(&title);
        self.write_size_hints(position);
        if !decorated {
            self.write_decorations(false);
        }
    }
}

impl OpenWindows for X11Windows {
    fn is_empty(&self) -> bool {
        self.table.borrow().windows.is_empty()
    }
}

impl LoopWindows for X11Windows {
    fn event(&self, event: &Event) {
        self.route(event);
    }

    fn read_stale(&self) {
        // Read now if no window asked for it in this turn.
        let _: WindowManagerSupport = self.support();
        if !self.unread.take() {
            return;
        }
        // Each window follows its state with no borrow of the table held:
        // its handler hears `state_changed`, and may open or close windows.
        // One handler's panic leaves the other windows read, and the first
        // goes on after the last.
        let mut failure = FirstPanic::new();
        for window in self.open_windows() {
            // The handle goes inside the containment too: it is the last one
            // of a window a handler closed meanwhile, and letting go of that
            // window lets go of its handler.
            failure.contain(move || {
                if window.unread.get() {
                    window.follow_state();
                }
            });
        }
        failure.resume();
    }

    fn chrome_event(&self, event: &Event) -> bool {
        let xid = match event {
            Event::XinputButtonPress(event)
            | Event::XinputButtonRelease(event)
            | Event::XinputMotion(event) => event.event,
            _ => return false,
        };
        self.window_of(xid)
            .is_some_and(|window| window.chrome_event(event))
    }

    /// One handler's panic leaves the other windows their frames, and the
    /// first goes on after the last.
    fn deliver_frames(&self) {
        // As `read_stale` read it earlier in the turn: no request and no
        // `tracing` here, outside every window's containment.
        let paced = self.support.get().paces_frames;
        let mut failure = FirstPanic::new();
        for window in self.open_windows() {
            // The handle goes inside the containment: it may be the last one
            // of a window its handler closed.
            failure.contain(move || window.deliver_frame(paced));
        }
        failure.resume();
    }

    fn wants_frame(&self) -> bool {
        self.table
            .borrow()
            .windows
            .values()
            .any(|window| window.redraw.load(Ordering::Acquire))
    }

    fn redraw_for_deadline(&self) {
        for window in self.open_windows() {
            window.mark_redraw();
        }
    }

    fn take_closed(&self) -> bool {
        self.closed.replace(false)
    }

    fn refresh(&self) {
        self.mark_all();
    }

    fn observe_appearance(&self, appearance: Brightness) {
        let changed = self.appearance.replace(appearance) != appearance;
        if changed {
            self.mark_all();
        }
    }

    fn frames(&self) -> u64 {
        self.frames.get()
    }

    fn release(&self) {
        // Each window is let go of with no borrow of the table held: its
        // handler drops with it.
        let windows: Vec<_> = {
            let mut table = self.table.borrow_mut();
            table.by_xid.clear();
            table.windows.drain().map(|(_, window)| window).collect()
        };
        for window in windows {
            window.release();
        }
    }

    fn shows_any(&self) -> bool {
        self.open_windows().iter().any(|window| window.shown())
    }

    /// The window that had the focus last comes forward, the one the
    /// runner routes the launch's links to; once it closed or was hidden,
    /// any shown one. Activating a window not mapped yet would not bring it
    /// forward: with none mapped, the time waits for the next window mapped,
    /// the one the application shows from the reopen it was asked, or the
    /// one waiting for its first frame.
    fn activate_forwarded(&self, token: Option<&str>) {
        let time = token
            .and_then(crate::owner::linux::single_instance::startup_time)
            .unwrap_or(x11rb::CURRENT_TIME);
        let windows = self.open_windows();
        let open = |window: &&Rc<X11Window>| window.shown();
        let last = self.last_focused.get();
        let window = windows
            .iter()
            .filter(open)
            .find(|window| Some(window.id) == last)
            .or_else(|| windows.iter().find(open));
        match window {
            Some(window) if !window.unmapped() => window.activate_at(time),
            _ => self.forwarded_activation.set(Some(time)),
        }
    }
}

impl host::Window<X11Loop> for X11Window {
    type Shared = X11Windows;
    /// A window by its X id and its identity.
    type Ref = WindowRef;

    fn open(
        owner: &X11Loop,
        shared: &Arc<X11Windows>,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<Rc<dyn HostWindow>, OpenWindowError> {
        shared.open(owner, options, handler)
    }

    fn capabilities(_: &X11Loop, shared: &X11Windows) -> WindowCapabilities {
        capabilities(shared.support())
    }

    fn frame_inset(
        _: &X11Loop,
        shared: &X11Windows,
        scale_factor: flui_foundation::geometry::DevicePixelRatio,
    ) -> Size<f64> {
        shared.frame_inset.logical(scale_factor)
    }

    /// The connection every window of the host is made on.
    fn display_connection(
        _: &X11Loop,
        shared: &X11Windows,
    ) -> Arc<dyn flui_platform::DisplayConnection> {
        Arc::new(X11Display(Arc::clone(&shared.connection)))
    }
}
