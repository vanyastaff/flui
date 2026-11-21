//! What the loop and its windows ask of the seats' input, the clipboard and drag and drop, which the table registers.

use std::any::Any;
use std::rc::Rc;
use std::sync::{Arc, Weak};

use cursor_icon::CursorIcon;
use flui_platform::transfer::data::DragTransport;
use flui_platform::transfer::drag::PlatformDragSource;
use flui_platform::{Cursor, CursorError, CursorGrab, PlatformFailure, WindowId};
use wayland_client::backend::ObjectId;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, QueueHandle};
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;
use web_time::Instant;

use super::WaylandState;
use crate::host;
use crate::owner::linux::wayland::PointerOffer;
use crate::owner::window_delivery::WindowDelivery;
#[cfg(doc)]
use crate::platform::Platform;

/// A window as the seats' input reaches it, from its open to its close:
/// its identity, the surface the compositor names it by, and the delivery
/// its input goes through.
pub(crate) struct SeatWindow {
    /// The window's identity.
    pub(crate) id: WindowId,
    /// Its `wl_surface`.
    pub(crate) surface: WlSurface,
    /// What its input is delivered through.
    pub(crate) delivery: Arc<WindowDelivery>,
    /// Its device pixels per logical pixel now, which a theme cursor
    /// over it is drawn at.
    pub(crate) scale: Arc<dyn Fn() -> f64 + Send + Sync>,
    /// The frame it draws itself where the compositor draws none, which
    /// the pointer over the frame's surfaces reaches instead of the
    /// window's input.
    pub(crate) frame: Rc<dyn FramePointer>,
    /// Its text, which a seat's `zwp_text_input_v3` in the window drives;
    /// `None` where its text input is no Wayland one.
    pub(crate) text: Option<Arc<dyn SeatWindowText>>,
}

/// A window's own frame as the seats' input reaches it: the window
/// feature's, which draws it. The input asks it whether a surface is the
/// frame's and tells it what each seat's pointer does there; the frame
/// records it seat by seat, and the window carries it out as it settles.
/// Called by a dispatch on the owner thread, with no cell of the input
/// borrowed; it runs no handler and calls back into nothing.
pub(crate) trait FramePointer {
    /// Whether `surface` is one of the frame's.
    fn holds(&self, surface: &WlSurface) -> bool;

    /// `seat`'s pointer is at (`x`, `y`), surface-local, on `surface`,
    /// one of the frame's: the cursor it shows there.
    fn moved(&self, seat: &WlSeat, surface: &WlSurface, x: f64, y: f64) -> CursorIcon;

    /// `seat`'s pointer left the frame's surfaces.
    fn left(&self, seat: &WlSeat);

    /// Button `code` (evdev) of `seat`'s pointer went down or up over the
    /// frame at `time` (the compositor's milliseconds), as the event
    /// numbered `serial`: a move, a resize or a window menu it starts
    /// names that seat and serial.
    fn button(&self, seat: &WlSeat, serial: u32, time: u32, code: u32, pressed: bool);

    /// `seat`'s pointer is at (`x`, `y`), surface-local, on the window's
    /// own surface: a press on a title bar of the application's is
    /// answered from there, and a press on it held moves the window once
    /// the pointer travels (ADR-0286 §11).
    fn content_moved(&self, seat: &WlSeat, x: f64, y: f64);

    /// Button `code` (evdev) of `seat`'s pointer went down or up over the
    /// window's own surface, as the event numbered `serial`: whether the
    /// window's chrome took it, which keeps it from the window's input.
    /// The release of a press it took is taken too.
    fn content_button(&self, seat: &WlSeat, serial: u32, code: u32, pressed: bool) -> bool;

    /// `seat` went, or its pointer did: what the frame kept of that
    /// pointer goes with it, and an action it asked for is not carried
    /// out.
    fn seat_gone(&self, seat: &WlSeat);
}

/// What one seat's `zwp_text_input_v3` reported, recorded in the seats'
/// queue in order with the keys and pointers around it.
#[derive(Debug)]
pub(crate) struct TextWire {
    /// The `zwp_text_input_v3` it came from.
    pub(crate) input: ObjectId,
    pub(crate) event: TextEvent,
}

/// One `zwp_text_input_v3` event.
#[derive(Debug)]
pub(crate) enum TextEvent {
    Enter(WlSurface),
    Leave,
    Preedit {
        text: Option<String>,
        begin: i32,
        end: i32,
    },
    Commit(Option<String>),
    Delete {
        before: u32,
        after: u32,
    },
    Done(u32),
    /// The text input went with its seat or its manager: recorded
    /// behind what it reported before, so that reaches the field first.
    Retired,
}

/// Where a text input's dispatch records its events: the seats' queue,
/// the input feature's.
pub(crate) trait TextRecorder: Send + Sync {
    /// Queue `wire` behind what the seats recorded before it.
    fn record_text(&self, wire: TextWire);
}

/// A window's text as the seats' text inputs reach it: the text
/// feature's, which made it with the window. The window hands it to the
/// seats' input with its surface, and the input passes it to
/// [`SeatText::attach`] unopened.
pub trait SeatWindowText: Any + Send + Sync {}

/// The seats' text inputs: the text feature's (`text::linux::wayland`),
/// which the table gives the seats' input. The input binds the manager
/// and makes a text input per seat through it, and hands it each
/// recorded [`TextWire`] in turn. Called on the owner thread, with no
/// cell of the input borrowed.
pub(crate) trait SeatText: Send + Sync {
    /// Whether `interface` is the global the text inputs come from.
    fn speaks(&self, interface: &str) -> bool;

    /// Whether the manager is bound.
    fn holds_manager(&self) -> bool;

    /// Bind the manager, global `name`, and make a text input for each
    /// of `seats`, recording into `recorder`.
    fn bind(
        &self,
        registry: &WlRegistry,
        name: u32,
        version: u32,
        queue: &QueueHandle<WaylandState>,
        recorder: &Weak<dyn TextRecorder>,
        seats: &[WlSeat],
    );

    /// The compositor removed global `name`.
    fn global_removed(&self, name: u32);

    /// Seat `seat` was bound.
    fn seat_added(
        &self,
        seat: &WlSeat,
        queue: &QueueHandle<WaylandState>,
        recorder: &Weak<dyn TextRecorder>,
    );

    /// Seat `seat` went.
    fn seat_removed(&self, seat: &WlSeat);

    /// Window `id` opened on `surface` with `text`.
    fn attach(&self, id: WindowId, surface: &WlSurface, text: &Arc<dyn SeatWindowText>);

    /// Window `id` closed.
    fn detach(&self, id: WindowId);

    /// Apply one recorded event.
    fn deliver(&self, wire: TextWire);

    /// The loop ends: the text inputs go.
    fn release(&self);
}

/// The cursor the input last showed, for a test.
#[cfg(any(test, feature = "testing"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShownCursor {
    /// The name it was found by: a `cursor-shape` value's number, or the
    /// theme cursor's name.
    pub(crate) name: String,
    /// The image pixels per logical pixel a theme cursor was drawn at;
    /// `0` for a shape.
    pub(crate) scale: i32,
}

/// The loop's globals a theme cursor is drawn with: the input draws it
/// from the loop's `wl_shm` on a surface of the loop's `wl_compositor`,
/// and binds neither a second time.
#[derive(Clone)]
pub(crate) struct CursorGlobals {
    /// The connection the globals are on.
    pub(crate) connection: Connection,
    /// The owner's event queue.
    pub(crate) queue: QueueHandle<WaylandState>,
    /// `wl_compositor`, which makes the cursor's surface.
    pub(crate) compositor: WlCompositor,
    /// `wl_shm`, the shared memory the theme's images are in.
    pub(crate) shm: WlShm,
    /// `wp_viewporter`, through which an image whose size the cursor's
    /// scale does not divide is drawn at its size; `None` where the
    /// compositor offers none.
    pub(crate) viewporter: Option<WpViewporter>,
}

/// What the loop and its windows ask of the seats' input: the input
/// feature's (`input::linux::wayland`), which the table registers.
///
/// Reached on the owner thread. The dispatch of the input's objects
/// records what they report and runs no handler; [`Self::deliver`] runs
/// the handlers, with no lock of the loop held.
pub(crate) trait WaylandInput: Send + Sync {
    /// The compositor announced global `name`: bind it if the input
    /// speaks its interface.
    fn global(
        &self,
        registry: &WlRegistry,
        name: u32,
        interface: &str,
        version: u32,
        queue: &QueueHandle<WaylandState>,
    );

    /// The compositor removed global `name`.
    fn global_removed(&self, name: u32);

    /// The loop's globals a theme cursor is drawn with, handed over once
    /// as the loop starts, where the compositor has shared memory.
    fn cursor_globals(&self, globals: CursorGlobals);

    /// Deliver to the windows what the seats reported since the last
    /// turn, and a held key's repeat that came due.
    fn deliver(&self);

    /// When the input next delivers something no event brings.
    fn next_due(&self) -> Option<Instant>;

    /// A window opened: its input starts.
    fn attach(&self, window: SeatWindow);

    /// The window `id` closed: what it held open ends, and nothing more
    /// reaches it.
    fn detach(&self, id: WindowId);

    /// Show `cursor` while the pointer is over the window `id`.
    ///
    /// # Errors
    /// [`CursorError::Unsupported`] when the compositor takes no cursor
    /// shape from the host, or no image ([`PointerOffer`]).
    fn set_cursor(&self, id: WindowId, cursor: &Cursor) -> Result<(), CursorError>;

    /// Constrain the pointer of the seat whose keyboard is in the window
    /// `id` as `grab` asks, after letting go of what the window held;
    /// [`CursorGrab::None`] only lets go. A window in no seat's keyboard
    /// holds nothing. The input lets a grab go itself when the keyboard
    /// leaves the window, and the compositor ends it when it ends the
    /// constraint.
    ///
    /// # Errors
    /// [`PlatformFailure::Failed`] when the compositor offers no
    /// constraint of that kind.
    fn hold_pointer(&self, id: WindowId, grab: CursorGrab) -> Result<(), PlatformFailure>;

    /// What the window `id` holds of the pointer now: a grab the
    /// compositor said it carries out (`confined`, `locked`), none
    /// before it did or after it ended it.
    fn pointer_grab(&self, id: WindowId) -> CursorGrab;

    /// What the compositor offers the windows' pointer, as the input
    /// bound it.
    fn pointer_offer(&self) -> PointerOffer;

    /// The window `id` is drawn at another scale: a cursor theme image
    /// shown over it is drawn again for that scale.
    fn rescaled(&self, id: WindowId);

    /// The active keyboard layout's name; empty before a keymap came.
    fn keyboard_layout(&self) -> String;

    /// Call `callback` on the owner thread each time the layout changes.
    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>);

    /// The seats the input holds now, which the clipboard keeps a data
    /// device on.
    fn seats(&self) -> Vec<WlSeat>;

    /// The loop ends: let go of the windows and the callbacks.
    fn release(&self);

    /// The cursor the input last showed, for a test.
    #[cfg(any(test, feature = "testing"))]
    fn shown_cursor(&self) -> Option<ShownCursor>;
}

/// What the loop asks of the clipboard: the clipboard feature's
/// (`clipboard::linux::wayland`), which the table registers, and what
/// [`Platform::clipboard`] hands out, with the primary selection
/// [`Platform::primary_selection`] hands out where the compositor has
/// one.
///
/// Reached on the owner thread. The dispatch of its objects records what
/// they report and runs no handler; a read answers through its resolver,
/// and a request for the application's primary selection waits for
/// [`deliver`](Self::deliver).
pub(crate) trait WaylandClipboard:
    host::NativeClipboard + host::NativePrimarySelection + Send + Sync
{
    /// Whether the compositor has a primary selection the clipboard
    /// bound.
    fn has_primary_selection(&self) -> bool;

    /// Answer the requests for the application's primary selection the
    /// dispatch recorded, asking its text with no lock of the loop or of
    /// the clipboard held.
    fn deliver(&self);

    /// The compositor announced global `name`: bind it if the clipboard
    /// speaks its interface.
    fn global(
        &self,
        registry: &WlRegistry,
        name: u32,
        interface: &str,
        version: u32,
        queue: &QueueHandle<WaylandState>,
    );

    /// The compositor removed global `name`.
    fn global_removed(&self, name: u32);

    /// The seats the input holds now: each has a data device, and a seat
    /// gone lets go of its own.
    fn seats(&self, seats: &[WlSeat]);

    /// Whether the selection offer the last input's seat holds names
    /// the concealment hint; `None` without one. For a test.
    #[cfg(any(test, feature = "testing"))]
    fn selection_offers_hint(&self) -> Option<bool>;

    /// The loop ends: let go of the devices and the selection.
    fn release(&self);
}

/// A window as the drags over it reach it, from its open to its close.
pub(crate) struct DragWindow {
    /// The window's identity.
    pub(crate) id: WindowId,
    /// Its `wl_surface`, which a drag enters.
    pub(crate) surface: WlSurface,
    /// What it hears a drag through.
    pub(crate) delivery: Arc<WindowDelivery>,
}

/// What the loop and its windows ask of drag and drop: the drag
/// feature's (`drag::linux::wayland`), which the table registers, and the
/// platform's one [`DragTransport`].
///
/// Its windows' drags reach it through the seats' data devices, which
/// record them; [`Self::deliver`] tells the windows, with no lock of the
/// loop held.
pub(crate) trait WaylandDrag: DragTransport + Send + Sync {
    /// A window opened: drags over it are offered to it from now on.
    fn attach(&self, window: DragWindow);

    /// The window `id` closed: a drag over it ends with nothing told, and
    /// a session it started ends cancelled.
    fn detach(&self, id: WindowId);

    /// Tell the windows what the drags over them did since the last
    /// turn, and answer each drag's source.
    fn deliver(&self);

    /// The window `window`'s way to start a drag session.
    fn source(self: Arc<Self>, window: WindowId) -> Arc<dyn PlatformDragSource>;

    /// The loop ends: the drags' offers, a running session and the
    /// windows go.
    fn release(&self);
}
