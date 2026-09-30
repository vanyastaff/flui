//! [`TabController`] — shared selection state for [`crate::TabBar`], plus
//! [`DefaultTabController`], the inherited-widget shortcut that owns one for
//! a subtree.
//!
//! # `TabController`: one non-`Send` cell, not an `Arc<AtomicUsize>` pair
//!
//! A tab controller holds two plain indices, `index` and `previous_index`,
//! mutated together and then notified once. A naive Rust design — following
//! [`crate::CupertinoTabController`](../flui_cupertino/struct.CupertinoTabController.html)'s
//! `Arc<AtomicUsize>` precedent, but for a *pair* of counters — would let a
//! listener observe a **torn** snapshot: `index` swapped to the new value on
//! one atomic while `previous_index` still reads the old-old value on the
//! other, if anything ever raced the two swaps. It would also advertise
//! `Send + Sync` on a type this crate's own state model never shares across
//! threads — every `TabController` is created, read, and mutated from the UI
//! realm only (`DefaultTabController`'s state, or a caller's own
//! single-realm code) — so `Send + Sync` would be a claim with no real
//! backing, "false Send advertising" that invites a caller to hand a
//! `TabController` across a thread boundary where nothing here actually
//! makes that safe.
//!
//! Instead, `(index, previous_index)` lives in **one** `Cell<(usize,
//! usize)>` behind an `Rc` — Copy, so `Cell::set` replaces the whole pair in
//! one non-interruptible store; there is no window where a reader can
//! observe one field updated and not the other, because on a single
//! (`!Send`) realm nothing else can run between the `set` and the `notify`.
//! `Rc<Cell<_>>` is `!Send`/`!Sync`, so `TabController` itself does not (and
//! cannot) implement [`Listenable`](flui_sdk::foundation::Listenable) — that
//! trait requires `Send + Sync` — which is exactly the point: the compiler
//! now enforces single-realm use instead of a doc comment promising it.
//!
//! The listener registry follows the same logic: [`TabController::add_listener`]
//! takes a plain `Rc<dyn Fn()>` (this crate's usual owner-local callback
//! shape — see [`crate::ink_well::InkWell::on_tap`]), not
//! `flui_sdk::foundation::ListenerCallback` (`Arc<dyn Fn() + Send + Sync>`). A
//! `Send + Sync`-bound callback could never legally capture this
//! `TabController` (or its `Rc<Cell<_>>` state) to begin with, so reusing
//! `flui_sdk::foundation::ChangeNotifier` here would make the *listener*
//! unable to read back the very state it was notified about.
//! [`TabBar`](crate::TabBar) subscribes with a plain closure that schedules
//! a rebuild via [`flui_sdk::view::RebuildHandle`] (itself `Send + Sync`, but
//! that is incidental — nothing about the registry requires it).
//!
//! # `animate_to` is a documented alias, not an animation
//!
//! A full animated tab change would additionally: (a) notify listeners
//! **twice** when a `duration` is given — once immediately (so an
//! `indexIsChanging` flag flips true) and once when the drive animation
//! completes; (b) expose `indexIsChanging`, read by `TabBarView`'s
//! warp-to-adjacent-page optimization to distinguish a programmatic tab
//! change from a drag; (c) animate `animation.value` from the old index to
//! the new one over `duration`/`curve`. None of that exists here: this V1 has no
//! `AnimationController`/`Ticker` wiring on `TabController` at all — see
//! [`crate::tabs`] module docs for why `TabBarView` itself is out of scope
//! for this unit. [`TabController::animate_to`] is therefore a plain alias
//! for [`TabController::set_index`]: one synchronous index change, one
//! synchronous notify, no interpolation. `indexIsChanging` and the
//! start/completion double-notify are named deferrals, not silently dropped
//! — they return when a caller (`TabBarView`, or a real
//! `AnimationController`-driven indicator sweep) needs them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_sdk::foundation::ListenerId;
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, InheritedView, impl_inherited_view};

/// `(index, previous_index)`, mutated and read as one unit — see the module
/// docs' "one non-`Send` cell" section for why this is a single `Cell` of a
/// packed pair rather than two independent counters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct IndexPair {
    index: usize,
    previous_index: usize,
}

/// A [`TabController`] change listener — `Rc`-based, matching this crate's
/// other owner-local callback types (e.g. [`crate::ink_well::InkWell`]'s
/// `on_tap`), NOT `flui_sdk::foundation::ListenerCallback`
/// (`Arc<dyn Fn() + Send + Sync>`). This is deliberate, not an oversight: a
/// `Send + Sync` callback could never legally capture a `TabController` (or
/// anything reachable from its `Rc<Cell<_>>` state) in the first place, so a
/// `TabController`-flavored listener registry needs its own `!Send`
/// callback type rather than reusing `flui_sdk::foundation::ChangeNotifier`'s.
type TabChangeListener = Rc<dyn Fn()>;

/// [`TabController`]'s own listener registry — a private, `!Send`
/// counterpart to `flui_sdk::foundation::ChangeNotifier` (see
/// [`TabChangeListener`]'s doc comment for why that type doesn't fit here).
/// `Rc`-shared so every [`TabController`] clone registers into and notifies
/// from the same underlying list.
#[derive(Clone)]
struct TabListenerRegistry {
    listeners: Rc<RefCell<Vec<(ListenerId, TabChangeListener)>>>,
    /// The next id to hand out. `ListenerId` is 1-based (see this
    /// workspace's ID offset pattern — public ids are `NonZeroUsize`), so
    /// this starts at `1`, not `0`.
    next_id: Rc<Cell<usize>>,
}

impl Default for TabListenerRegistry {
    fn default() -> Self {
        Self {
            listeners: Rc::new(RefCell::new(Vec::new())),
            next_id: Rc::new(Cell::new(1)),
        }
    }
}

impl TabListenerRegistry {
    fn add_listener(&self, listener: TabChangeListener) -> ListenerId {
        let id = ListenerId::new(self.next_id.get());
        self.next_id.set(self.next_id.get() + 1);
        self.listeners.borrow_mut().push((id, listener));
        id
    }

    fn remove_listener(&self, id: ListenerId) {
        self.listeners
            .borrow_mut()
            .retain(|(entry, _)| *entry != id);
    }

    fn len(&self) -> usize {
        self.listeners.borrow().len()
    }

    /// Fires every registered listener. Snapshots the list first (cloning
    /// the `Rc<dyn Fn()>` handles, not the `Vec`'s backing allocation) so a
    /// listener that adds/removes a listener mid-notify does not conflict
    /// with the in-progress borrow — same reentrancy shape as
    /// `flui_sdk::foundation::ChangeNotifier::notify_listeners`.
    fn notify(&self) {
        let snapshot: Vec<TabChangeListener> = self
            .listeners
            .borrow()
            .iter()
            .map(|(_, listener)| Rc::clone(listener))
            .collect();
        for listener in snapshot {
            listener();
        }
    }
}

/// Coordinates tab selection for [`TabBar`](crate::TabBar) (and, in a later
/// unit, `TabBarView`). See the module docs for what this V1 does and does
/// not carry over.
///
/// `Clone` is cheap and shares identity: every clone observes and mutates
/// the *same* underlying state, the same shape as
/// [`crate::navigation_bar`]'s and `flui_cupertino::CupertinoTabController`'s
/// shared-handle controllers (that crate is not a dependency of this one, so
/// this is a plain-text cross-reference, not a doc link).
///
/// ```
/// use flui_material::TabController;
///
/// let controller = TabController::new(3, 0);
/// assert_eq!(controller.index(), 0);
/// controller.set_index(2);
/// assert_eq!(controller.index(), 2);
/// assert_eq!(controller.previous_index(), 0);
/// ```
pub struct TabController {
    state: Rc<Cell<IndexPair>>,
    length: usize,
    listeners: TabListenerRegistry,
}

/// Rejects an illegal construction-time tab index in every build profile.
///
/// When `length == 0`, only `index == 0` is accepted. See
/// `ARCHITECTURE.md` §TabController.
#[inline]
fn assert_construction_tab_index(length: usize, index: usize, context: &str) {
    assert!(
        (length == 0 && index == 0) || index < length,
        "{context}: index {index} is out of range for length {length}"
    );
}

/// Rejects an out-of-range `set_index` argument (`value < length || length ==
/// 0` must hold), as a release `assert!`.
///
/// When `length == 0`, any `index` passes this gate and then hits the
/// `length < 2` no-op.
#[inline]
fn assert_set_index_in_range(length: usize, index: usize) {
    assert!(
        index < length || length == 0,
        "TabController::set_index: index {index} is out of range for length {length}"
    );
}

impl TabController {
    /// A controller over `length` tabs, starting at `initial_index`.
    ///
    /// # Panics
    ///
    /// Panics if `initial_index` is out of range for `length` (only `0` is
    /// valid when `length == 0`). Construction bounds are enforced in every
    /// build profile — see `ARCHITECTURE.md` §TabController length/index.
    #[must_use]
    pub fn new(length: usize, initial_index: usize) -> Self {
        assert_construction_tab_index(length, initial_index, "TabController::new");
        Self::with_previous(length, initial_index, initial_index)
    }

    /// A controller starting at `index` with a distinct `previous_index` —
    /// the shape [`DefaultTabController`]'s length-change re-creation needs
    /// (see that type's docs).
    ///
    /// `index` must already be in range for `length`; callers that shrink
    /// length clamp first (see `recreate_for_length_change`).
    fn with_previous(length: usize, index: usize, previous_index: usize) -> Self {
        assert_construction_tab_index(length, index, "TabController::with_previous");
        Self {
            state: Rc::new(Cell::new(IndexPair {
                index,
                previous_index,
            })),
            length,
            listeners: TabListenerRegistry::default(),
        }
    }

    /// The index of the currently selected tab.
    #[must_use]
    pub fn index(&self) -> usize {
        self.state.get().index
    }

    /// The index of the previously selected tab. Initially equal to
    /// [`index`](Self::index).
    #[must_use]
    pub fn previous_index(&self) -> usize {
        self.state.get().previous_index
    }

    /// The total number of tabs this controller was created for. Immutable
    /// for the lifetime of one `TabController` instance — a change in tab
    /// count is a *new* controller identity, not a mutation (see
    /// [`DefaultTabController`]'s docs).
    #[must_use]
    pub fn length(&self) -> usize {
        self.length
    }

    /// Selects `index`, updating [`previous_index`](Self::previous_index)
    /// and notifying listeners — unless this is a no-op.
    ///
    /// The non-animating branch; see the module docs for what is deferred.
    /// **No-op, no notify** when `index == self.index()` OR
    /// `self.length() < 2` — both conditions are checked before
    /// touching any state.
    /// The pair update itself is a single [`Cell::set`] of the whole
    /// `(index, previous_index)` tuple, so a listener invoked by the
    /// `notify_listeners()` that follows always observes a consistent pair
    /// — never `index` updated with a stale `previous_index` or vice versa.
    ///
    /// # Panics
    ///
    /// Panics if `index >= length` when `length > 0` (in every build
    /// profile). When `length == 0` (or `length == 1`), the call is a no-op
    /// after the check.
    ///
    /// This panic is **not** wrapped by the build-error `ErrorView` boundary:
    /// it fires on the caller's thread (gesture / app code). Length mismatch
    /// asserts inside `TabBar` / `TabBarView::build` recover as `ErrorView`.
    pub fn set_index(&self, index: usize) {
        assert_set_index_in_range(self.length, index);
        let current = self.state.get();
        if index == current.index || self.length < 2 {
            return;
        }
        self.state.set(IndexPair {
            index,
            previous_index: current.index,
        });
        self.listeners.notify();
    }

    /// An alias for [`set_index`](Self::set_index) — see the module docs'
    /// "`animate_to` is a documented alias" section for exactly what the
    /// oracle's `animateTo` does that this does not (yet) do.
    pub fn animate_to(&self, index: usize) {
        self.set_index(index);
    }

    /// Registers a listener, called whenever [`set_index`](Self::set_index)
    /// actually changes the selection. `listener` is `Rc`-based internally
    /// (see the module docs' "listener registry follows the same logic"
    /// section) — it may freely capture this same `TabController` (or any
    /// other owner-local, `!Send` state) to read the just-updated `(index,
    /// previous_index)` pair.
    pub fn add_listener(&self, listener: impl Fn() + 'static) -> ListenerId {
        self.listeners.add_listener(Rc::new(listener))
    }

    /// Unregisters a previously-registered listener.
    pub fn remove_listener(&self, id: ListenerId) {
        self.listeners.remove_listener(id);
    }

    /// The number of currently-registered listeners. Mainly a test seam —
    /// mirrors `flui_sdk::foundation::ChangeNotifier::len`'s own reason for
    /// existing: proving a consumer's `dispose()` actually unregisters
    /// (rather than leaking) is otherwise unobservable from outside.
    #[must_use]
    pub fn listener_count(&self) -> usize {
        self.listeners.len()
    }
}

impl Clone for TabController {
    fn clone(&self) -> Self {
        Self {
            state: Rc::clone(&self.state),
            length: self.length,
            listeners: self.listeners.clone(),
        }
    }
}

/// Identity equality — two clones of the same controller are equal; two
/// independently-constructed controllers are not, even with identical
/// `(index, previous_index, length)`. The scope's `update_should_notify`
/// relies on this reference-identity comparison, implemented as `Rc::ptr_eq`
/// over the shared cell.
impl PartialEq for TabController {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }
}

impl Eq for TabController {}

impl std::fmt::Debug for TabController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pair = self.state.get();
        f.debug_struct("TabController")
            .field("index", &pair.index)
            .field("previous_index", &pair.previous_index)
            .field("length", &self.length)
            .finish_non_exhaustive()
    }
}

/// The inherited node [`DefaultTabController`] publishes its
/// [`TabController`] through. A private `InheritedView`, since nothing
/// outside this module should construct one directly,
/// only read it via [`DefaultTabController::of`]/[`maybe_of`](DefaultTabController::maybe_of).
#[derive(Clone)]
struct TabControllerScope {
    controller: TabController,
    child: BoxedView,
}

impl InheritedView for TabControllerScope {
    type Data = TabController;

    fn data(&self) -> &Self::Data {
        &self.controller
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        // Only controller identity is compared: no consumer here depends
        // on ticker-mode-gated notification yet (this controller has no
        // ticker to gate); see `DefaultTabController`'s docs.
        self.controller != old.controller
    }
}

impl_inherited_view!(TabControllerScope);

/// Shares one [`TabController`] with a `TabBar`/`TabBarView` pair that don't
/// have a convenient stateful ancestor to own it directly.
///
/// # Length-change re-creation
///
/// Rebuilding with a different `length` does not mutate the existing
/// controller's `length` field in place (`TabController::length` is
/// immutable per instance — see that type's docs). Instead, exactly like the
/// oracle's `_copyWithAndDispose`, this creates a **new** `TabController`
/// identity:
///
/// - If the old `index` is still in range for the new `length`, it carries
///   over unchanged, and `previous_index` carries over unchanged too.
/// - If the old `index` is now out of range (the tab list shrank past it),
///   the new controller clamps to `length - 1` and records the *old* index
///   as its `previous_index` (`new_index = max(0, length - 1);
///   previous_index = old.index`).
///
/// Because [`TabController`]'s equality is identity-based (see its `PartialEq`
/// doc), this re-creation is itself what makes the private
/// `_TabControllerScope::update_should_notify` equivalent fire: dependents (a
/// `TabBar` re-reading [`DefaultTabController::maybe_of`] every `build`) see
/// a *different* controller and re-resolve their listener subscription onto
/// it — see `crate::tabs::TabBarState`'s `resolve_controller` for the
/// consumer side of that contract.
///
/// ```
/// use flui_material::DefaultTabController;
/// use flui_sdk::widgets::SizedBox;
///
/// let _root = DefaultTabController::new(3, SizedBox::shrink());
/// ```
#[derive(Clone, StatefulView)]
pub struct DefaultTabController {
    length: usize,
    initial_index: usize,
    child: BoxedView,
}

impl DefaultTabController {
    /// A `DefaultTabController` over `length` tabs (starting at index `0`)
    /// wrapping `child`.
    #[must_use]
    pub fn new(length: usize, child: impl IntoView) -> Self {
        Self {
            length,
            initial_index: 0,
            child: child.into_view().boxed(),
        }
    }

    /// Overrides the initially-selected tab. Defaults to `0`.
    ///
    /// # Panics
    ///
    /// Panics if `initial_index` is out of range for this controller's
    /// `length` (only `0` is valid when `length == 0`).
    #[must_use]
    pub fn initial_index(mut self, initial_index: usize) -> Self {
        assert_construction_tab_index(
            self.length,
            initial_index,
            "DefaultTabController::initial_index",
        );
        self.initial_index = initial_index;
        self
    }

    /// The closest ancestor [`DefaultTabController`]'s [`TabController`],
    /// registering a dependency so this element rebuilds when the
    /// controller identity changes.
    ///
    /// # Panics
    ///
    /// Panics if there is no `DefaultTabController` ancestor.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> TabController {
        Self::maybe_of(ctx).expect(
            "DefaultTabController::of called with a context that has no DefaultTabController \
             ancestor — wrap the subtree in a DefaultTabController, or pass an explicit \
             TabController instead",
        )
    }

    /// Like [`of`](Self::of), but returns `None` instead of panicking when
    /// there is no ancestor.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<TabController> {
        ctx.depend_on::<TabControllerScope, _>(|scope| scope.controller.clone())
    }
}

impl std::fmt::Debug for DefaultTabController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultTabController")
            .field("length", &self.length)
            .field("initial_index", &self.initial_index)
            .finish_non_exhaustive()
    }
}

/// Persistent state behind [`DefaultTabController`]: the [`TabController`]
/// it owns, re-created (not mutated) on a `length` change — see
/// [`DefaultTabController`]'s docs.
pub struct DefaultTabControllerState {
    controller: TabController,
}

impl std::fmt::Debug for DefaultTabControllerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultTabControllerState")
            .field("controller", &self.controller)
            .finish()
    }
}

impl StatefulView for DefaultTabController {
    type State = DefaultTabControllerState;

    fn create_state(&self) -> Self::State {
        DefaultTabControllerState {
            controller: TabController::new(self.length, self.initial_index),
        }
    }
}

impl ViewState<DefaultTabController> for DefaultTabControllerState {
    fn did_update_view(
        &mut self,
        old_view: &DefaultTabController,
        new_view: &DefaultTabController,
    ) {
        if new_view.length == old_view.length {
            return;
        }
        self.controller = recreate_for_length_change(&self.controller, new_view.length);
    }

    fn build(&self, view: &DefaultTabController, _ctx: &dyn BuildContext) -> impl IntoView {
        TabControllerScope {
            controller: self.controller.clone(),
            child: view.child.clone(),
        }
    }
}

/// The re-creation rule for a `length` change — extracted from
/// [`DefaultTabControllerState::did_update_view`] so it is unit-testable in
/// isolation. See [`DefaultTabController`]'s docs for the full contract.
fn recreate_for_length_change(old: &TabController, new_length: usize) -> TabController {
    let old_index = old.index();
    if old_index >= new_length {
        let clamped = new_length.saturating_sub(1);
        TabController::with_previous(new_length, clamped, old_index)
    } else {
        TabController::with_previous(new_length, old_index, old.previous_index())
    }
}
