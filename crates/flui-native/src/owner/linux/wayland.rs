//! The Wayland host's loop: the connection to the compositor, the globals it
//! binds, and the owner thread's turns (ADR-0151 §10).
//!
//! Two halves:
//!
//! - `wait`, the pure one: how long a turn may block, by the rule ADR-0249
//!   set for the Android loop. Every producer of work wakes the loop, so it
//!   wakes on no timer of its own, and a wake deadline that came due is turned
//!   into frame requests once, never into a busy loop while no frame serves
//!   it.
//! - `event_loop`: `WaylandLoop` over `wayland-client` on libwayland. The
//!   owner thread blocks in `poll` on the compositor's socket and an `eventfd`
//!   every other producer writes (the owner signal, a redraw asked for on
//!   another thread), reads and dispatches what the compositor sent, then lets
//!   each window carry it out with no lock of the loop held.
//!
//! The loop names no feature. What it keeps of one, it keeps through a hook
//! declared here that the host's table fills (`WaylandHost`): the displays,
//! the seats' input, the clipboard, the Settings portal's preferences, the
//! notice channel and the sources of logind's and the kernel's notices. The
//! input binds the
//! globals it speaks as the compositor announces them, records what its
//! objects report, and delivers it to the windows on each turn before they
//! settle (`WaylandInput`). The last user input the seats report, which an
//! activation token names, is kept here too (`LastInput`): the input feature
//! records it, the window feature reads it. A window's own frame reaches the
//! input the same way (`FramePointer`): the window feature draws it, the
//! input tells it what the pointer does over its surfaces. The seats' data
//! devices are base code (`transfer::linux::wayland`): the loop feeds them the
//! globals and the seats the input holds every turn. The clipboard
//! (`WaylandClipboard`) reads and sets a seat's selection through them, under
//! the last user input; drag and drop (`WaylandDrag`) hears the drags over the
//! windows through them and tells the windows on the turn. Its
//! windows are the window feature's (`window::linux::wayland`): the loop
//! holds them as `LoopWindow`s and asks each to settle after every read, and
//! the window feature implements the dispatch of its own protocol objects on
//! `WaylandState`.

#[cfg(all(target_os = "linux", feature = "wayland"))]
pub use event_loop::OutputName;
#[cfg(all(
    target_os = "linux",
    feature = "wayland",
    any(test, feature = "testing")
))]
pub(crate) use event_loop::ShownCursor;
#[cfg(all(target_os = "linux", feature = "wayland"))]
pub(crate) use event_loop::{
    CursorGlobals, DragWindow, FramePointer, LastInput, LiveOutputs, LoopWaker, LoopWindow,
    MAX_OUTPUTS, PresentationTime, SUSPENSION_VERSION, SeatInput, SeatText, SeatWindow,
    SeatWindowText, TextEvent, TextRecorder, TextWire, WaylandClipboard, WaylandDisplays,
    WaylandDrag, WaylandHost, WaylandInput, WaylandLoop, WaylandState, WindowSeed, WindowTable,
    bindable,
};
#[cfg(all(target_os = "linux", feature = "wayland"))]
pub(crate) use outputs::{OnOutputs, OutputFacts};

/// What the compositor offers a window's pointer, as the seats' input bound
/// it (`WaylandInput::pointer_offer`):
/// shared memory for a cursor image's buffer, and the pointer constraints and
/// relative motion a grab is made of. The window's capabilities follow it
/// (`window::linux::wayland::configure::pointer_reported`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PointerOffer {
    /// `wl_shm`, which a cursor image is a buffer of.
    pub(crate) shared_memory: bool,
    /// `zwp_pointer_constraints_v1`, which confines and locks a pointer.
    pub(crate) constraints: bool,
    /// `zwp_relative_pointer_manager_v1`, which reports a locked pointer's
    /// motion.
    pub(crate) relative_motion: bool,
}

pub(crate) mod activation;
#[cfg(all(target_os = "linux", feature = "wayland"))]
mod event_loop;
pub(crate) mod outputs;
pub(crate) mod user_input;
pub(crate) mod wait;
