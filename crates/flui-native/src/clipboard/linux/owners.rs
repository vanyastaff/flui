//! Which of the application's selections each seat reads.
//!
//! A Wayland selection belongs to one seat's data device: the application
//! sets its text as the selection of the seat the last input came from, and
//! another seat keeps whatever selection it held.
//!
//! [`Owners`] keeps one entry for each seat the application set a selection
//! on, and at most one text kept in this process alone: a write on a seat
//! with no data device, or under a compositor with no data device manager,
//! has no system clipboard to go to and answers `ApplicationOnly`
//! (ADR-0337 §1). The kept text is the newest selection while it lives:
//! every read answers it, and it gives way to a newer write or to the first
//! selection that arrives. It is never offered to a seat later: a selection
//! the compositor re-sends as the keyboard enters and a newer copy made in
//! another application look the same, so an offer could overwrite the
//! user's copy.

/// The application's selections by seat, and the text kept under none.
pub(crate) struct Owners<S, T> {
    /// One entry per seat.
    seated: Vec<(S, T)>,
    /// The text kept in this process alone, newer than every seated entry.
    kept: Option<T>,
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
    /// earlier selection, then the kept text), for its owner to release.
    pub(crate) fn set(&mut self, seat: S, item: T) -> impl Iterator<Item = T> + use<S, T> {
        let earlier = self
            .seated
            .iter()
            .position(|(held, _)| *held == seat)
            .map(|at| self.seated.remove(at).1);
        let kept = self.kept.take();
        self.seated.push((seat, item));
        earlier.into_iter().chain(kept)
    }

    /// Keeps `item` in this process alone. Returns the text kept before,
    /// which it replaces.
    pub(crate) fn keep(&mut self, item: T) -> Option<T> {
        self.kept.replace(item)
    }

    /// Takes the kept text: a newer write answered for the clipboard, or a
    /// selection arrived.
    pub(crate) fn take_kept(&mut self) -> Option<T> {
        self.kept.take()
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
            return Some(kept);
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
            .chain(self.kept)
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
        assert_eq!(owners.set(1, "first on 1").count(), 0);
        assert_eq!(owners.set(2, "on 2").count(), 0);
        assert_eq!(
            owners.set(1, "second on 1").collect::<Vec<&str>>(),
            ["first on 1"]
        );
        assert_eq!(owners.read(Some(&1)), Some(&"second on 1"));
        assert_eq!(owners.read(Some(&2)), Some(&"on 2"));
        assert_eq!(owners.into_items(), ["on 2", "second on 1"]);
    }

    /// Kept text answers every seat and a read with no seat; a newer write
    /// on any seat replaces it, as a newer kept text does, and so does the
    /// first selection that arrives.
    #[test]
    fn kept_text_gives_way_to_a_newer_write_or_a_selection() {
        let mut owners = Owners::<u32, &str>::default();
        owners.set(1, "on 1").for_each(drop);
        assert_eq!(owners.keep("kept"), None);
        assert_eq!(owners.read(Some(&2)), Some(&"kept"));
        assert_eq!(owners.read(None), Some(&"kept"));
        assert_eq!(owners.keep("kept again"), Some("kept"));
        assert_eq!(owners.set(2, "on 2").collect::<Vec<&str>>(), ["kept again"]);
        assert_eq!(owners.read(Some(&1)), Some(&"on 1"));
        assert_eq!(owners.read(Some(&2)), Some(&"on 2"));
        assert_eq!(owners.read(None), None);

        owners.keep("kept for want of a device");
        assert_eq!(owners.take_kept(), Some("kept for want of a device"));
        assert_eq!(owners.read(Some(&1)), Some(&"on 1"));
    }

    /// A cancelled source removes its own entry alone.
    #[test]
    fn a_cancelled_selection_leaves_the_others() {
        let mut owners = Owners::<u32, &str>::default();
        owners.set(1, "on 1").for_each(drop);
        owners.set(2, "on 2").for_each(drop);
        assert_eq!(owners.remove(|item| *item == "on 1"), Some("on 1"));
        assert_eq!(owners.remove(|item| *item == "on 1"), None);
        assert_eq!(owners.read(Some(&1)), None);
        assert_eq!(owners.read(Some(&2)), Some(&"on 2"));
    }
}
