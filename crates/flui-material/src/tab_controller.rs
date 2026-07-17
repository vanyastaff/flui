//! [`TabController`] — shared selection state for [`crate::TabBar`], plus
//! [`DefaultTabController`], the inherited-widget shortcut that owns one for
//! a subtree.
//!
//! # Flutter parity
//!
//! `material/tab_controller.dart`'s `TabController`/`DefaultTabController`
//! (oracle tag `3.44.0`).
//!
//! # `TabController`: one non-`Send` cell, not an `Arc<AtomicUsize>` pair
//!
//! The oracle's `TabController` is a `ChangeNotifier` (Dart has no
//! `Send`/`Sync` distinction) holding two plain `int` fields, `_index` and
//! `_previousIndex`, mutated together by `_changeIndex` and then notified
//! once. A naive Rust port — following
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
//! cannot) implement [`Listenable`](flui_foundation::Listenable) — that
//! trait requires `Send + Sync` — which is exactly the point: the compiler
//! now enforces single-realm use instead of a doc comment promising it.
//!
//! The listener registry follows the same logic: [`TabController::add_listener`]
//! takes a plain `Rc<dyn Fn()>` (this crate's usual owner-local callback
//! shape — see [`crate::ink_well::InkWell::on_tap`]), not
//! `flui_foundation::ListenerCallback` (`Arc<dyn Fn() + Send + Sync>`). A
//! `Send + Sync`-bound callback could never legally capture this
//! `TabController` (or its `Rc<Cell<_>>` state) to begin with, so reusing
//! `flui_foundation::ChangeNotifier` here would make the *listener*
//! unable to read back the very state it was notified about.
//! [`TabBar`](crate::TabBar) subscribes with a plain closure that schedules
//! a rebuild via [`flui_view::RebuildHandle`] (itself `Send + Sync`, but
//! that is incidental — nothing about the registry requires it).
//!
//! # `animate_to` is a documented alias, not an animation
//!
//! The oracle's `animateTo`/`_changeIndex` additionally: (a) fires **two**
//! `notifyListeners()` calls when a `duration` is given — once immediately
//! (so [`indexIsChanging`](https://api.flutter.dev/flutter/material/TabController/indexIsChanging.html)
//! flips true) and once when the drive animation completes; (b) exposes
//! `indexIsChanging`, read by `TabBarView`'s warp-to-adjacent-page
//! optimization to distinguish a programmatic tab change from a drag; (c)
//! animates `animation.value` from the old index to the new one over
//! `duration`/`curve`. None of that is ported here: this V1 has no
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

use flui_foundation::ListenerId;
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, impl_inherited_view};

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
/// `on_tap`), NOT `flui_foundation::ListenerCallback`
/// (`Arc<dyn Fn() + Send + Sync>`). This is deliberate, not an oversight: a
/// `Send + Sync` callback could never legally capture a `TabController` (or
/// anything reachable from its `Rc<Cell<_>>` state) in the first place, so a
/// `TabController`-flavored listener registry needs its own `!Send`
/// callback type rather than reusing `flui_foundation::ChangeNotifier`'s.
type TabChangeListener = Rc<dyn Fn()>;

/// [`TabController`]'s own listener registry — a private, `!Send`
/// counterpart to `flui_foundation::ChangeNotifier` (see
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
    /// `flui_foundation::ChangeNotifier::notify_listeners`.
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
/// unit, `TabBarView`). Flutter parity: `TabController`
/// (`tab_controller.dart`, oracle tag `3.44.0`) — see the module docs for
/// what this V1 does and does not carry over.
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

impl TabController {
    /// A controller over `length` tabs, starting at `initial_index`.
    ///
    /// Flutter parity: `TabController`'s constructor asserts (`length >= 0`
    /// is implied by `usize`; `initialIndex` valid for `length`) — ported as
    /// a `debug_assert!`, matching this crate's other oracle-assert ports
    /// (e.g. `flui_cupertino::CupertinoTabScaffold`'s index-bounds check).
    #[must_use]
    pub fn new(length: usize, initial_index: usize) -> Self {
        debug_assert!(
            (length == 0 && initial_index == 0) || initial_index < length,
            "TabController: initial_index {initial_index} is out of range for length {length}"
        );
        Self::with_previous(length, initial_index, initial_index)
    }

    /// A controller starting at `index` with a distinct `previous_index` —
    /// the shape [`DefaultTabController`]'s length-change re-creation needs
    /// (Flutter parity: `TabController._copyWithAndDispose`'s
    /// `index`/`previousIndex` pair, see that type's docs).
    fn with_previous(length: usize, index: usize, previous_index: usize) -> Self {
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
    /// Flutter parity: `TabController._changeIndex`'s core (the
    /// non-animating branch; see the module docs for what is deferred).
    /// **No-op, no notify** when `index == self.index()` OR
    /// `self.length() < 2` — both conditions the oracle checks before
    /// touching any state (`if (value == _index || length < 2) return;`).
    /// The pair update itself is a single [`Cell::set`] of the whole
    /// `(index, previous_index)` tuple, so a listener invoked by the
    /// `notify_listeners()` that follows always observes a consistent pair
    /// — never `index` updated with a stale `previous_index` or vice versa.
    ///
    /// Flutter parity: `_changeIndex`'s own bounds assert (`assert(value >=
    /// 0 && (value < length || length == 0))`) is ported as a
    /// `debug_assert!` here too, matching [`new`](Self::new)'s — `index`
    /// must be in range for [`length`](Self::length) (or `length` must be
    /// `0`, in which case only `index == 0` ever reaches this far since the
    /// `length < 2` no-op guard below returns first).
    pub fn set_index(&self, index: usize) {
        debug_assert!(
            index < self.length || self.length == 0,
            "TabController::set_index: index {index} is out of range for length {}",
            self.length
        );
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
    /// mirrors `flui_foundation::ChangeNotifier::len`'s own reason for
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
/// `(index, previous_index, length)`. Flutter parity: the oracle's
/// `TabController` has no `==` override, so Dart's default reference
/// equality applies to `_TabControllerScope.updateShouldNotify`'s `controller
/// != old.controller` check — this is that same reference-identity
/// comparison, ported as `Rc::ptr_eq` over the shared cell.
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
/// [`TabController`] through. Flutter parity: `_TabControllerScope` — a
/// private `InheritedWidget`, ported as a private `InheritedView` for the
/// same reason: nothing outside this module should construct one directly,
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
        // Flutter parity: `_TabControllerScope.updateShouldNotify` also
        // compares `enabled` (a `TickerMode.of(context)` snapshot) — no
        // consumer here depends on ticker-mode-gated notification yet (this
        // controller has no ticker to gate), so only the controller-identity
        // half of that comparison is ported; see `DefaultTabController`'s
        // docs.
        self.controller != old.controller
    }
}

impl_inherited_view!(TabControllerScope);

/// Shares one [`TabController`] with a `TabBar`/`TabBarView` pair that don't
/// have a convenient stateful ancestor to own it directly.
///
/// Flutter parity: `DefaultTabController` (`tab_controller.dart`, oracle tag
/// `3.44.0`).
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
///   as its `previous_index` — Flutter parity: `newIndex = max(0,
///   widget.length - 1); previousIndex = _controller.index;`
///   (`_DefaultTabControllerState.didUpdateWidget`).
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
/// use flui_widgets::SizedBox;
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
    #[must_use]
    pub fn initial_index(mut self, initial_index: usize) -> Self {
        self.initial_index = initial_index;
        self
    }

    /// The closest ancestor [`DefaultTabController`]'s [`TabController`],
    /// registering a dependency so this element rebuilds when the
    /// controller identity changes.
    ///
    /// # Panics
    ///
    /// Panics if there is no `DefaultTabController` ancestor. Flutter
    /// parity: `DefaultTabController.of`, which throws a `FlutterError` in
    /// release mode (an `assert` in debug) under the identical condition.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> TabController {
        Self::maybe_of(ctx).expect(
            "DefaultTabController::of called with a context that has no DefaultTabController \
             ancestor — wrap the subtree in a DefaultTabController, or pass an explicit \
             TabController instead",
        )
    }

    /// Like [`of`](Self::of), but returns `None` instead of panicking when
    /// there is no ancestor. Flutter parity: `DefaultTabController.maybeOf`.
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
/// isolation. Flutter parity:
/// `_DefaultTabControllerState.didUpdateWidget`'s `newIndex`/`previousIndex`
/// computation (`tab_controller.dart`, oracle tag `3.44.0`); see
/// [`DefaultTabController`]'s docs for the full contract.
fn recreate_for_length_change(old: &TabController, new_length: usize) -> TabController {
    let old_index = old.index();
    if old_index >= new_length {
        let clamped = new_length.saturating_sub(1);
        TabController::with_previous(new_length, clamped, old_index)
    } else {
        TabController::with_previous(new_length, old_index, old.previous_index())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A listener that counts its own invocations. `Rc<Cell<usize>>`, not
    /// `Arc<AtomicUsize>` — [`TabController::add_listener`] takes a plain
    /// `!Send` closure (see the module docs), so nothing here needs to be
    /// thread-safe.
    fn counting_listener() -> (impl Fn() + 'static, Rc<Cell<usize>>) {
        let count = Rc::new(Cell::new(0usize));
        let count_for_listener = Rc::clone(&count);
        let listener = move || {
            count_for_listener.set(count_for_listener.get() + 1);
        };
        (listener, count)
    }

    #[test]
    fn new_starts_with_index_equal_to_previous_index() {
        let controller = TabController::new(3, 1);
        assert_eq!(controller.index(), 1);
        assert_eq!(controller.previous_index(), 1);
        assert_eq!(controller.length(), 3);
    }

    #[test]
    fn set_index_updates_index_and_previous_index() {
        let controller = TabController::new(3, 0);
        controller.set_index(2);
        assert_eq!(controller.index(), 2);
        assert_eq!(controller.previous_index(), 0);
    }

    #[test]
    fn set_index_notifies_listeners_on_a_real_change() {
        let controller = TabController::new(3, 0);
        let (listener, count) = counting_listener();
        let _id = controller.add_listener(listener);

        controller.set_index(1);

        assert_eq!(count.get(), 1);
    }

    /// Red-check: if `set_index` stopped checking `index == current.index`
    /// and always wrote + notified, this would observe a spurious notify.
    #[test]
    fn set_index_is_a_no_op_when_the_index_does_not_change() {
        let controller = TabController::new(3, 1);
        let (listener, count) = counting_listener();
        let _id = controller.add_listener(listener);

        controller.set_index(1);

        assert_eq!(count.get(), 0);
        assert_eq!(
            controller.previous_index(),
            1,
            "previous_index must not move on a no-op"
        );
    }

    /// Red-check: if `set_index` dropped the `length < 2` guard, a
    /// single-tab controller would incorrectly accept `set_index(0)` as a
    /// "change" the first time and notify.
    #[test]
    fn set_index_is_a_no_op_when_length_is_below_two() {
        let controller = TabController::new(1, 0);
        let (listener, count) = counting_listener();
        let _id = controller.add_listener(listener);

        controller.set_index(0);

        assert_eq!(count.get(), 0);
    }

    #[test]
    fn set_index_on_a_zero_length_controller_is_a_no_op() {
        let controller = TabController::new(0, 0);
        let (listener, count) = counting_listener();
        let _id = controller.add_listener(listener);

        controller.set_index(0);

        assert_eq!(count.get(), 0);
        assert_eq!(controller.index(), 0);
    }

    /// Flutter parity: `_changeIndex`'s own bounds assert. Red-check: delete
    /// the `debug_assert!` in `set_index` — this test stops panicking and
    /// `set_index(7)` on a length-3 controller instead silently stores 7 and
    /// notifies.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "index 7 is out of range for length 3")]
    fn set_index_out_of_range_debug_asserts() {
        let controller = TabController::new(3, 0);
        controller.set_index(7);
    }

    #[test]
    fn a_listener_reading_both_fields_mid_notify_sees_a_consistent_pair() {
        // The pair is a single Cell::set before notify_listeners fires, so a
        // listener invoked BY that notify must see index/previous_index
        // already updated together, never a torn half-update.
        let controller = TabController::new(3, 0);
        let observed = Rc::new(Cell::new(None::<(usize, usize)>));
        let observed_for_listener = Rc::clone(&observed);
        let controller_for_listener = controller.clone();
        let _id = controller.add_listener(move || {
            observed_for_listener.set(Some((
                controller_for_listener.index(),
                controller_for_listener.previous_index(),
            )));
        });

        controller.set_index(2);

        assert_eq!(observed.get(), Some((2, 0)));
    }

    #[test]
    fn animate_to_is_an_alias_for_set_index() {
        let controller = TabController::new(3, 0);
        controller.animate_to(2);
        assert_eq!(controller.index(), 2);
        assert_eq!(controller.previous_index(), 0);
    }

    #[test]
    fn remove_listener_stops_future_notifications() {
        let controller = TabController::new(3, 0);
        let (listener, count) = counting_listener();
        let id = controller.add_listener(listener);
        controller.remove_listener(id);

        controller.set_index(1);

        assert_eq!(count.get(), 0);
    }

    #[test]
    fn clones_share_identity() {
        let a = TabController::new(3, 0);
        let b = a.clone();
        assert_eq!(a, b);

        b.set_index(2);
        assert_eq!(
            a.index(),
            2,
            "a clone must observe the other clone's mutation"
        );
    }

    #[test]
    fn independently_constructed_controllers_are_not_equal() {
        let a = TabController::new(3, 0);
        let b = TabController::new(3, 0);
        assert_ne!(a, b, "identical (index, length) does not imply identity");
    }

    #[test]
    fn recreate_for_length_change_carries_a_still_valid_index_over_unchanged() {
        let old = TabController::new(5, 2);
        old.set_index(3);

        let recreated = recreate_for_length_change(&old, 4);

        assert_eq!(recreated.length(), 4);
        assert_eq!(recreated.index(), 3, "3 is still < 4, so it carries over");
        assert_eq!(recreated.previous_index(), old.previous_index());
        assert_ne!(recreated, old, "recreation must produce a new identity");
    }

    /// Red-check: if the clamp used `new_length` instead of `new_length -
    /// 1`, this would accept an out-of-range index equal to the new length.
    #[test]
    fn recreate_for_length_change_clamps_an_out_of_range_index_to_the_last_tab() {
        let old = TabController::new(5, 4);

        let recreated = recreate_for_length_change(&old, 2);

        assert_eq!(recreated.length(), 2);
        assert_eq!(recreated.index(), 1, "clamped to length - 1");
        assert_eq!(
            recreated.previous_index(),
            4,
            "the old out-of-range index becomes previous_index"
        );
    }

    #[test]
    fn recreate_for_length_change_to_zero_clamps_to_zero() {
        let old = TabController::new(3, 2);

        let recreated = recreate_for_length_change(&old, 0);

        assert_eq!(recreated.length(), 0);
        assert_eq!(recreated.index(), 0);
        assert_eq!(recreated.previous_index(), 2);
    }

    #[test]
    fn default_tab_controller_new_leaves_initial_index_at_zero() {
        let root = DefaultTabController::new(3, flui_widgets::SizedBox::shrink());
        assert_eq!(root.length, 3);
        assert_eq!(root.initial_index, 0);
    }

    #[test]
    fn default_tab_controller_initial_index_overrides_the_start() {
        let root = DefaultTabController::new(3, flui_widgets::SizedBox::shrink()).initial_index(2);
        assert_eq!(root.initial_index, 2);
    }

    #[test]
    fn debug_format_does_not_panic() {
        let controller = TabController::new(3, 1);
        let rendered = format!("{controller:?}");
        assert!(rendered.contains("TabController"));

        let root = DefaultTabController::new(3, flui_widgets::SizedBox::shrink());
        let rendered = format!("{root:?}");
        assert!(rendered.contains("DefaultTabController"));
    }
}
