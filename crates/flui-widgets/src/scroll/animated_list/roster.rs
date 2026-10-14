//! The keyed diff the animated lists share: which entries a new item list
//! keeps, which it adds, and where the ones it dropped stay while they leave.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::Hash;
use std::rc::Rc;

use flui_animation::{
    Animation, AnimationController, AnimationError, AnimationRunFuture, AnimationStatus,
    CurvedAnimation, DrivenController, StatusSubscription, Vsync,
};
use flui_foundation::RebuildReason;
use flui_objects::PlacementSlot;
use flui_view::{BoxedView, RebuildHandle};

use crate::motion::Transition;
use crate::support::retirement::Terminal;

/// Builds the view of one item.
pub(super) type ItemBuilder<T> = Rc<dyn Fn(&T) -> BoxedView>;

/// The items one configuration of a list shows, in order, each with its key.
pub(super) struct Items<T, K> {
    entries: Vec<(K, Rc<T>)>,
    keys: HashSet<K>,
}

impl<T, K> Items<T, K>
where
    K: Clone + Eq + Hash + fmt::Debug,
{
    /// Key every item.
    ///
    /// # Panics
    ///
    /// When two items share a key: the list could not tell which one an
    /// entry, its state and its animation belong to.
    pub(super) fn new(items: impl IntoIterator<Item = T>, key: impl Fn(&T) -> K) -> Self {
        let entries: Vec<(K, Rc<T>)> = items
            .into_iter()
            .map(|item| (key(&item), Rc::new(item)))
            .collect();
        let mut keys = HashSet::with_capacity(entries.len());
        for (key, _) in &entries {
            assert!(
                keys.insert(key.clone()),
                "BUG: animated list item key {key:?} appears twice; every item needs its own key"
            );
        }
        Self { entries, keys }
    }

    fn contains(&self, key: &K) -> bool {
        self.keys.contains(key)
    }
}

impl<T, K> Items<T, K> {
    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &(K, Rc<T>)> {
        self.entries.iter()
    }
}

/// Where a key of the merged roster comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    /// The new items' entry at this index.
    Item(usize),
    /// An entry the new items dropped, kept while it leaves.
    Ghost,
}

/// The next roster: every new item in its new order, and each key of `old`
/// the new items dropped, kept right after the key that preceded it in `old`
/// (at the start when nothing did), so a leaving entry stays beside its
/// neighbour wherever that neighbour moves.
pub(super) fn merge<T, K>(old: &[K], new: &Items<T, K>) -> Vec<(K, Source)>
where
    K: Clone + Eq + Hash + fmt::Debug,
{
    // Each dropped key, filed under the key just before it in `old`.
    let mut leading: Vec<&K> = Vec::new();
    let mut after: HashMap<&K, Vec<&K>> = HashMap::new();
    let mut previous: Option<&K> = None;
    for key in old {
        if !new.contains(key) {
            match previous {
                Some(anchor) => after.entry(anchor).or_default().push(key),
                None => leading.push(key),
            }
        }
        previous = Some(key);
    }

    let mut merged = Vec::with_capacity(new.len() + old.len());
    // Ghosts chain after ghosts: a stack, so a long run of them cannot
    // recurse deeply.
    let mut emit_ghosts = |anchor: Option<&K>, merged: &mut Vec<(K, Source)>| {
        let mut pending: Vec<&K> = match anchor {
            Some(anchor) => after.remove(anchor).unwrap_or_default(),
            None => std::mem::take(&mut leading),
        };
        pending.reverse();
        while let Some(ghost) = pending.pop() {
            merged.push((ghost.clone(), Source::Ghost));
            if let Some(mut chained) = after.remove(ghost) {
                chained.reverse();
                pending.extend(chained);
            }
        }
    };
    emit_ghosts(None, &mut merged);
    for (index, (key, _)) in new.iter().enumerate() {
        merged.push((key.clone(), Source::Item(index)));
        emit_ghosts(Some(key), &mut merged);
    }
    merged
}

/// What the lazy item builder reads of an entry. One build hands the sliver
/// closures holding these, so they outlive the entry's place in the roster
/// until the sliver's next update.
pub(super) struct Shown<T> {
    item: RefCell<Terminal<Rc<T>>>,
    builder: RefCell<Terminal<ItemBuilder<T>>>,
    /// The entry's presence, eased by the transition's curve: `0` absent,
    /// `1` present.
    pub(super) progress: Rc<dyn Animation<f64>>,
    /// Where the sliver last laid the entry out.
    pub(super) slot: Rc<PlacementSlot>,
    /// The entry's place in the batch the list mounted with, for a
    /// staggered entrance.
    pub(super) entrance: Option<(usize, usize)>,
}

impl<T> Shown<T> {
    /// The item's view. The item and builder are cloned out before the
    /// builder runs, so it may reach this entry again.
    pub(super) fn view(&self) -> BoxedView {
        let item = Rc::clone(&self.item.borrow());
        let builder = Rc::clone(&self.builder.borrow());
        builder(&item)
    }

    /// Show `item` through `builder` from now on. The outgoing values retire
    /// after both slots hold the new ones.
    fn show(&self, item: Rc<T>, builder: ItemBuilder<T>) {
        let item = self.item.replace(Terminal::new(item));
        let builder = self.builder.replace(Terminal::new(builder));
        drop(item);
        drop(builder);
    }
}

/// One keyed entry of an animated list: present, or leaving after its key
/// was dropped.
pub(super) struct Entry<T, K> {
    pub(super) key: K,
    pub(super) shown: Rc<Shown<T>>,
    /// Drives `shown.progress`.
    driver: DrivenController,
    status: Option<StatusSubscription>,
    /// Set once a leaving entry's exit has run out.
    dismissed: Rc<Cell<bool>>,
    leaving: bool,
}

impl<T, K> Entry<T, K> {
    /// A present entry: at rest when `entering` is false (the list's first
    /// items), otherwise entering from absent.
    pub(super) fn new(
        key: K,
        item: Rc<T>,
        builder: ItemBuilder<T>,
        transition: &Transition,
        entering: bool,
        entrance: Option<(usize, usize)>,
        vsync: Option<&Vsync>,
    ) -> Self {
        let driver = AnimationController::builder(transition.run_duration())
            .debug_label("AnimatedList.item")
            .build_on(vsync);
        if !entering {
            driver.controller().set_value(1.0);
        }
        let linear: Rc<dyn Animation<f64>> = Rc::new(driver.controller().clone());
        let progress: Rc<dyn Animation<f64>> =
            Rc::new(CurvedAnimation::new(linear, transition.run_curve()));
        let entry = Self {
            key,
            shown: Rc::new(Shown {
                item: RefCell::new(Terminal::new(item)),
                builder: RefCell::new(Terminal::new(builder)),
                progress,
                slot: Rc::new(PlacementSlot::new()),
                entrance,
            }),
            driver,
            status: None,
            dismissed: Rc::new(Cell::new(false)),
            leaving: false,
        };
        if entering {
            entry.admit("enter", entry.driver.controller().forward());
        }
        entry
    }

    /// Ask `rebuild` for a build once this entry's exit runs out, so the
    /// build drops it.
    pub(super) fn watch(&mut self, rebuild: RebuildHandle) {
        let dismissed = Rc::clone(&self.dismissed);
        let subscription =
            self.driver
                .controller()
                .subscribe_status(Rc::new(move |status: AnimationStatus| {
                    if status == AnimationStatus::ReverseSettled {
                        dismissed.set(true);
                        rebuild.schedule(RebuildReason::AnimationTick);
                    }
                }));
        drop(self.status.replace(subscription));
    }

    pub(super) fn driver_mut(&mut self) -> &mut DrivenController {
        &mut self.driver
    }

    /// Show the item's latest value.
    pub(super) fn show(&self, item: Rc<T>, builder: ItemBuilder<T>) {
        self.shown.show(item, builder);
    }

    /// Runs of this entry take the transition's duration.
    pub(super) fn retime(&self, transition: &Transition) {
        self.driver
            .controller()
            .set_duration(transition.run_duration());
    }

    /// Present again: a leaving entry turns back from where its exit got to.
    pub(super) fn enter(&mut self) {
        if self.leaving {
            self.leaving = false;
            self.dismissed.set(false);
            self.admit("re-enter", self.driver.controller().forward());
        }
    }

    /// Leave: play the exit from where the entry is. An entry with nothing
    /// to play back, or whose exit is refused, is dismissed at once rather
    /// than left in the roster.
    pub(super) fn leave(&mut self) {
        if self.leaving {
            return;
        }
        self.leaving = true;
        let controller = self.driver.controller();
        if controller.value() <= 0.0 || controller.reverse().is_err() {
            if controller.is_animating() {
                let _ = controller.stop();
            }
            self.dismissed.set(true);
        }
    }

    /// Whether the entry left and its exit ran out.
    pub(super) fn is_gone(&self) -> bool {
        self.leaving && self.dismissed.get()
    }

    /// An entry whose entrance is refused shows at once instead.
    fn admit(&self, what: &str, run: Result<AnimationRunFuture, AnimationError>) {
        if let Err(error) = run {
            tracing::warn!(%error, what, "an animated list item refused a run; it shows at once");
            self.driver.controller().set_value(1.0);
        }
    }
}

impl<T, K> Drop for Entry<T, K> {
    fn drop(&mut self) {
        // Logical cancellation runs even during an incoming unwind; the
        // authored item and builder sit in terminal slots of `shown`.
        drop(self.status.take());
        self.driver.dispose();
    }
}

impl<T, K: fmt::Debug> fmt::Debug for Entry<T, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Entry")
            .field("key", &self.key)
            .field("leaving", &self.leaving)
            .field("dismissed", &self.dismissed.get())
            .finish_non_exhaustive()
    }
}
