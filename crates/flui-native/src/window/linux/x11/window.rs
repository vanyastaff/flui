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
//! unmap, a property, the focus, the visibility) marks the snapshot stale and
//! publishes it again, and the handler hears `state_changed` for a new
//! revision. What a window honours follows what the window manager supports
//! (`support`), so a window on a server with no window manager neither
//! claims to minimize nor waits for a maximize nobody carries out.
//!
//! # Commands
//!
//! The server carries out the title, the place, the size, the limits
//! (`WM_NORMAL_HINTS`), the decorations (`_MOTIF_WM_HINTS`), showing and
//! hiding (mapping and withdrawing); the window manager the modes, asked with
//! the EWMH client messages, or, for a window it does not manage yet (one not
//! mapped), in the window's own `_NET_WM_STATE`, which it reads when it maps
//! the window. A window opened in a mode is mapped only once that property
//! holds it, so it never appears in normal mode first.
//!
//! # Frames
//!
//! A frame request from the owner thread or from any other through the
//! window's redraw requester marks the window and rings the loop's doorbell;
//! the loop delivers the frame on its next turn. An `Expose` asks for one, as
//! does a change of the content size.
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

use std::cell::{Cell, RefCell};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use cursor_icon::CursorIcon;
use flui_foundation::geometry::{Point, Size, WindowMetrics};
use flui_platform::{
    AdmittedCommand, ApplyError, CloseDecision, CursorError, DisplaySnapshot, PlatformTextInput,
    PlatformWindow, SizeConstraints, Unsupported, UnsupportedReason, WindowAppearance,
    WindowCapabilities, WindowCommand, WindowGeometry, WindowHandler, WindowId, WindowMode,
    WindowOptions, WindowState,
};
use flui_semantics::platform::PlatformAccessibility;
use parking_lot::Mutex;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle, XcbDisplayHandle, XcbWindowHandle,
};
use x11rb::connection::Connection as _;
use x11rb::properties::{WmSizeHints, WmSizeHintsSpecification};
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    self, AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConfigureWindowAux,
    ConnectionExt as _, CreateWindowAux, EventMask, InputFocus, MapState, PropMode, StackMode,
    UnmapNotifyEvent, Visibility, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

use super::hints::{motif_hints, size_limits};
use super::mode::{NetWmState, NetWmStateAtoms, WmState, mode_flags, with_mode};
use super::placement::{
    FrameExtents, device_coordinate, device_extent, display_of, is_scale, logical_geometry,
    outer_bounds, window_scale,
};
use super::support::{SupportAtoms, WindowManagerSupport, capabilities};
use crate::host::{self, WindowFacets, WindowPositions};
use crate::host_window::HostWindow;
use crate::owner::OpenWindowError;
use crate::owner::handlers::OpenWindows;
use crate::owner::linux::x11::{
    Doorbell, HostDisplays, InputTarget, LoopInput, LoopWindows, WindowRef, WindowScale,
    X11Connection, X11Loop,
};
use crate::owner::slot::{OwnerMap, OwnerWeak};
use crate::owner::wake_deadline::FrameCount;
use crate::owner::window_delivery::{Publication, Retirement, WindowDelivery};
use crate::text::WindowTextInput;
use crate::window::plan::{FrameInset, OpenPlan, PlacementDisplay, WindowShell};
use crate::window::state::{ReportedFacts, WindowStateCell, mode_of, not_carried_out};

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
    net_active_window: xproto::Atom,
    net_frame_extents: xproto::Atom,
    net_request_frame_extents: xproto::Atom,
    net_supported: xproto::Atom,
    net_supporting_wm_check: xproto::Atom,
    net_wm_window_type: xproto::Atom,
    net_wm_window_type_normal: xproto::Atom,
    motif_wm_hints: xproto::Atom,
}

impl WindowAtoms {
    /// Intern the atoms on `connection`: every request first, then every
    /// reply.
    ///
    /// # Errors
    /// [`PlatformError::Init`](crate::PlatformError::Init) when the server
    /// does not answer.
    pub(crate) fn intern(connection: &X11Connection) -> Result<Self, crate::PlatformError> {
        const NAMES: [&[u8]; 20] = [
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
            b"_NET_ACTIVE_WINDOW",
            b"_NET_FRAME_EXTENTS",
            b"_NET_REQUEST_FRAME_EXTENTS",
            b"_NET_SUPPORTED",
            b"_NET_SUPPORTING_WM_CHECK",
            b"_NET_WM_WINDOW_TYPE",
            b"_NET_WM_WINDOW_TYPE_NORMAL",
            b"_MOTIF_WM_HINTS",
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
        let mut atoms = [0; NAMES.len()];
        for (atom, cookie) in atoms.iter_mut().zip(cookies) {
            *atom = cookie.reply().map_err(|error| failed(&error))?.atom;
        }
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
            net_active_window,
            net_frame_extents,
            net_request_frame_extents,
            net_supported,
            net_supporting_wm_check,
            net_wm_window_type,
            net_wm_window_type_normal,
            motif_wm_hints,
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
            net_active_window,
            net_frame_extents,
            net_request_frame_extents,
            net_supported,
            net_supporting_wm_check,
            net_wm_window_type,
            net_wm_window_type_normal,
            motif_wm_hints,
        })
    }

    /// The `_NET_SUPPORTED` atoms the window's capabilities read.
    fn support(&self) -> SupportAtoms {
        SupportAtoms {
            maximized_horz: self.states.maximized_horz,
            maximized_vert: self.states.maximized_vert,
            fullscreen: self.states.fullscreen,
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

/// What a window keeps of what it was asked, which the server does not hold
/// in a form it reads back.
#[derive(Debug)]
struct Record {
    title: String,
    constraints: SizeConstraints,
    resizable: bool,
    decorated: bool,
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
    /// The window manager's support last read, kept for a window whose
    /// platform is gone.
    support: WindowManagerSupport,
    /// The scale of the display the window was last read on, or opened on.
    scale: f64,
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
    /// What the table's positions made for the window: its AT-SPI bridge, and
    /// the text, haptics and chrome X11 does not have yet or at all.
    facets: WindowFacets,
    cell: Arc<WindowStateCell>,
    /// A frame was asked for and not delivered yet.
    redraw: Arc<AtomicBool>,
    doorbell: Doorbell,
    displays: Arc<dyn HostDisplays>,
    /// The windows' input, which delivers this window's pointers and keys
    /// and shows its cursor.
    input: Arc<dyn LoopInput>,
    /// The scale the input reads this window's positions at.
    input_scale: WindowScale,
    record: RefCell<Record>,
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
            Some(windows) => *windows.support.lock(),
            None => self.record.borrow().support,
        }
    }

    /// The colour scheme the system applies, as the platform last heard it.
    fn appearance(&self) -> WindowAppearance {
        self.windows
            .upgrade()
            .map_or(WindowAppearance::Light, |windows| {
                *windows.appearance.lock()
            })
    }

    /// The display snapshot, or none when it cannot be read.
    fn display_snapshot(&self) -> Option<DisplaySnapshot> {
        self.displays.snapshot().ok()
    }

    /// Read the window again for its state publisher, announcing nothing:
    /// `WindowDelivery::dispatch_state_changed` calls `state_changed`.
    fn read_state(&self) -> Publication {
        self.cell.invalidate();
        self.cell.state_with(|| self.sample()).outcome()
    }

    /// The window may differ from its last snapshot: read it again, raise
    /// `state_changed` for a new revision, tell the bridge of a change of
    /// focus, and ask for a frame when the content changed size.
    fn refresh_state(&self) {
        self.cell.invalidate();
        let state = PlatformWindow::state(self);
        let (focus_changed, resized) = {
            let mut record = self.record.borrow_mut();
            let focus_changed =
                std::mem::replace(&mut record.focus_told, state.focused) != state.focused;
            // A change of a size known before: the first one an `Expose`
            // paints.
            let content = state.metrics.map(WindowMetrics::physical);
            let resized =
                record.content.is_some() && content.is_some() && record.content != content;
            if content.is_some() {
                record.content = content;
            }
            (focus_changed, resized)
        };
        if focus_changed {
            self.facets.accessibility.focus_changed(state.focused);
        }
        if resized {
            self.request_redraw();
        }
    }

    /// The window as the server and the window manager hold it now.
    fn sample(&self) -> WindowState {
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
            state.focused = false;
            state.active = false;
            return state;
        };
        let outer = outer_bounds(read.origin, read.content, read.frame);
        let display = snapshot
            .as_ref()
            .and_then(|snapshot| display_of(snapshot, outer).or_else(|| snapshot.primary()));
        let scale = window_scale(display, record.scale);
        record.scale = scale;
        self.input_scale.set(scale);
        // Remembered for a window whose platform is gone.
        record.support = support;
        let mode = mode_of(mode_flags(read.wm_state, read.net_state));
        let viewable = read.map_state == MapState::VIEWABLE;
        let visible = (record.reveal_pending || viewable)
            && !record.obscured
            && mode != WindowMode::Minimized;
        let geometry = logical_geometry(outer, scale);
        let content_size = Size::new(
            f64::from(read.content.width) / scale,
            f64::from(read.content.height) / scale,
        );
        let normal_geometry = match mode {
            WindowMode::Normal => {
                record.normal_geometry = Some(geometry);
                None
            }
            _ => record.normal_geometry,
        };
        if mode == WindowMode::Normal
            && record.decorated
            && let Some(windows) = self.windows.upgrade()
        {
            windows.frame_inset.record(read.frame.added());
        }
        let metrics = WindowMetrics::new(read.content, scale).ok();
        let state = WindowState::new(geometry, content_size)
            .with_mode(mode)
            .with_normal_geometry(normal_geometry)
            .with_visible(visible)
            .with_focused(read.focused)
            .with_title(Some(record.title.clone()))
            .with_constraints(record.constraints)
            .with_resizable(record.resizable)
            .with_decorated(record.decorated)
            .with_display(display.map(|display| display.id));
        ReportedFacts {
            metrics,
            appearance: self.appearance(),
            safe_area: flui_foundation::geometry::EdgeInsets::ZERO,
            execution: flui_platform::WindowExecutionState::Running,
            hovered: self.delivery.hovered(),
            active: if support.managed {
                read.active
            } else {
                read.focused
            },
            capabilities,
        }
        .onto(state)
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
            reply.value32().map(Iterator::collect).unwrap_or_default()
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
            focused: focus == self.xid,
            active: active.first() == Some(&self.xid),
        })
    }

    /// The scale of the display the window was last read on: the primary's
    /// when it is on none, the one it opened on before it was read.
    fn scale(&self) -> f64 {
        self.record.borrow().scale
    }

    /// The content size of the window now, in device pixels.
    fn content_device(&self) -> Size<i32> {
        self.xcb()
            .get_geometry(self.xid)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .map_or(Size::new(1, 1), |geometry| {
                Size::new(i32::from(geometry.width), i32::from(geometry.height))
            })
    }

    /// Write `WM_NORMAL_HINTS` from the limits the window keeps, and the
    /// place it opens at, `position`, in device pixels.
    fn write_size_hints(&self, position: Option<(i16, i16)>) {
        let (constraints, resizable) = {
            let record = self.record.borrow();
            (record.constraints, record.resizable)
        };
        let limits = size_limits(constraints, resizable, self.content_device(), self.scale());
        let hints = WmSizeHints {
            position: position.map(|(x, y)| {
                (
                    WmSizeHintsSpecification::ProgramSpecified,
                    i32::from(x),
                    i32::from(y),
                )
            }),
            min_size: limits.min,
            max_size: limits.max,
            win_gravity: Some(xproto::Gravity::NORTH_WEST),
            ..WmSizeHints::default()
        };
        if let Err(error) = hints.set_normal_hints(self.xcb(), self.xid) {
            tracing::debug!(%error, "cannot set an X11 window's size hints");
        }
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

    /// Whether the window is unmapped and waits to be shown: the window
    /// manager does not manage it, and reads a mode from its properties when
    /// it maps it.
    fn unmapped(&self) -> bool {
        let record = self.record.borrow();
        record.reveal_pending || !record.shown
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
        let current: Vec<u32> = self
            .xcb()
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
            .unwrap_or_default();
        if let Some(atoms) = with_mode(&current, mode, &self.atoms.states)
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
        let minimized = self.cell.published().mode == WindowMode::Minimized;
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
    /// minimized after when that is what it was last asked to be.
    fn map(&self) {
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
        }
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
            || !self.record.borrow().decorated
            || !self.support().managed
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
        if was_unmapped || self.cell.published().mode == WindowMode::Minimized {
            self.map();
        }
        self.activate();
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

    /// Give the window the focus: through the window manager where one runs
    /// (`_NET_ACTIVE_WINDOW`, which also raises it), else from the server
    /// directly.
    fn activate(&self) {
        if self.support().managed {
            self.send_to_root(ClientMessageEvent::new(
                32,
                self.xid,
                self.atoms.net_active_window,
                [SOURCE_APPLICATION, x11rb::CURRENT_TIME, 0, 0, 0],
            ));
            return;
        }
        let raised = self.xcb().configure_window(
            self.xid,
            &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
        );
        let focused = self
            .xcb()
            .set_input_focus(InputFocus::PARENT, self.xid, x11rb::CURRENT_TIME);
        if let Err(error) = raised.and(focused) {
            tracing::debug!(%error, "cannot focus an X11 window");
        }
    }

    /// Set the title in `_NET_WM_NAME` (UTF-8) and `WM_NAME`, which older
    /// window managers read.
    fn write_title(&self, title: &str) {
        let net_name = self.xcb().change_property8(
            PropMode::REPLACE,
            self.xid,
            self.atoms.net_wm_name,
            self.atoms.utf8_string,
            title.as_bytes(),
        );
        let name = self.xcb().change_property8(
            PropMode::REPLACE,
            self.xid,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            title.as_bytes(),
        );
        if let Err(error) = net_name.and(name) {
            tracing::debug!(%error, "cannot set an X11 window's title");
        }
    }

    /// Set `_MOTIF_WM_HINTS` to `decorated`.
    fn write_decorations(&self, decorated: bool) {
        if let Err(error) = self.xcb().change_property32(
            PropMode::REPLACE,
            self.xid,
            self.atoms.motif_wm_hints,
            self.atoms.motif_wm_hints,
            &motif_hints(decorated),
        ) {
            tracing::debug!(%error, "cannot set an X11 window's decorations");
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
        let mode = self.cell.published().mode;
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

    /// Move the window's outer top-left corner to `at`, in logical pixels.
    /// With the north-west gravity its hints give, the window manager places
    /// the frame there (ICCCM 4.1.2.3).
    fn place(&self, at: Point<f64>) -> Result<(), ApplyError> {
        let scale = self.scale();
        let (Some(x), Some(y)) = (
            device_coordinate(at.x, scale),
            device_coordinate(at.y, scale),
        ) else {
            return Err(ApplyError::failed(format!(
                "{at:?} is beyond the 16-bit coordinates of X11"
            )));
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
        // The visible effect does not wait for the handler.
        if self.native.get() == NativeWindow::Live
            && let Err(error) = self.xcb().unmap_window(self.xid)
        {
            tracing::debug!(%error, "cannot unmap a closing X11 window");
        }
        self.delivery.dispatch_close();
        if let Some(windows) = self.windows.upgrade() {
            windows.forget(self.id, self.xid);
        }
        self.facets.text.detach();
        self.facets.accessibility.detach();
        self.input.window_closed(self.window_ref());
        self.retire();
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
    fn retire(&self) {
        let destroy = match self.native.replace(NativeWindow::Released) {
            NativeWindow::Released => return,
            NativeWindow::Live => true,
            NativeWindow::DestroyedByServer => false,
        };
        let connection = Arc::clone(&self.connection);
        let xid = self.xid;
        self.delivery.clear_then(move || {
            if !destroy {
                return;
            }
            let destroyed = connection.xcb().destroy_window(xid);
            if let Err(error) = destroyed.and_then(|_| connection.xcb().flush()) {
                tracing::debug!(%error, "cannot destroy an X11 window");
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
                self.refresh_state();
            }
            Event::VisibilityNotify(visibility) => {
                self.record.borrow_mut().obscured = visibility.state == Visibility::FULLY_OBSCURED;
                self.refresh_state();
            }
            Event::PropertyNotify(property) if self.atoms.concerns_window(property.atom) => {
                self.refresh_state();
            }
            Event::ClientMessage(message)
                if message.type_ == self.atoms.wm_protocols
                    && message.data.as_data32()[0] == self.atoms.wm_delete_window =>
            {
                self.close_requested();
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
            Event::ConfigureNotify(_)
            | Event::UnmapNotify(_)
            | Event::ReparentNotify(_)
            | Event::FocusIn(_)
            | Event::FocusOut(_) => self.refresh_state(),
            _ => {}
        }
    }

    /// Let go of the window at the platform's end: closed to commands, its
    /// handler released and its X window destroyed after, without `closed`,
    /// which only a close delivers.
    fn release(&self) {
        self.closed.set(true);
        self.input.window_closed(self.window_ref());
        self.retire();
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
    focused: bool,
    active: bool,
}

impl Drop for X11Window {
    fn drop(&mut self) {
        // The handler may own the window's renderer, whose surface names the
        // window: it goes first, then the X window.
        self.closed.set(true);
        self.retire();
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
    #[expect(
        unsafe_code,
        reason = "a raw display handle is borrowed for as long as the target lives"
    )]
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        if self.retirement.is_retired() {
            return Err(HandleError::Unavailable);
        }
        let connection = std::ptr::NonNull::new(self.connection.xcb().get_raw_xcb_connection());
        let screen =
            i32::try_from(self.connection.screen_number()).map_err(|_| HandleError::Unavailable)?;
        // SAFETY: the target holds the connection (`Arc<X11Connection>`), so
        // the libxcb connection the pointer names stays open for as long as
        // the borrow, which cannot outlive the target.
        Ok(unsafe {
            DisplayHandle::borrow_raw(RawDisplayHandle::Xcb(XcbDisplayHandle::new(
                connection, screen,
            )))
        })
    }
}

impl flui_platform::SurfaceTarget for X11Surface {
    /// X11 has no frame callback to arm: a present is paced by the swap
    /// chain, and this returns at once. The swap chain's completion events go
    /// to the GPU driver's own connection, so the host learns nothing of when
    /// the frame is shown.
    fn pre_present_notify(&self, _: flui_platform::SwapChainStatistics) {}
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

    fn state(&self) -> WindowState {
        let published = self.cell.state_with(|| self.sample());
        if published.changed {
            self.delivery.dispatch_state_changed();
        }
        published.state
    }

    fn apply(&self, command: AdmittedCommand) -> Result<(), ApplyError> {
        if self.closed.get() {
            return Err(ApplyError::Closed);
        }
        let command = command.against(&self.cell.published_capabilities())?;
        match command {
            WindowCommand::SetTitle(title) => {
                self.write_title(&title);
                self.record.borrow_mut().title = title;
            }
            WindowCommand::SetPosition(at) => self.place(at)?,
            WindowCommand::SetSize(size) => self.resize(size)?,
            WindowCommand::SetConstraints(constraints) => {
                self.record.borrow_mut().constraints = constraints;
                self.write_size_hints(None);
            }
            WindowCommand::SetResizable(resizable) => {
                self.record.borrow_mut().resizable = resizable;
                self.write_size_hints(None);
            }
            WindowCommand::SetDecorated(decorated) => {
                self.record.borrow_mut().decorated = decorated;
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

    /// The pointer and its cursor are the input feature's: it loads the
    /// cursor from the theme and sets it on the window.
    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        if self.closed.get() {
            return Err(Unsupported::new(UnsupportedReason::NoWindow).into());
        }
        self.input.set_cursor(self.window_ref(), cursor)
    }

    /// X11 takes no refresh-rate request.
    fn set_frame_rate_hint(&self, _hint: flui_platform::FrameRateHint) -> Result<(), Unsupported> {
        Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform))
    }

    /// The input regions are the system's edge gestures a region keeps out,
    /// and X11 has none.
    fn set_input_regions(
        &self,
        _regions: &[flui_platform::input_regions::InputRegion],
    ) -> Result<(), Unsupported> {
        Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform))
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        self.facets
            .text
            .window_text_input()
            .and_then(WindowTextInput::ime)
    }

    fn haptics(&self) -> Option<Arc<dyn flui_platform::PlatformHaptics>> {
        Some(Arc::clone(&self.facets.haptics))
    }

    fn system_chrome(&self) -> Option<Arc<dyn flui_platform::PlatformSystemChrome>> {
        Some(Arc::clone(&self.facets.chrome))
    }

    /// No OS drag session starts from an X11 window yet; its input
    /// capabilities say so.
    fn drag_source(&self) -> Option<Arc<dyn flui_platform::drag::PlatformDragSource>> {
        None
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

/// The platform's X11 windows: what they open with and share, and the table
/// the loop reaches them through.
///
/// Built on the thread that builds the platform, which owns the windows: the
/// table answers there alone. Public where it is defined, as the host's table
/// names it, and nameable only inside the crate.
pub struct X11Windows {
    positions: WindowPositions<WindowRef>,
    connection: Arc<X11Connection>,
    atoms: WindowAtoms,
    windows: OwnerMap<WindowId, Rc<X11Window>>,
    /// The window each X id names now.
    by_xid: OwnerMap<xproto::Window, WindowId>,
    /// The last identity given; never given again.
    last_id: AtomicU64,
    /// A window closed since the loop last asked.
    closed: AtomicBool,
    frames: FrameCount,
    frame_inset: Arc<FrameInset>,
    support: Mutex<WindowManagerSupport>,
    appearance: Mutex<WindowAppearance>,
}

impl std::fmt::Debug for X11Windows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("X11Windows").finish_non_exhaustive()
    }
}

impl X11Windows {
    /// The windows of `connection`, each handing out what `positions` make,
    /// with the atoms interned on it. Hears the root window's property changes
    /// from here on (the window manager's support and the active window),
    /// and reads what the window manager supports.
    pub(crate) fn new(
        positions: WindowPositions<WindowRef>,
        connection: Arc<X11Connection>,
        atoms: WindowAtoms,
    ) -> Self {
        // The root window's mask is this connection's: a feature that needs
        // more of the root's events adds them here, since a second
        // `ChangeWindowAttributes` replaces the first.
        let watched = connection
            .xcb()
            .change_window_attributes(
                connection.root(),
                &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE),
            )
            .map(x11rb::cookie::VoidCookie::ignore_error);
        if let Err(error) = watched {
            tracing::debug!(%error, "the X11 root window's properties go unheard");
        }
        let windows = Self {
            positions,
            atoms,
            windows: OwnerMap::new(),
            by_xid: OwnerMap::new(),
            last_id: AtomicU64::new(0),
            closed: AtomicBool::new(false),
            frames: FrameCount::default(),
            frame_inset: Arc::new(FrameInset::default()),
            support: Mutex::new(WindowManagerSupport::default()),
            appearance: Mutex::new(WindowAppearance::Light),
            connection,
        };
        windows.read_support();
        windows
    }

    /// Read what the window manager supports from the root window: its
    /// supporting window, which names itself in turn while the window manager
    /// runs, and its `_NET_SUPPORTED`.
    fn read_support(&self) {
        let xcb = self.connection.xcb();
        let root = self.connection.root();
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
        let managed = window_of(root).is_some_and(|check| window_of(check) == Some(check));
        let supported: Vec<u32> = xcb
            .get_property(false, root, atoms.net_supported, AtomEnum::ATOM, 0, 1024)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .and_then(|reply| Some(reply.value32()?.collect()))
            .unwrap_or_default();
        *self.support.lock() = WindowManagerSupport::read(managed, &supported, &atoms.support());
    }

    /// The next identity, never one given before.
    fn mint_id(&self) -> Result<WindowId, OpenWindowError> {
        self.last_id
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
                last.checked_add(1)
            })
            .map(|last| WindowId(last + 1))
            .map_err(|_| OpenWindowError::Backend {
                message: "this X11 platform has given every window identity".into(),
            })
    }

    /// The window an X id names now.
    fn window_of(&self, xid: xproto::Window) -> Option<Rc<X11Window>> {
        let id = self.by_xid.get(&xid)?;
        self.windows.get(&id)
    }

    /// Take a closing window out of the table.
    fn forget(&self, id: WindowId, xid: xproto::Window) {
        if self.by_xid.get(&xid) == Some(id) {
            self.by_xid.remove(&xid);
        }
        if let Some(window) = self.windows.remove(&id) {
            // The table's handle goes outside its borrow: the caller holds
            // another.
            drop(window);
            self.closed.store(true, Ordering::Release);
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
        let id = self.mint_id()?;
        let support = *self.support.lock();
        let snapshot = owner.displays().snapshot().ok();
        let placement = PlacementDisplay::choose(snapshot.as_ref(), options.display);
        let plan = OpenPlan::new(
            &options,
            WindowShell::Desktop,
            placement,
            self.frame_inset.logical(placement.scale_factor),
        );
        let scale = Some(placement.scale_factor)
            .filter(|scale| is_scale(*scale))
            .unwrap_or(1.0);
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
        let initial = WindowState::new(
            WindowGeometry::new(options.position, plan.content_size),
            plan.content_size,
        )
        .with_capabilities(capabilities);
        let opens_in = match plan.mode {
            WindowMode::Maximized if support.maximizes => WindowMode::Maximized,
            WindowMode::Fullscreen if support.fullscreen => WindowMode::Fullscreen,
            _ => WindowMode::Normal,
        };
        let input = Arc::clone(owner.input());
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
            input_scale: WindowScale::new(scale),
            record: RefCell::new(Record {
                title: options.title.clone(),
                constraints: SizeConstraints::new(options.min_size, options.max_size),
                resizable: options.resizable,
                decorated: options.decorated,
                shown: options.visible,
                reveal_pending: plan.hidden_until_first_frame,
                minimize_on_map: false,
                outer_to_fit: plan
                    .normal_geometry
                    .filter(|_| plan.unmapped_until_shown())
                    .map(|geometry| geometry.size),
                obscured: false,
                normal_geometry: plan.normal_geometry,
                content: None,
                focus_told: false,
                support,
                scale,
            }),
            closed: Cell::new(false),
            native: Cell::new(NativeWindow::Live),
        });
        owner.notices().mark_stale_on_wake(&window.cell);
        window
            .delivery
            .publish_through_owner(&window, X11Window::read_state);

        window.write_properties(position);
        if opens_in != WindowMode::Normal {
            window.seed_mode(opens_in);
        }
        if self.windows.insert(id, Rc::clone(&window)).is_err()
            || self.by_xid.insert(xid, id).is_err()
        {
            window.release();
            return Err(OpenWindowError::Unavailable {
                message: "the X11 window table answers on its owner thread alone".into(),
            });
        }
        window.input.window_opened(InputTarget {
            window: window.window_ref(),
            delivery: Arc::clone(&window.delivery),
            scale: window.input_scale.clone(),
        });
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

    /// The window an event names, and the root window's events, which may
    /// concern every window.
    fn route(&self, event: &Event) {
        let root = self.connection.root();
        if let Event::PropertyNotify(property) = event
            && property.window == root
        {
            if self.atoms.concerns_support(property.atom) {
                self.read_support();
                self.refresh_all();
            } else if property.atom == self.atoms.net_active_window {
                self.refresh_all();
            }
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
            window.handle(event);
        }
    }

    /// Every window reads itself again.
    fn refresh_all(&self) {
        for window in self.windows.values() {
            window.refresh_state();
        }
    }
}

impl X11Window {
    /// Write what the window manager reads as it maps the window: the
    /// protocols the window takes, its title, class, process, type, size
    /// hints and decorations.
    fn write_properties(&self, position: Option<(i16, i16)>) {
        let xcb = self.xcb();
        let atoms = &self.atoms;
        let (title, decorated) = {
            let record = self.record.borrow();
            (record.title.clone(), record.decorated)
        };
        let class = std::env::current_exe()
            .ok()
            .and_then(|path| {
                path.file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "flui".to_owned());
        // WM_CLASS: the instance, then the class, each ended by a zero.
        let wm_class = format!("{class}\0{class}\0");
        let written = [
            xcb.change_property32(
                PropMode::REPLACE,
                self.xid,
                atoms.wm_protocols,
                AtomEnum::ATOM,
                &[atoms.wm_delete_window],
            ),
            xcb.change_property8(
                PropMode::REPLACE,
                self.xid,
                AtomEnum::WM_CLASS,
                AtomEnum::STRING,
                wm_class.as_bytes(),
            ),
            xcb.change_property32(
                PropMode::REPLACE,
                self.xid,
                atoms.net_wm_pid,
                AtomEnum::CARDINAL,
                &[std::process::id()],
            ),
            xcb.change_property32(
                PropMode::REPLACE,
                self.xid,
                atoms.net_wm_window_type,
                AtomEnum::ATOM,
                &[atoms.net_wm_window_type_normal],
            ),
        ];
        if let Some(error) = written.into_iter().find_map(Result::err) {
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
        self.windows.is_empty()
    }
}

impl LoopWindows for X11Windows {
    fn event(&self, event: &Event) {
        self.route(event);
    }

    fn deliver_frames(&self) {
        for window in self.windows.values() {
            if window.take_redraw() && !window.closed.get() {
                window.delivery.dispatch_request_frame();
            }
        }
    }

    fn wants_frame(&self) -> bool {
        self.windows
            .values()
            .iter()
            .any(|window| window.redraw.load(Ordering::Acquire))
    }

    fn redraw_for_deadline(&self) {
        for window in self.windows.values() {
            window.mark_redraw();
        }
    }

    fn take_closed(&self) -> bool {
        self.closed.swap(false, Ordering::AcqRel)
    }

    fn refresh(&self) {
        self.refresh_all();
    }

    fn observe_appearance(&self, appearance: WindowAppearance) {
        let changed = std::mem::replace(&mut *self.appearance.lock(), appearance) != appearance;
        if changed {
            self.refresh_all();
        }
    }

    fn frames(&self) -> u64 {
        self.frames.get()
    }

    fn release(&self) {
        let windows: Vec<_> = self.windows.entries();
        for (id, window) in windows {
            self.by_xid.remove(&window.xid);
            self.windows.remove(&id);
            window.release();
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

    /// The window that has the input focus.
    fn active(_: &X11Loop, shared: &X11Windows) -> Option<WindowId> {
        shared
            .windows
            .values()
            .into_iter()
            .find(|window| PlatformWindow::state(&**window).focused)
            .map(|window| window.id)
    }

    fn capabilities(_: &X11Loop, shared: &X11Windows) -> WindowCapabilities {
        capabilities(*shared.support.lock())
    }

    fn frame_inset(_: &X11Loop, shared: &X11Windows, scale_factor: f64) -> Size<f64> {
        shared.frame_inset.logical(scale_factor)
    }
}
