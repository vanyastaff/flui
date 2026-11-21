//! What the seats' input keeps of the user inputs a later request names
//! ([`Inputs`]): the last one, which an activation token names, and each
//! seat's pointer grab, which a drag starts under.

#![forbid(unsafe_code)]

/// The seats the input binds, and so the most pointers a window's own
/// frame follows. A seat is a user at the machine, and a compositor with
/// more than a few is unheard of; a compositor announcing seats without
/// end is not bound past this.
pub(crate) const MAX_SEATS: usize = 16;

/// A user input as an activation token names it: the seat it came from,
/// the serial the compositor gave its event (a key, a button, a touch,
/// or the keyboard focus entering a surface), and the surface it went to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserInput<Seat, Surface> {
    /// The seat whose device the input came from.
    pub(crate) seat: Seat,
    /// The serial of the input's event.
    pub(crate) serial: u32,
    /// The surface the input went to.
    pub(crate) surface: Surface,
}

/// The user inputs the seats reported that a later request names: the
/// last one, which an activation token names, and the press that began
/// each seat's pointer grab while a button is held, which a drag starts
/// under. A seat has one grab at most, and a seat that goes leaves
/// nothing here but the end of a watched grab, so what is kept is
/// bounded by the seats bound.
#[derive(Debug)]
pub(crate) struct Inputs<Seat, Surface> {
    last: Option<UserInput<Seat, Surface>>,
    /// Whether the keyboard left the surface `last` went to since: a
    /// compositor takes no selection under that input any more, while an
    /// activation token still names it.
    last_left: bool,
    /// Each seat's grab: the press that began it and how many buttons
    /// hold it now.
    grabs: Vec<(UserInput<Seat, Surface>, u32)>,
    /// The grab a drag session started under, and whether it ended in a
    /// way no drag under it survives. See [`Self::watch`].
    watched: Option<Watched<Seat>>,
}

/// The grab a drag session started under, by its seat and the serial of
/// its press.
#[derive(Debug)]
struct Watched<Seat> {
    seat: Seat,
    serial: u32,
    /// Its last button went up over its surface, or its seat went.
    ended: bool,
}

impl<Seat, Surface> Default for Inputs<Seat, Surface> {
    fn default() -> Self {
        Self {
            last: None,
            last_left: false,
            grabs: Vec::new(),
            watched: None,
        }
    }
}

impl<Seat: Clone + PartialEq, Surface: PartialEq> Inputs<Seat, Surface> {
    /// The seats report `input`, which is now the last.
    pub(crate) fn record(&mut self, input: UserInput<Seat, Surface>) {
        self.last = Some(input);
        self.last_left = false;
    }

    /// The last input; `None` before any.
    pub(crate) const fn latest(&self) -> Option<&UserInput<Seat, Surface>> {
        self.last.as_ref()
    }

    /// The last input while the keyboard has not left the surface it went
    /// to: the one a selection is set under. `None` before any input, or
    /// once the keyboard left, until the next input.
    pub(crate) fn latest_in_focus(&self) -> Option<&UserInput<Seat, Surface>> {
        self.last.as_ref().filter(|_| !self.last_left)
    }

    /// `surface`'s window closed: an input that went to it names no
    /// surface any more, so it is no longer the last input, and a grab on
    /// it ends.
    pub(crate) fn surface_gone(&mut self, surface: &Surface) {
        if self
            .last
            .as_ref()
            .is_some_and(|last| last.surface == *surface)
        {
            self.last = None;
            self.last_left = false;
        }
        self.grabs.retain(|(grab, _)| grab.surface != *surface);
    }

    /// `seat`'s keyboard left `surface`: the last input, if it was the
    /// seat's on that surface, sets no selection any more. The last input
    /// stays what an activation token names.
    pub(crate) fn keyboard_left(&mut self, seat: &Seat, surface: &Surface) {
        if self
            .last
            .as_ref()
            .is_some_and(|last| last.seat == *seat && last.surface == *surface)
        {
            self.last_left = true;
        }
    }

    /// A button of `press.seat`'s pointer went down: the first one held
    /// begins the seat's grab, on `press.surface` under `press.serial`.
    pub(crate) fn pressed(&mut self, press: UserInput<Seat, Surface>) {
        match self
            .grabs
            .iter_mut()
            .find(|(grab, _)| grab.seat == press.seat)
        {
            Some((_, held)) => *held = held.saturating_add(1),
            None => self.grabs.push((press, 1)),
        }
    }

    /// A button of `seat`'s pointer went up: the last one held ends the
    /// seat's grab.
    pub(crate) fn released(&mut self, seat: &Seat) {
        let mut ended = None;
        self.grabs.retain_mut(|(grab, held)| {
            if grab.seat == *seat {
                *held = held.saturating_sub(1);
                if *held == 0 {
                    ended = Some(grab.serial);
                }
            }
            *held > 0
        });
        if let (Some(serial), Some(watched)) = (ended, self.watched.as_mut())
            && watched.seat == *seat
            && watched.serial == serial
        {
            // A drag takes the pointer from its surface before any
            // release: one that reached the surface means none began.
            watched.ended = true;
        }
    }

    /// Watch the grab `seat`'s press `serial` began, which a drag
    /// session starts under, in place of any grab watched before: from
    /// here [`Self::watched_ended`] says whether it ended in a way no
    /// drag under it survives.
    pub(crate) fn watch(&mut self, seat: Seat, serial: u32) {
        self.watched = Some(Watched {
            seat,
            serial,
            ended: false,
        });
    }

    /// Whether the grab `seat`'s press `serial` began is the one watched
    /// and ended since it was: its last button went up over its surface,
    /// so the compositor took no drag from it, or its seat went, taking
    /// any drag under it along.
    pub(crate) fn watched_ended(&self, seat: &Seat, serial: u32) -> bool {
        self.watched.as_ref().is_some_and(|watched| {
            watched.ended && watched.seat == *seat && watched.serial == serial
        })
    }

    /// `seat`'s pointer left its surface, or went, which ends any grab
    /// it held: no release reaches a surface the pointer left.
    pub(crate) fn left(&mut self, seat: &Seat) {
        self.grabs.retain(|(grab, _)| grab.seat != *seat);
    }

    /// `seat`'s pointer went while the seat stays: the grab it held ends,
    /// and so does a watched grab of the seat's, whether or not a leave
    /// came first. Weston (`weston_seat_release_pointer`) and wlroots
    /// (`wlr_seat_set_capabilities`) send the focused surface a leave
    /// before the capabilities, so a grab's leave tells nothing here: a
    /// drag the compositor ignored would never end, and one it took is
    /// cancelled by the compositor with its pointer, which the session
    /// hears and ends on at most once.
    pub(crate) fn pointer_gone(&mut self, seat: &Seat) {
        if let Some(watched) = self.watched.as_mut()
            && watched.seat == *seat
        {
            watched.ended = true;
        }
        self.left(seat);
    }

    /// `seat` went: its grab ends, the last input no longer names it, so
    /// no token is asked for under a seat the compositor no longer
    /// knows, and a watched grab of its own has ended, whether its
    /// button was still held or already released.
    pub(crate) fn seat_gone(&mut self, seat: &Seat) {
        self.left(seat);
        if self.last.as_ref().is_some_and(|last| last.seat == *seat) {
            self.last = None;
        }
        if let Some(watched) = self.watched.as_mut()
            && watched.seat == *seat
        {
            watched.ended = true;
        }
    }

    /// The press that began a grab `surface` holds now; `None` when no
    /// button is held there.
    pub(crate) fn grab_on(&self, surface: &Surface) -> Option<&UserInput<Seat, Surface>> {
        self.grabs
            .iter()
            .find(|(grab, _)| grab.surface == *surface)
            .map(|(grab, _)| grab)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(seat: u32, serial: u32, surface: u32) -> UserInput<u32, u32> {
        UserInput {
            seat,
            serial,
            surface,
        }
    }

    /// A seat's grab begins with its first button and ends with its
    /// last, or as its pointer leaves; the watched grab a release ended
    /// is one a drag started under it never began.
    #[test]
    fn a_grab_lasts_while_a_button_is_held() {
        let mut inputs = Inputs::default();
        inputs.pressed(input(1, 10, 7));
        inputs.pressed(input(1, 11, 7));
        assert_eq!(
            inputs.grab_on(&7),
            Some(&input(1, 10, 7)),
            "the first press"
        );
        inputs.watch(1, 10);
        inputs.released(&1);
        assert_eq!(inputs.grab_on(&7), Some(&input(1, 10, 7)), "one still held");
        assert!(!inputs.watched_ended(&1, 10));
        inputs.released(&1);
        assert_eq!(inputs.grab_on(&7), None);
        assert!(inputs.watched_ended(&1, 10), "ended by a release");
        assert!(!inputs.watched_ended(&1, 11), "the grab of 10 is watched");

        inputs.pressed(input(2, 20, 8));
        inputs.watch(2, 20);
        assert!(!inputs.watched_ended(&1, 10), "watched in its place");
        inputs.left(&2);
        assert_eq!(inputs.grab_on(&8), None, "the pointer left");
        assert!(!inputs.watched_ended(&2, 20), "no release reached it");
    }

    /// The keyboard leaving the surface the last input went to ends that
    /// input for a selection, which a compositor takes only from the
    /// client the user is in, and leaves it for an activation token.
    /// Leaving another surface, or another seat's keyboard leaving, keeps
    /// it; the next input is in focus again.
    #[test]
    fn the_keyboard_leaving_ends_the_last_input_for_a_selection_alone() {
        let mut inputs = Inputs::default();
        inputs.record(input(1, 5, 3));
        inputs.keyboard_left(&1, &4);
        inputs.keyboard_left(&2, &3);
        assert_eq!(inputs.latest_in_focus(), Some(&input(1, 5, 3)));
        inputs.keyboard_left(&1, &3);
        assert_eq!(inputs.latest_in_focus(), None);
        assert_eq!(inputs.latest(), Some(&input(1, 5, 3)), "a token names it");
        inputs.record(input(1, 6, 3));
        assert_eq!(inputs.latest_in_focus(), Some(&input(1, 6, 3)));
    }

    /// A window that closes takes the inputs that went to it along: no
    /// selection, claim or token names a serial from a destroyed window,
    /// and a grab on it ends. Another window's input stays.
    #[test]
    fn a_closed_window_leaves_no_input_behind() {
        let mut inputs = Inputs::default();
        inputs.record(input(1, 5, 3));
        inputs.pressed(input(1, 5, 3));
        inputs.surface_gone(&4);
        assert_eq!(inputs.latest(), Some(&input(1, 5, 3)));
        inputs.surface_gone(&3);
        assert_eq!(inputs.latest(), None);
        assert_eq!(inputs.latest_in_focus(), None);
        assert_eq!(inputs.grab_on(&3), None);
    }

    /// A seat removed with a button held leaves no grab and is not the
    /// last input: a compositor adding and removing seats without end
    /// grows nothing, and no token asked after names a seat that is
    /// gone.
    #[test]
    fn a_seat_that_goes_leaves_no_input_behind() {
        let mut inputs = Inputs::default();
        inputs.pressed(input(1, 5, 3));
        inputs.record(input(1, 5, 3));
        for seat in 2..10_002 {
            inputs.record(input(seat, seat, 7));
            inputs.pressed(input(seat, seat, 7));
            inputs.seat_gone(&seat);
            assert_eq!(inputs.grab_on(&7), None, "seat {seat} is gone");
            assert_eq!(inputs.latest(), None, "seat {seat} is gone");
        }
        assert_eq!(inputs.grabs.len(), 1, "the seat still here keeps its grab");
        assert_eq!(inputs.grab_on(&3), Some(&input(1, 5, 3)));
        inputs.record(input(1, 6, 3));
        inputs.seat_gone(&1);
        assert_eq!(inputs.latest(), None);
    }

    /// A drag session's grab ends when its seat goes, in either order
    /// one read may carry: the release, then the seat's removal; or the
    /// removal with the button still held. A compositor that ignored
    /// the drag says nothing of it either way, and the session ends on
    /// what is recorded here. Another seat's going ends nothing.
    #[test]
    fn a_watched_grab_ends_when_its_seat_goes() {
        let mut released_then_gone = Inputs::default();
        released_then_gone.pressed(input(1, 5, 3));
        released_then_gone.watch(1, 5);
        released_then_gone.released(&1);
        released_then_gone.seat_gone(&1);
        assert!(
            released_then_gone.watched_ended(&1, 5),
            "released, then the seat went"
        );

        let mut gone_held = Inputs::default();
        gone_held.pressed(input(1, 5, 3));
        gone_held.pressed(input(2, 9, 4));
        gone_held.watch(1, 5);
        gone_held.seat_gone(&2);
        assert!(!gone_held.watched_ended(&1, 5), "another seat went");
        gone_held.seat_gone(&1);
        assert!(gone_held.watched_ended(&1, 5), "gone with its button held");

        let mut gone_after_leave = Inputs::default();
        gone_after_leave.pressed(input(1, 5, 3));
        gone_after_leave.watch(1, 5);
        gone_after_leave.left(&1);
        assert!(!gone_after_leave.watched_ended(&1, 5), "the drag began");
        gone_after_leave.seat_gone(&1);
        assert!(
            gone_after_leave.watched_ended(&1, 5),
            "the seat went with the drag under way"
        );
    }

    /// A seat that keeps its seat but loses its pointer ends the watched
    /// grab, in the order a compositor sends it (a leave, then the
    /// capabilities) and with no leave at all. A leave alone ends
    /// nothing, nor does another seat's pointer going.
    #[test]
    fn a_watched_grab_ends_when_its_pointer_goes() {
        let mut held = Inputs::default();
        held.pressed(input(1, 5, 3));
        held.pressed(input(2, 9, 4));
        held.watch(1, 5);
        held.pointer_gone(&2);
        assert!(!held.watched_ended(&1, 5), "another seat's pointer went");
        held.pointer_gone(&1);
        assert!(
            held.watched_ended(&1, 5),
            "its pointer went, no leave first"
        );
        assert_eq!(held.grab_on(&3), None);

        let mut left = Inputs::default();
        left.pressed(input(1, 5, 3));
        left.watch(1, 5);
        left.left(&1);
        assert!(!left.watched_ended(&1, 5), "a leave alone ends nothing");
        left.pointer_gone(&1);
        assert!(left.watched_ended(&1, 5), "a leave, then its pointer went");
    }
}
