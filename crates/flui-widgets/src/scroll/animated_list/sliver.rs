//! [`SliverAnimatedList`] — the stateful sliver every animated list is built
//! on.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::rc::Rc;

use flui_animation::{
    AnimationController, DrivenController, MotionSpec, Stagger, Vsync, VsyncUpdate,
};
use flui_foundation::{ValueKey, ViewKey};
use flui_objects::{Placement, PlacementSlot};
use flui_rendering::delegates::SliverGridDelegate;
use flui_view::element::ElementKind;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{BoxedView, IntoView, RebuildHandle, StatelessView, View, ViewExt, ViewState};

use super::roster::{Entry, ItemBuilder, Items, Shown, Source, merge};
use crate::animated::VsyncScope;
use crate::motion::{Animate, Effects, Transition, Trigger};
use crate::scroll::sliver_list::wrap_in_repaint_boundary;
use crate::scroll::{SliverGrid, SliverList};

/// The per-item extent a list estimates before it measures its items.
const DEFAULT_ITEM_EXTENT_ESTIMATE: f64 = 48.0;

/// How an animated list moves its items.
#[derive(Clone, Debug)]
pub(super) struct ListMotion {
    pub(super) transition: Transition,
    pub(super) placement: Option<MotionSpec>,
    pub(super) entrance: Option<(Effects, Stagger)>,
}

impl Default for ListMotion {
    fn default() -> Self {
        Self {
            transition: Transition::fade(),
            placement: Some(MotionSpec::default()),
            entrance: None,
        }
    }
}

/// How the sliver lays its items out.
#[derive(Clone)]
pub(super) enum SliverLayout {
    List { item_extent_estimate: f64 },
    Grid(Rc<dyn SliverGridDelegate>),
}

impl fmt::Debug for SliverLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::List {
                item_extent_estimate,
            } => f
                .debug_struct("List")
                .field("item_extent_estimate", item_extent_estimate)
                .finish(),
            Self::Grid(delegate) => f.debug_tuple("Grid").field(delegate).finish(),
        }
    }
}

/// A lazy sliver list over keyed items that animates every change to them.
///
/// Give it the items, a function naming each item's key and a builder for
/// an item's view, and rebuild it with new items: the list works out the
/// difference by key.
///
/// - A new key enters through the [`transition`](Self::transition).
/// - A key that disappears leaves: its last view stays where it was and plays
///   the transition's exit, then drops out. The caller keeps nothing around
///   for it.
/// - A key that comes back while it is still leaving takes back the same
///   element, with its state, and turns back from wherever its exit got to.
/// - Items whose position changes slide from where they were to where they
///   are (FLIP placement) along the [`placement`](Self::placement) motion; a
///   leaving item holds its index until it drops, and the items after it then
///   slide up.
///
/// Items are built lazily, like [`ListView::builder`](crate::ListView::builder):
/// only the visible band and its cache are mounted. A leaving item keeps its
/// index slot, so the band and the key lookup both count it. Placement
/// animates only an item laid out both before and after the change: one
/// scrolled in, or brought in from far away, simply appears in its place.
///
/// The items the list mounts with appear at rest, unless
/// [`stagger`](Self::stagger) gives them an entrance.
///
/// Every motion follows the presentation's [`MotionOverride`]: under
/// [`Reduce`](flui_animation::MotionOverride::Reduce) entries, exits and
/// slides land on their next frame.
///
/// ```
/// use flui_widgets::prelude::*;
/// use flui_widgets::SliverAnimatedList;
/// use flui_widgets::motion::Transition;
///
/// #[derive(Clone)]
/// struct Todo {
///     id: u64,
///     title: String,
/// }
///
/// let todos = vec![Todo { id: 1, title: "Write the list".into() }];
/// let list = SliverAnimatedList::new(todos, |todo| todo.id, |todo| Text::new(todo.title.clone()))
///     .transition(Transition::fade() + Transition::scale_in(0.9));
/// let _ = CustomScrollView::new((list,));
/// ```
///
/// [`MotionOverride`]: flui_animation::MotionOverride
#[derive(StatefulView)]
pub struct SliverAnimatedList<T, K> {
    items: Rc<Items<T, K>>,
    builder: ItemBuilder<T>,
    motion: ListMotion,
    layout: SliverLayout,
}

impl<T, K> Clone for SliverAnimatedList<T, K> {
    fn clone(&self) -> Self {
        Self {
            items: Rc::clone(&self.items),
            builder: Rc::clone(&self.builder),
            motion: self.motion.clone(),
            layout: self.layout.clone(),
        }
    }
}

impl<T, K> SliverAnimatedList<T, K>
where
    T: 'static,
    K: Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
{
    /// A list of `items`, each keyed by `key` and shown by `builder`.
    ///
    /// # Panics
    ///
    /// When two items share a key, with a `BUG:` message naming it.
    pub fn new<V: IntoView>(
        items: impl IntoIterator<Item = T>,
        key: impl Fn(&T) -> K,
        builder: impl Fn(&T) -> V + 'static,
    ) -> Self {
        Self::with_layout(
            items,
            key,
            builder,
            SliverLayout::List {
                item_extent_estimate: DEFAULT_ITEM_EXTENT_ESTIMATE,
            },
        )
    }

    /// The grid form [`AnimatedGrid`](super::AnimatedGrid) builds on.
    pub(super) fn grid<V: IntoView>(
        grid_delegate: impl SliverGridDelegate + 'static,
        items: impl IntoIterator<Item = T>,
        key: impl Fn(&T) -> K,
        builder: impl Fn(&T) -> V + 'static,
    ) -> Self {
        Self::with_layout(
            items,
            key,
            builder,
            SliverLayout::Grid(Rc::new(grid_delegate)),
        )
    }

    fn with_layout<V: IntoView>(
        items: impl IntoIterator<Item = T>,
        key: impl Fn(&T) -> K,
        builder: impl Fn(&T) -> V + 'static,
        layout: SliverLayout,
    ) -> Self {
        Self {
            items: Rc::new(Items::new(items, key)),
            builder: Rc::new(move |item| builder(item).into_view().boxed()),
            motion: ListMotion::default(),
            layout,
        }
    }

    /// How items enter and leave (default [`Transition::fade`]). An item
    /// already on its way keeps the curve it started with.
    #[must_use]
    pub fn transition(mut self, transition: Transition) -> Self {
        self.motion.transition = transition;
        self
    }

    /// How moved items slide into place (default [`MotionSpec::default`]);
    /// `None` places them at once.
    #[must_use]
    pub fn placement(mut self, placement: Option<MotionSpec>) -> Self {
        self.motion.placement = placement;
        self
    }

    /// Play `effects` on the items the list mounts with, each delayed by
    /// `stagger` for its position, so they arrive one after another. Items
    /// first built after the mount frame, and items inserted later, rest.
    #[must_use]
    pub fn stagger(mut self, effects: Effects, stagger: Stagger) -> Self {
        self.motion.entrance = Some((effects, stagger));
        self
    }

    /// The extent assumed for an item not yet measured (default 48). Only
    /// the list form has one; a grid's delegate sizes its tiles.
    ///
    /// # Panics
    ///
    /// Unless `estimate` is finite and positive.
    #[must_use]
    pub fn item_extent_estimate(mut self, estimate: f64) -> Self {
        assert!(
            estimate.is_finite() && estimate > 0.0,
            "item_extent_estimate must be finite and positive, got {estimate}"
        );
        if let SliverLayout::List {
            item_extent_estimate,
        } = &mut self.layout
        {
            *item_extent_estimate = estimate;
        }
        self
    }
}

impl<T, K: fmt::Debug> fmt::Debug for SliverAnimatedList<T, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverAnimatedList")
            .field("items", &self.items.len())
            .field("motion", &self.motion)
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

/// The entries of a [`SliverAnimatedList`], present and leaving, in the
/// order the sliver shows them, and the clock their slides share.
pub struct SliverAnimatedListState<T, K> {
    /// Swept during `build`, which takes `&self`, when an exit runs out.
    roster: RefCell<Vec<Entry<T, K>>>,
    /// Advances whenever the roster's order or membership changes; the
    /// sliver animates placement only across a change of it.
    generation: Cell<u64>,
    /// The placement clock, restarted by the sliver's layout.
    clock: DrivenController,
    vsync: Option<Vsync>,
    rebuild: Option<RebuildHandle>,
    /// Whether the mount frame's staggered entrance still plays for an item
    /// built now.
    entrance_open: Rc<Cell<bool>>,
}

impl<T, K: fmt::Debug> fmt::Debug for SliverAnimatedListState<T, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverAnimatedListState")
            .field("roster", &self.roster.borrow())
            .field("generation", &self.generation.get())
            .finish_non_exhaustive()
    }
}

impl<T, K> StatefulView for SliverAnimatedList<T, K>
where
    T: 'static,
    K: Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
{
    type State = SliverAnimatedListState<T, K>;

    fn create_state(&self) -> Self::State {
        let count = self.items.len();
        let staggered = self.motion.entrance.is_some();
        let roster = self
            .items
            .iter()
            .enumerate()
            .map(|(index, (key, item))| {
                Entry::new(
                    key.clone(),
                    Rc::clone(item),
                    Rc::clone(&self.builder),
                    &self.motion.transition,
                    false,
                    staggered.then_some((index, count)),
                    None,
                )
            })
            .collect();
        let clock = AnimationController::builder(MotionSpec::DEFAULT_DURATION)
            .debug_label("AnimatedList.placement")
            .build_on(None);
        let _ = clock.controller().set_value(1.0);
        SliverAnimatedListState {
            roster: RefCell::new(roster),
            generation: Cell::new(0),
            clock,
            vsync: None,
            rebuild: None,
            entrance_open: Rc::new(Cell::new(staggered)),
        }
    }
}

impl<T, K> SliverAnimatedListState<T, K>
where
    T: 'static,
    K: Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
{
    /// Bind every driver and the clock to `vsync` in one update.
    fn rebind(&mut self) {
        let vsync = self.vsync.clone();
        let roster = self.roster.get_mut();
        let clock = &mut self.clock;
        if let Err(error) = VsyncUpdate::run(|update| {
            for entry in roster.iter_mut() {
                update.rebind_controller(entry.driver_mut(), vsync.as_ref());
            }
            update.rebind_controller(clock, vsync.as_ref());
        }) {
            tracing::error!(%error, "an animated list lost its frame registry");
        }
    }

    /// Diff `view`'s items against the roster: keep, add, re-enter and leave
    /// entries, and advance the generation when the order changed.
    fn apply(&mut self, view: &SliverAnimatedList<T, K>) {
        let old = std::mem::take(self.roster.get_mut());
        let old_keys: Vec<K> = old.iter().map(|entry| entry.key.clone()).collect();
        let merged = merge(&old_keys, &view.items);
        let reordered = merged.len() != old_keys.len()
            || merged
                .iter()
                .zip(&old_keys)
                .any(|((key, _), old)| key != old);

        let mut by_key: HashMap<K, Entry<T, K>> = old
            .into_iter()
            .map(|entry| (entry.key.clone(), entry))
            .collect();
        let items: Vec<&(K, Rc<T>)> = view.items.iter().collect();
        let mut next = Vec::with_capacity(merged.len());
        for (key, source) in merged {
            let entry = match (by_key.remove(&key), source) {
                (Some(mut entry), Source::Item(index)) => {
                    entry.show(Rc::clone(&items[index].1), Rc::clone(&view.builder));
                    entry.enter();
                    entry
                }
                (Some(mut entry), Source::Ghost) => {
                    entry.leave();
                    entry
                }
                (None, Source::Item(index)) => {
                    let mut entry = Entry::new(
                        key,
                        Rc::clone(&items[index].1),
                        Rc::clone(&view.builder),
                        &view.motion.transition,
                        true,
                        None,
                        self.vsync.as_ref(),
                    );
                    if let Some(rebuild) = &self.rebuild {
                        entry.watch(rebuild.clone());
                    }
                    entry
                }
                (None, Source::Ghost) => {
                    unreachable!("BUG: a ghost key always comes from the previous roster")
                }
            };
            next.push(entry);
        }
        debug_assert!(by_key.is_empty(), "BUG: the merge dropped an entry");
        *self.roster.get_mut() = next;
        if reordered {
            self.bump();
        }
    }

    /// Mark the roster changed. The generation names no item: the sliver
    /// only compares it with the one its last settled pass saw, so wrapping
    /// would need 2^64 changes between two layouts to hide one.
    fn bump(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
    }

    /// Drop the entries whose exit ran out.
    fn sweep(&self) {
        let gone: Vec<Entry<T, K>> = self
            .roster
            .borrow_mut()
            .extract_if(.., |entry| entry.is_gone())
            .collect();
        if !gone.is_empty() {
            self.bump();
        }
        // Dropped once the roster is available again: an item's destructor is
        // user code. Each entry withdraws its driver even during an unwind.
        drop(gone);
    }
}

impl<T, K> ViewState<SliverAnimatedList<T, K>> for SliverAnimatedListState<T, K>
where
    T: 'static,
    K: Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
{
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let rebuild = ctx.rebuild_handle();
        for entry in self.roster.get_mut() {
            entry.watch(rebuild.clone());
        }
        self.rebuild = Some(rebuild);
        self.vsync = VsyncScope::maybe_of(ctx);
        self.rebind();
        if self.entrance_open.get() {
            // The mount frame's items play the entrance; anything built
            // after it rests.
            let open = Rc::clone(&self.entrance_open);
            let scheduled = ctx
                .post_frame_handle()
                .is_some_and(|post_frame| post_frame.schedule(move |_| open.set(false)).is_ok());
            if !scheduled {
                tracing::warn!(
                    "an animated list has no post-frame lane; its entrance closes on its next update"
                );
            }
        }
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.vsync = VsyncScope::maybe_of(ctx);
        self.rebind();
    }

    fn did_update_view(&mut self, old: &SliverAnimatedList<T, K>, new: &SliverAnimatedList<T, K>) {
        if old.motion.transition != new.motion.transition {
            for entry in self.roster.get_mut().iter() {
                entry.retime(&new.motion.transition);
            }
        }
        self.apply(new);
        self.entrance_open.set(false);
    }

    fn build(&self, view: &SliverAnimatedList<T, K>, _ctx: &dyn BuildContext) -> impl IntoView {
        self.sweep();

        // One snapshot per build: the closures the sliver keeps read these,
        // never the roster.
        let (keys, shown): (Vec<K>, Vec<Rc<Shown<T>>>) = self
            .roster
            .borrow()
            .iter()
            .map(|entry| (entry.key.clone(), Rc::clone(&entry.shown)))
            .unzip();
        let count = shown.len();
        let slots: Rc<[Rc<PlacementSlot>]> =
            shown.iter().map(|shown| Rc::clone(&shown.slot)).collect();
        let index: HashMap<K, usize> = keys
            .iter()
            .enumerate()
            .map(|(index, key)| (key.clone(), index))
            .collect();

        let item = ItemView {
            transition: view.motion.transition.clone(),
            entrance: view.motion.entrance.clone(),
            entrance_open: Rc::clone(&self.entrance_open),
        };
        let builder: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(move |position| {
            let shown = shown.get(position)?;
            Some(item.build(&keys[position], shown))
        });
        let find = move |key: &dyn ViewKey| {
            key.as_any()
                .downcast_ref::<ValueKey<K>>()
                .and_then(|key| index.get(key.value()).copied())
        };
        let placement = view.motion.placement.clone().map(|motion| {
            Placement::new(
                self.clock.controller().clone(),
                motion,
                slots,
                self.generation.get(),
            )
        });

        match &view.layout {
            SliverLayout::List {
                item_extent_estimate,
            } => {
                let sliver =
                    SliverList::new(count, *item_extent_estimate, builder).find_index_by_key(find);
                match placement {
                    Some(placement) => sliver.placement(placement).boxed(),
                    None => sliver.boxed(),
                }
            }
            SliverLayout::Grid(delegate) => {
                let sliver = SliverGrid::with_config(Rc::clone(delegate), count, builder)
                    .find_index_by_key(find);
                match placement {
                    Some(placement) => sliver.placement(placement).boxed(),
                    None => sliver.boxed(),
                }
            }
        }
    }

    fn dispose(&mut self) {
        let roster = std::mem::take(self.roster.get_mut());
        self.clock.dispose();
        drop(roster);
    }
}

/// Wraps an item's view in its transition, its entrance and its key.
struct ItemView {
    transition: Transition,
    entrance: Option<(Effects, Stagger)>,
    entrance_open: Rc<Cell<bool>>,
}

impl ItemView {
    fn build<T, K>(&self, key: &K, shown: &Shown<T>) -> BoxedView
    where
        K: Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
    {
        let mut child = Animate::transition(self.transition.clone(), shown.view())
            .trigger(Trigger::Controller(Rc::clone(&shown.progress)))
            .boxed();
        if let Some((effects, stagger)) = &self.entrance {
            let (index, count) = shown.entrance.unwrap_or((0, 1));
            let trigger = if self.entrance_open.get() && shown.entrance.is_some() {
                Trigger::OnMount
            } else {
                Trigger::Target(1.0)
            };
            child = Animate::new(effects.clone().stagger(*stagger, index, count), child)
                .trigger(trigger)
                .boxed();
        }
        wrap_in_repaint_boundary(
            KeyedItem {
                key: ValueKey::new(key.clone()),
                child,
            }
            .boxed(),
        )
    }
}

/// Carries an item's key into the sliver, so the item keeps its element
/// wherever it moves, leaving included.
#[derive(Clone)]
struct KeyedItem<K: Clone + Eq + Hash + Send + Sync + 'static> {
    key: ValueKey<K>,
    child: BoxedView,
}

impl<K> View for KeyedItem<K>
where
    K: Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
{
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

impl<K> StatelessView for KeyedItem<K>
where
    K: Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
{
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        self.child.clone()
    }
}
