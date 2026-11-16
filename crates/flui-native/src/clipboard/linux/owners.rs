//! Which of the application's selections each seat reads.
//!
//! A Wayland selection belongs to one seat's data device: the application
//! sets its text as the selection of the seat the last input came from, and
//! another seat keeps whatever selection it held.
//!
//! [`Owners`] keeps one entry for each seat the application set a selection
//! on, and at most one text kept in this process alone, under no seat: a
//! write with no input serial to set it under, or with no data device to
//! set it on, answers `ApplicationOnly` (ADR-0337 §1), and that answer is an
//! obligation the host still owes the system clipboard. The kept text is
//! the newest selection while it lives: every read answers it, a newer write
//! replaces it, and it is offered to the seat at the first serial the seat
//! gives. A selection another client sets before that attempt does not
//! drop it, since the host has not yet had the chance to offer it; one that
//! arrives after the attempt does.

/// The application's selections by seat, and the text kept under none.
pub(crate) struct Owners<S, T> {
    /// One entry per seat.
    seated: Vec<(S, T)>,
    /// The text kept in this process alone, newer than every seated entry.
    kept: Option<Kept<T>>,
}

/// Text kept under no seat, and whether the host tried to offer it yet.
struct Kept<T> {
    item: T,
    offered: bool,
}

impl<S, T> Default for Owners<S, T> {
    fn default() -> Self {
        Self {
            seated: Vec::new(),
            kept: None,
        }
    }
}

impl<S: PartialEq, T> Owners<S, T> {
    /// Sets `item` as `seat`'s selection, a write newer than the text kept
    /// under no seat, which it replaces. Returns what it replaces (`seat`'s
    /// earlier selection, and the kept text), for its owner to release.
    pub(crate) fn set(&mut self, seat: S, item: T) -> Vec<T> {
        let mut replaced = Vec::new();
        if let Some(at) = self.seated.iter().position(|(held, _)| *held == seat) {
            replaced.push(self.seated.remove(at).1);
        }
        replaced.extend(self.take_kept());
        self.seated.push((seat, item));
        replaced
    }

    /// Keeps `item` in this process alone, not yet offered to any seat.
    /// Returns the text kept before, which it replaces.
    pub(crate) fn keep(&mut self, item: T) -> Option<T> {
        self.kept
            .replace(Kept {
                item,
                offered: false,
            })
            .map(|kept| kept.item)
    }

    /// Takes the kept text: a newer write answered for the clipboard.
    pub(crate) fn take_kept(&mut self) -> Option<T> {
        self.kept.take().map(|kept| kept.item)
    }

    /// The kept text the host owes the system clipboard and has not tried to
    /// offer yet.
    pub(crate) fn owed(&self) -> Option<&T> {
        self.kept
            .as_ref()
            .filter(|kept| !kept.offered)
            .map(|kept| &kept.item)
    }

    /// The host tried to offer the kept text and no seat took it: a
    /// selection that arrives from now on replaces it.
    pub(crate) fn offer_failed(&mut self) {
        if let Some(kept) = &mut self.kept {
            kept.offered = true;
        }
    }

    /// Another client's selection arrived. The kept text gives way to it only
    /// once the host has tried to offer it; before that it stays, and is
    /// returned `None`.
    pub(crate) fn selection_arrived(&mut self) -> Option<T> {
        if self.kept.as_ref().is_some_and(|kept| kept.offered) {
            self.take_kept()
        } else {
            None
        }
    }

    /// The selection set on `seat`.
    pub(crate) fn get_mut(&mut self, seat: &S) -> Option<&mut T> {
        self.seated
            .iter_mut()
            .find(|(held, _)| held == seat)
            .map(|(_, item)| item)
    }

    /// The selection a read on `seat` answers: the kept text while there is
    /// one, else the one set on that seat. A read with no seat answers only
    /// the kept text.
    pub(crate) fn read(&self, seat: Option<&S>) -> Option<&T> {
        if let Some(kept) = &self.kept {
            return Some(&kept.item);
        }
        let seat = seat?;
        self.seated
            .iter()
            .find(|(held, _)| held == seat)
            .map(|(_, item)| item)
    }

    /// The first seated selection `matches` names, in place.
    pub(crate) fn find_mut(&mut self, matches: impl Fn(&T) -> bool) -> Option<&mut T> {
        self.seated
            .iter_mut()
            .map(|(_, item)| item)
            .find(|item| matches(item))
    }

    /// Removes the first seated selection `matches` names: the one whose
    /// source the compositor cancelled.
    pub(crate) fn remove(&mut self, matches: impl Fn(&T) -> bool) -> Option<T> {
        let at = self.seated.iter().position(|(_, item)| matches(item))?;
        Some(self.seated.remove(at).1)
    }

    /// Every selection, the kept text last, for its owner to release.
    pub(crate) fn into_items(self) -> Vec<T> {
        self.seated
            .into_iter()
            .map(|(_, item)| item)
            .chain(self.kept.map(|kept| kept.item))
            .collect::<Vec<T>>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A copy on one seat releases only that seat's earlier selection: the
    /// other seat still reads its own.
    #[test]
    fn a_copy_on_one_seat_keeps_the_other_seats_selection() {
        let mut owners = Owners::<u32, &str>::default();
        assert_eq!(owners.set(1, "first on 1"), Vec::<&str>::new());
        assert_eq!(owners.set(2, "on 2"), Vec::<&str>::new());
        assert_eq!(owners.set(1, "second on 1"), ["first on 1"]);
        assert_eq!(owners.read(Some(&1)), Some(&"second on 1"));
        assert_eq!(owners.read(Some(&2)), Some(&"on 2"));
        assert_eq!(owners.into_items(), ["on 2", "second on 1"]);
    }

    /// The compositor's order after a cut kept for the application: another
    /// client's selection arrives as the window takes the keyboard back,
    /// before the keyboard's `enter` gives the serial the kept text is
    /// offered under. That selection does not drop the text: a paste before
    /// the offer reads it, the offer under the new serial seats it, and a
    /// paste after reads it from the seat. Had the offer failed, the next
    /// selection would replace it.
    #[test]
    fn kept_text_outlives_a_selection_that_arrives_before_its_offer() {
        let mut owners = Owners::<u32, &str>::default();
        owners.set(1, "copied earlier");
        assert_eq!(owners.keep("cut"), None);
        // selection
        assert_eq!(owners.selection_arrived(), None);
        // paste before the enter
        assert_eq!(owners.read(None), Some(&"cut"));
        assert_eq!(owners.read(Some(&1)), Some(&"cut"));
        // enter: the kept text is owed, and offered under its serial
        assert_eq!(owners.owed(), Some(&"cut"));
        assert_eq!(owners.set(1, "cut"), ["copied earlier", "cut"]);
        assert_eq!(owners.owed(), None);
        // paste
        assert_eq!(owners.read(Some(&1)), Some(&"cut"));
        assert_eq!(owners.selection_arrived(), None);
        assert_eq!(owners.read(Some(&1)), Some(&"cut"));

        // An offer no seat took leaves the text until the next selection.
        let mut owners = Owners::<u32, &str>::default();
        owners.keep("cut");
        owners.offer_failed();
        assert_eq!(owners.owed(), None);
        assert_eq!(owners.read(Some(&1)), Some(&"cut"));
        assert_eq!(owners.selection_arrived(), Some("cut"));
        assert_eq!(owners.read(Some(&1)), None);
    }

    /// Kept text answers every seat and a read with no seat; a newer write
    /// on any seat replaces it, as a newer kept text does.
    #[test]
    fn a_newer_write_replaces_the_kept_text() {
        let mut owners = Owners::<u32, &str>::default();
        owners.set(1, "on 1");
        assert_eq!(owners.keep("kept"), None);
        assert_eq!(owners.read(Some(&2)), Some(&"kept"));
        assert_eq!(owners.keep("kept again"), Some("kept"));
        assert_eq!(owners.set(2, "on 2"), ["kept again"]);
        assert_eq!(owners.read(Some(&1)), Some(&"on 1"));
        assert_eq!(owners.read(Some(&2)), Some(&"on 2"));
        assert_eq!(owners.read(None), None);
    }

    /// A cancelled source removes its own entry alone.
    #[test]
    fn a_cancelled_selection_leaves_the_others() {
        let mut owners = Owners::<u32, &str>::default();
        owners.set(1, "on 1");
        owners.set(2, "on 2");
        assert_eq!(owners.remove(|item| *item == "on 1"), Some("on 1"));
        assert_eq!(owners.remove(|item| *item == "on 1"), None);
        assert_eq!(owners.read(Some(&1)), None);
        assert_eq!(owners.read(Some(&2)), Some(&"on 2"));
    }
}
