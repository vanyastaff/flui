//! Which of the application's selections each seat reads.
//!
//! A Wayland selection belongs to one seat's data device: the application
//! sets its text as the selection of the seat the last input came from, and
//! another seat keeps whatever selection it held. Before any input it holds
//! its text in this process alone, under no seat.
//!
//! [`Owners`] keeps one entry for each seat the application set a selection
//! on, and at most one held under none, newest last. A write replaces only
//! the entry of its own seat, so a copy on one seat never releases what
//! another seat holds; a read on a seat answers the newest entry set on it
//! or held under none.

/// The application's selections by seat, newest last: at most one entry
/// per seat, and at most one under no seat (`None`).
pub(crate) struct Owners<S, T> {
    entries: Vec<(Option<S>, T)>,
}

impl<S, T> Default for Owners<S, T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<S: PartialEq, T> Owners<S, T> {
    /// Sets `item` as the newest selection of `seat`, and returns the one it
    /// replaces on that seat, for its owner to release.
    pub(crate) fn set(&mut self, seat: Option<S>, item: T) -> Option<T> {
        let replaced = self
            .entries
            .iter()
            .position(|(held, _)| *held == seat)
            .map(|at| self.entries.remove(at).1);
        self.entries.push((seat, item));
        replaced
    }

    /// The selection set on `seat`, made the newest: a write that changes it
    /// in place is the last one made.
    pub(crate) fn get_mut(&mut self, seat: &S) -> Option<&mut T> {
        let at = self
            .entries
            .iter()
            .position(|(held, _)| held.as_ref() == Some(seat))?;
        let entry = self.entries.remove(at);
        self.entries.push(entry);
        self.entries.last_mut().map(|(_, item)| item)
    }

    /// The selection a read on `seat` answers: the newest set on that seat or
    /// held under none. A read with no seat answers only the one held under
    /// none.
    pub(crate) fn read(&self, seat: Option<&S>) -> Option<&T> {
        self.entries
            .iter()
            .rev()
            .find(|(held, _)| held.is_none() || held.as_ref() == seat)
            .map(|(_, item)| item)
    }

    /// Removes the first selection `matches` names: the one whose source the
    /// compositor cancelled.
    pub(crate) fn remove(&mut self, matches: impl Fn(&T) -> bool) -> Option<T> {
        let at = self.entries.iter().position(|(_, item)| matches(item))?;
        Some(self.entries.remove(at).1)
    }

    /// Removes the selection held under no seat: another client's selection
    /// arrived, and it is the selection now.
    pub(crate) fn remove_unseated(&mut self) -> Option<T> {
        let at = self.entries.iter().position(|(held, _)| held.is_none())?;
        Some(self.entries.remove(at).1)
    }

    /// Every selection, oldest first, for its owner to release.
    pub(crate) fn into_items(self) -> Vec<T> {
        self.entries
            .into_iter()
            .map(|(_, item)| item)
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
        assert_eq!(owners.set(Some(1), "first on 1"), None);
        assert_eq!(owners.set(Some(2), "on 2"), None);
        assert_eq!(owners.set(Some(1), "second on 1"), Some("first on 1"));
        assert_eq!(owners.read(Some(&1)), Some(&"second on 1"));
        assert_eq!(owners.read(Some(&2)), Some(&"on 2"));
        assert_eq!(owners.into_items(), ["on 2", "second on 1"]);
    }

    /// Text held under no seat answers every seat that set nothing newer, and
    /// a read with no seat; a seat's own newer selection wins on that seat.
    #[test]
    fn text_held_under_no_seat_answers_until_a_newer_one() {
        let mut owners = Owners::<u32, &str>::default();
        assert_eq!(owners.set(Some(1), "on 1"), None);
        assert_eq!(owners.set(None, "held"), None);
        assert_eq!(owners.read(Some(&1)), Some(&"held"));
        assert_eq!(owners.read(Some(&2)), Some(&"held"));
        assert_eq!(owners.read(None), Some(&"held"));
        assert_eq!(owners.set(Some(2), "on 2"), None);
        assert_eq!(owners.read(Some(&2)), Some(&"on 2"));
        assert_eq!(owners.read(Some(&1)), Some(&"held"));
        assert_eq!(owners.remove_unseated(), Some("held"));
        assert_eq!(owners.read(Some(&1)), Some(&"on 1"));
        assert_eq!(owners.read(None), None);
    }

    /// A cancelled source removes its own entry alone.
    #[test]
    fn a_cancelled_selection_leaves_the_others() {
        let mut owners = Owners::<u32, &str>::default();
        owners.set(Some(1), "on 1");
        owners.set(Some(2), "on 2");
        assert_eq!(owners.remove(|item| *item == "on 1"), Some("on 1"));
        assert_eq!(owners.remove(|item| *item == "on 1"), None);
        assert_eq!(owners.read(Some(&1)), None);
        assert_eq!(owners.read(Some(&2)), Some(&"on 2"));
    }

    /// A selection changed in place is the newest: it answers its seat over
    /// text held under no seat since.
    #[test]
    fn a_selection_changed_in_place_is_the_newest() {
        let mut owners = Owners::<u32, &str>::default();
        owners.set(Some(1), "on 1");
        owners.set(None, "held");
        assert_eq!(owners.get_mut(&2), None);
        *owners.get_mut(&1).expect("set on 1") = "changed on 1";
        assert_eq!(owners.read(Some(&1)), Some(&"changed on 1"));
        assert_eq!(owners.into_items(), ["held", "changed on 1"]);
    }
}
