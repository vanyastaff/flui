//! The X11 platform's windows: the table the loop reaches them through, opening a window and routing the server's events to it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use flui_foundation::panic::FirstPanic;
use flui_platform::{
    Brightness, WindowGeometry, WindowHandler, WindowId, WindowIdMint, WindowMode, WindowOptions,
    WindowState,
};
use rustc_hash::FxHashMap;
use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    self, AtomEnum, ChangeWindowAttributesAux, ConnectionExt as _, CreateWindowAux, EventMask,
    PropMode, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

use super::compositing::CompositingManager;
use super::sync::{SyncCounters, initialize_sync};
use super::{NativeWindow, Record, WindowAtoms, X11Window};
use crate::host::WindowPositions;
use crate::host_window::HostWindow;
use crate::owner::OpenWindowError;
use crate::owner::OpenWindows;
use crate::owner::linux::x11::{
    InputTarget, LoopWindows, PeerHeard, PeerInterest, PeerSelect, WindowRef, WindowScale,
    X11Connection, X11Loop,
};
use crate::owner::wake_deadline::FrameCount;
use crate::owner::window_delivery::WindowDelivery;
use crate::window::linux::class::AppClass;
use crate::window::linux::x11::hints::WM_HINTS;
use crate::window::linux::x11::placement::device_extent;
use crate::window::linux::x11::protocols::{ClientProcess, NameEncoding};
use crate::window::linux::x11::support::{WindowManager, WindowManagerSupport, capabilities};
use crate::window::plan::{FrameInset, OpenPlan};
use crate::window::state::WindowStateCell;

/// The X11 windows' table: each open window by its identity, the window each
/// X id names now, and the identities issued.
#[derive(Default)]
pub(super) struct WindowTable {
    pub(super) windows: FxHashMap<WindowId, Rc<X11Window>>,
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
    pub(super) connection: Arc<X11Connection>,
    atoms: WindowAtoms,
    /// The class every window carries in `WM_CLASS`.
    class: AppClass,
    /// The machine and process every window names, read once.
    process: ClientProcess,
    /// The server has the SYNC extension, initialised on the connection: each
    /// window makes the counters a synchronised resize waits on.
    synchronises: bool,
    /// The screen's compositing manager, whose coming and going changes
    /// whether a window tells a covering.
    compositing: CompositingManager,
    pub(super) table: RefCell<WindowTable>,
    /// A window closed since the loop last asked.
    closed: Cell<bool>,
    frames: FrameCount,
    pub(super) frame_inset: Arc<FrameInset>,
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
    pub(super) unread: Cell<bool>,
    pub(super) appearance: Cell<Brightness>,
    /// The window that had the focus last, whether or not it still has: the
    /// one a forwarded launch brings forward.
    last_focused: Cell<Option<WindowId>>,
    /// The time of a launch forwarded while no window was shown: the next
    /// window mapped comes forward at it, as the window the application
    /// shows from the reopen it was asked.
    pub(super) forwarded_activation: Cell<Option<u32>>,
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
        let compositing =
            CompositingManager::watch(connection.xcb(), connection.root(), atoms.cm_selection);
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
            compositing,
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
        // Read with the support, which every change of the compositing
        // manager's selection leaves unread (`route`).
        self.support.set(
            WindowManagerSupport::read(manager, &supported, &self.atoms.support())
                .with_compositing(self.compositing.runs(self.connection.xcb())),
        );
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
    pub(super) fn support(&self) -> WindowManagerSupport {
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
    pub(super) fn forget(&self, id: WindowId, xid: xproto::Window) {
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
    pub(super) fn open(
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
            input_scale: WindowScale::new(scale),
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
                mode: opens_in,
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
        if self.compositing.changed_by(event) {
            // A compositing manager took the selection, gave it up or went,
            // which announces nothing.
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
        self.mark_waits_over(Instant::now());
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

    fn mode_deadline(&self) -> Option<Instant> {
        self.earliest_mode_deadline()
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
