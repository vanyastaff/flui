//! The last user input the seats reported, which an activation token names.

use std::sync::Arc;

use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_surface::WlSurface;

use crate::owner::linux::wayland::user_input::{Inputs, UserInput};
use crate::owner::slot::OwnerCell;

/// A user input on this connection, as an activation token names it.
pub(crate) type SeatInput = UserInput<WlSeat, WlSurface>;

/// The last user input the seats reported, on the owner thread: the seats'
/// input records it as its events come, and a window names it on the
/// activation token it asks for. Nothing recorded, a token names no input
/// and asks the compositor for the user's attention instead of the focus.
///
/// Beside it, the press that began each seat's pointer grab while a button
/// is held: a drag starts only under the serial of the grab its origin
/// surface holds. See [`Inputs`].
#[derive(Clone, Default)]
pub(crate) struct LastInput(Arc<OwnerCell<Inputs<WlSeat, WlSurface>>>);

impl LastInput {
    /// The seats report `input`, which is now the last: a key, a button
    /// or a touch going down, or the keyboard entering a surface.
    pub(crate) fn record(&self, input: SeatInput) {
        self.0.with(|inputs| inputs.record(input));
    }

    /// The last input; `None` before any, or off the owner thread.
    pub(crate) fn latest(&self) -> Option<SeatInput> {
        self.0.with(|inputs| inputs.latest().cloned()).flatten()
    }

    /// The last input a selection is set under. See
    /// [`Inputs::latest_in_focus`].
    pub(crate) fn latest_in_focus(&self) -> Option<SeatInput> {
        self.0
            .with(|inputs| inputs.latest_in_focus().cloned())
            .flatten()
    }

    /// `surface`'s window closed. See [`Inputs::surface_gone`].
    pub(crate) fn surface_gone(&self, surface: &WlSurface) {
        self.0.with(|inputs| inputs.surface_gone(surface));
    }

    /// `seat`'s keyboard left `surface`. See [`Inputs::keyboard_left`].
    pub(crate) fn keyboard_left(&self, seat: &WlSeat, surface: &WlSurface) {
        self.0.with(|inputs| inputs.keyboard_left(seat, surface));
    }

    /// A button of `press.seat`'s pointer went down: the first one held
    /// begins the seat's grab, on `press.surface` under `press.serial`.
    pub(crate) fn pressed(&self, press: SeatInput) {
        self.0.with(|inputs| inputs.pressed(press));
    }

    /// A button of `seat`'s pointer went up: the last one held ends the
    /// seat's grab.
    pub(crate) fn released(&self, seat: &WlSeat) {
        self.0.with(|inputs| inputs.released(seat));
    }

    /// Watch the grab `seat`'s press `serial` began, which a drag
    /// session starts under. See [`Inputs::watch`].
    pub(crate) fn watch(&self, seat: WlSeat, serial: u32) {
        self.0.with(|inputs| inputs.watch(seat, serial));
    }

    /// Whether the watched grab of `seat`'s press `serial` ended: its
    /// last button went up over its surface, or its seat went; `false`
    /// off the owner thread.
    pub(crate) fn watched_ended(&self, seat: &WlSeat, serial: u32) -> bool {
        self.0
            .with(|inputs| inputs.watched_ended(seat, serial))
            .unwrap_or(false)
    }

    /// `seat`'s pointer left its surface, or went, which ends any grab
    /// it held: no release reaches a surface the pointer left.
    pub(crate) fn left(&self, seat: &WlSeat) {
        self.0.with(|inputs| inputs.left(seat));
    }

    /// `seat`'s pointer went while the seat stays. See
    /// [`Inputs::pointer_gone`].
    pub(crate) fn pointer_gone(&self, seat: &WlSeat) {
        self.0.with(|inputs| inputs.pointer_gone(seat));
    }

    /// `seat` went: nothing kept names it any more.
    pub(crate) fn seat_gone(&self, seat: &WlSeat) {
        self.0.with(|inputs| inputs.seat_gone(seat));
    }

    /// The press that began a grab `surface` holds now; `None` when no
    /// button is held there, or off the owner thread.
    pub(crate) fn grab_on(&self, surface: &WlSurface) -> Option<SeatInput> {
        self.0
            .with(|inputs| inputs.grab_on(surface).cloned())
            .flatten()
    }
}
