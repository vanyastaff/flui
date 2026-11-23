//! The X11 host's input: XInput 2's pointers, touches, pens, touchpad pinches and swipes
//! and smooth scrolling, XKB's keyboard, and the cursor.
//!
//! The pure half ([`decode`]) turns XInput 2 events, copied into plain
//! [`XiEvent`](decode::XiEvent)s, and the server's device list into owned
//! input through the shared pointer assembly; it is checked on every host,
//! and the conformance table and the native recordings run it.
//!
//! The live half (`X11Input`) is the loop's input (`owner::linux::x11`'s
//! `LoopInput`): it selects each window's XInput 2 events as the window
//! opens, reads the events the loop hands it, keeps the keyboard's XKB state
//! as the server reports it, follows the device hierarchy, and sets each
//! window's cursor from the cursor theme.
//!
//! What the translation decides:
//!
//! - **One cursor per master pointer.** A mouse and a touchpad move the same
//!   cursor; its contact is the master device's, a mouse. A pen moves it too,
//!   but is a contact of its own (the pen's device), so the cursor leaves as
//!   the pen takes the master over, and the other way round.
//! - **Emulated events are not input.** The server emulates the legacy wheel
//!   buttons from a device's smooth-scroll valuators, and pointer events from
//!   a touch; both carry `XIPointerEmulated`, and the valuators and the touch
//!   are read instead.
//! - **A wheel turn is detents.** A scroll valuator's change divided by its
//!   increment is the wheel's rotation, fractional for a touchpad; the legacy
//!   wheel buttons of a device with no scroll valuator (XTest's) are whole
//!   detents. Smooth scrolling has no phases on X11: the scroll is unphased.
//! - **A pinch is one pan-zoom.** XInput 2.4 reports a touchpad pinch's scale
//!   since it began and its rotation since the last update, as Wayland's
//!   `pointer-gestures` does; each update's own factor and clockwise radians
//!   are assembled about the cursor.
//! - **A swipe is a gesture.** XInput 2.4 reports a touchpad swipe of its
//!   finger count with the fingers' accelerated step since the last event;
//!   the steps are summed into the travel since the swipe began. XInput has
//!   no hold, so nothing stops a fling from resting fingers here.
//! - **A grab that takes the pointer mid-press ends the press.** A `Leave` in
//!   grab mode (the window manager's move, another client's grab) cancels the
//!   pressed contact; the implicit grab of a press delivers its release.
//! - **A device unplugged is gone.** One the hierarchy reports removed ends
//!   its contacts and is removed before the list is read again, so a device
//!   plugged back in under the same id is a new device.
//! - **The server's time, while it is the time.** An event is stamped with
//!   the server's millisecond time while that is within a second of the
//!   monotonic clock (a local server keeps it), so a burst read in one turn
//!   keeps its spacing; otherwise with its arrival.

pub mod decode;

#[cfg(x11_host)]
pub(crate) use live::X11Input;

#[cfg(x11_host)]
mod live;
