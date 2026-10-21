//! Display identity for browser screens that carry no stable key.
//!
//! The Window Management API lists the screens as JS objects. The browser
//! hands back the same object for the same screen across `screenschange`, and
//! a label is no identity (two identical monitors share one). A screen is
//! therefore known by the object itself, compared by identity, in a list the
//! platform retains: a screen found in the list keeps its [`DisplayId`], and
//! an object that matches nothing is a new display with a fresh id. An id
//! leaves the list with its screen and is never issued again, so a monitor that
//! is unplugged and plugged back in is a new display.

use flui_platform::DisplayId;

/// The screens of the last listing, each with the id it was given.
#[derive(Debug)]
pub(crate) struct RetainedScreens<T> {
    retained: Vec<(T, DisplayId)>,
}

impl<T> RetainedScreens<T> {
    /// An empty list.
    pub(crate) fn new() -> Self {
        Self {
            retained: Vec::new(),
        }
    }

    /// Whether no screen is retained.
    pub(crate) fn is_empty(&self) -> bool {
        self.retained.is_empty()
    }

    /// Retain `screen` under an id issued before it was listed: the display a
    /// page saw as its only one is this screen.
    pub(crate) fn adopt(&mut self, screen: T, id: DisplayId) {
        self.retained.push((screen, id));
    }

    /// Replace the list with `current` and answer the id of each entry, in
    /// order: the retained id for an entry `same` identifies with a retained
    /// one, otherwise the id `mint` issues. `None` where `mint` has none left;
    /// such a screen is not retained and has no display.
    pub(crate) fn reconcile(
        &mut self,
        current: Vec<T>,
        same: impl Fn(&T, &T) -> bool,
        mut mint: impl FnMut() -> Option<DisplayId>,
    ) -> Vec<Option<DisplayId>> {
        let mut previous = std::mem::take(&mut self.retained);
        let mut ids = Vec::with_capacity(current.len());
        for screen in current {
            let known = previous
                .iter()
                .position(|(retained, _)| same(retained, &screen))
                .map(|index| previous.swap_remove(index).1);
            let id = known.or_else(&mut mint);
            if let Some(id) = id {
                self.retained.push((screen, id));
            }
            ids.push(id);
        }
        ids
    }

    /// The id of the retained screen `same` identifies with `screen`.
    pub(crate) fn id_of(&self, screen: &T, same: impl Fn(&T, &T) -> bool) -> Option<DisplayId> {
        self.retained
            .iter()
            .find(|(retained, _)| same(retained, screen))
            .map(|(_, id)| *id)
    }
}

#[cfg(test)]
mod tests {
    use flui_platform::{DisplayId, DisplayKey, DisplayRegistry};

    use super::RetainedScreens;

    /// A screen object: the object is its identity, the label is not.
    #[derive(Debug, Clone, Copy)]
    struct Screen {
        object: u32,
        label: &'static str,
    }

    fn same(a: &Screen, b: &Screen) -> bool {
        a.object == b.object
    }

    fn mint_from(registry: &DisplayRegistry) -> impl FnMut() -> Option<DisplayId> + '_ {
        || registry.mint(&DisplayKey::Unkeyed).ok()
    }

    const LEFT: Screen = Screen {
        object: 1,
        label: "DELL U2720Q",
    };
    const RIGHT: Screen = Screen {
        object: 2,
        label: "DELL U2720Q",
    };

    #[test]
    fn retained_screen_keeps_its_id() {
        let registry = DisplayRegistry::new();
        let mut screens = RetainedScreens::new();
        let first = screens.reconcile(vec![LEFT, RIGHT], same, mint_from(&registry));
        // The listing comes back in another order, as `screenschange` may.
        let second = screens.reconcile(vec![RIGHT, LEFT], same, mint_from(&registry));
        assert_eq!(second, vec![first[1], first[0]]);
    }

    #[test]
    fn a_label_shared_by_two_screens_is_not_one_display() {
        let registry = DisplayRegistry::new();
        let mut screens = RetainedScreens::new();
        let ids = screens.reconcile(vec![LEFT, RIGHT], same, mint_from(&registry));
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn new_screen_object_gets_a_fresh_id() {
        let registry = DisplayRegistry::new();
        let mut screens = RetainedScreens::new();
        let first = screens.reconcile(vec![LEFT], same, mint_from(&registry));
        // The same label on a different object is a different screen.
        let second = screens.reconcile(vec![LEFT, RIGHT], same, mint_from(&registry));
        assert_eq!(second[0], first[0]);
        assert_ne!(second[1], first[0]);
    }

    #[test]
    fn removed_screen_id_is_not_reissued() {
        let registry = DisplayRegistry::new();
        let mut screens = RetainedScreens::new();
        let first = screens.reconcile(vec![LEFT, RIGHT], same, mint_from(&registry));
        screens.reconcile(vec![LEFT], same, mint_from(&registry));
        // The right monitor is plugged back in: a new object, a new display.
        let back = screens.reconcile(vec![LEFT, RIGHT], same, mint_from(&registry));
        assert_eq!(back[0], first[0]);
        assert_ne!(back[1], first[1]);
    }

    #[test]
    fn a_screen_with_no_id_left_is_not_retained() {
        let registry = DisplayRegistry::new();
        let mut screens = RetainedScreens::new();
        let ids = screens.reconcile(vec![LEFT, RIGHT], same, || None);
        assert_eq!(ids, vec![None, None]);
        assert_eq!(screens.id_of(&LEFT, same), None);
        // Ids that become available later are issued then.
        let later = screens.reconcile(vec![LEFT], same, mint_from(&registry));
        assert!(later[0].is_some());
    }

    #[test]
    fn an_adopted_screen_keeps_the_id_it_was_given() {
        // The single screen a page saw before screen access is the same
        // physical screen as the one the listing calls current.
        let registry = DisplayRegistry::new();
        let before = registry
            .mint(&DisplayKey::Web("primary".into()))
            .expect("id");
        let mut screens = RetainedScreens::new();
        assert!(screens.is_empty());
        screens.adopt(RIGHT, before);
        assert!(!screens.is_empty());
        let ids = screens.reconcile(vec![LEFT, RIGHT], same, mint_from(&registry));
        assert_eq!(ids[1], Some(before), "the adopted screen keeps its id");
        assert_ne!(ids[0], Some(before), "another screen gets a fresh one");
    }

    #[test]
    fn id_of_finds_the_current_screen_by_identity() {
        let registry = DisplayRegistry::new();
        let mut screens = RetainedScreens::new();
        let ids = screens.reconcile(vec![LEFT, RIGHT], same, mint_from(&registry));
        assert_eq!(screens.id_of(&RIGHT, same), ids[1]);
        let stranger = Screen {
            object: 3,
            label: "DELL U2720Q",
        };
        assert_eq!(screens.id_of(&stranger, same), None);
        assert_eq!(LEFT.label, stranger.label);
    }
}
