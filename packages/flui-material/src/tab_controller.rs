//! [`TabController`] — shared selection state and tab position for
//! [`crate::TabBar`] and [`crate::TabBarView`], plus [`DefaultTabController`],
//! the inherited-widget shortcut that owns one for a subtree.
//!
//! # One owner-local handle
//!
//! Every clone of a [`TabController`] shares one `Rc`: the committed
//! `(index, previous_index)` pair (one `Cell` of a packed pair, so a listener
//! never observes one field updated without the other), the index listeners,
//! and the position source. The handle is `!Send`; it lives on the UI runtime
//! that created it.
//!
//! # `index` and `position`: two values, one source of truth
//!
//! [`TabController::index`] is the committed selection, the value labels and
//! semantics read. [`TabController::position`] is the continuous, fractional
//! position the indicator paints: `1.5` is half way between the second and
//! third tab. Position is *derived*, never written back:
//!
//! - **No [`crate::TabBarView`] attached** — an owned `AnimatedValue<f64>`.
//!   [`TabController::animate_to`] retargets it with the controller's motion
//!   (300 ms ease-in-out unless [`TabController::with_motion`] says
//!   otherwise); [`TabController::set_index`] snaps it. It runs on the
//!   ambient `VsyncScope` clock that a `TabBar` or `DefaultTabController`
//!   binds it to, so reduced motion settles it on the next frame and a
//!   subtree with no clock settles it at once.
//! - **A `TabBarView` attached** — the view's `PageController::page()`. A tap
//!   commits `index` and animates the pager; a swipe moves the pager
//!   directly, and `index` commits only once the pager stops scrolling, to
//!   the page it rests on. During a drag `index` does not change. Reduced
//!   motion does not affect a swipe: following the finger is direct
//!   manipulation, not animation.
//!
//! So there is no `offset` to write back and no `index_is_changing` counter:
//! anything that needs "how far between tabs" reads `position()`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use flui_sdk::animation::{
    AnimatedValue, AnimatedValueView, Animation as _, ArcCurve, Curve as _, MotionSpec, Vsync,
    curves,
};
use flui_sdk::foundation::panic::PanicRecovery;
use flui_sdk::foundation::{ChangeNotifier, Listenable, ListenerId, ListenerObserver};
use flui_sdk::pipeline::ScrollDirection;
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, InheritedView, impl_inherited_view};
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::{
    Alternative, CompanionLink, CompanionRegistration, CompanionSpec, GestureAlternatives,
    GestureCommand, Intent, MotionBuilder, PageController, SemanticsAnchor, SingleActivator,
    ViewportFraction,
};

/// Select the tab after or before the selected one: what a
/// [`TabBarView`](crate::TabBarView)'s commands perform without a swipe, and
/// what tapping the adjacent tab of a [`TabBar`](crate::TabBar) does.
///
/// The view answers it through the controller, not by turning its pager: a
/// controller commits the selection when the user lets go of the pager, so a
/// page turned by the pager's own intent would leave the selection behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TabStep {
    /// The tab after the selected one.
    Next,
    /// The tab before the selected one.
    Previous,
}

impl Intent for TabStep {}

impl TabStep {
    /// The tab this step selects from `index` among `length` tabs; `None`
    /// past either end.
    pub(crate) fn target(self, index: usize, length: usize) -> Option<usize> {
        match self {
            Self::Next => index.checked_add(1).filter(|next| *next < length),
            Self::Previous => index.checked_sub(1),
        }
    }
}

/// `(index, previous_index)`, mutated and read as one unit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct IndexPair {
    index: usize,
    previous_index: usize,
}

/// A [`TabController`] index listener: an owner-local `Rc` callback that may
/// capture the controller itself.
type TabChangeListener = Rc<dyn Fn()>;

/// [`TabController`]'s index listener registry.
#[derive(Default)]
struct TabListenerRegistry {
    listeners: RefCell<Vec<(ListenerId, TabChangeListener)>>,
    /// The last id handed out; `ListenerId` is 1-based.
    last_id: Cell<usize>,
}

impl TabListenerRegistry {
    fn add_listener(&self, listener: TabChangeListener) -> ListenerId {
        let next = self
            .last_id
            .get()
            .checked_add(1)
            .expect("BUG: tab listener ids are exhausted");
        self.last_id.set(next);
        let id = ListenerId::new(next);
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

    /// Fires every registered listener from a snapshot, so a listener that
    /// adds or removes a listener does not collide with the borrow.
    fn notify(&self) {
        let snapshot: Vec<TabChangeListener> = self
            .listeners
            .borrow()
            .iter()
            .map(|(_, listener)| Rc::clone(listener))
            .collect();
        // Every listener hears the change; the first panic is resumed after.
        // A snapshot entry may be the last owner of its captures, so it is
        // released under the same recovery as the call.
        let mut recovery = PanicRecovery::new();
        for listener in snapshot {
            recovery.run(|| listener());
            recovery.retire(listener);
        }
        recovery.finish();
    }
}

/// How a selection change moves the position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    /// Straight to the new index.
    Jump,
    /// Over the controller's motion.
    Animate,
}

/// The attached `TabBarView`'s pager and this controller's subscriptions to
/// it.
struct PagerLink {
    pager: PageController,
    pixels: Rc<dyn Listenable>,
    pixel_listener: ListenerId,
    activity_listener: ListenerId,
}

impl PagerLink {
    fn unsubscribe(&self) {
        self.pixels.remove_listener(self.pixel_listener);
        self.pager
            .position()
            .remove_activity_listener(self.activity_listener);
    }

    fn is(&self, pager: &PageController) -> bool {
        Rc::ptr_eq(&self.pixels, &pager.listenable())
    }
}

/// Everything a [`TabController`] and its clones share.
struct Shared {
    length: usize,
    pair: Cell<IndexPair>,
    index_listeners: TabListenerRegistry,
    /// Tells the step actions their enablement may have changed: whether
    /// there is a tab before or after depends on the selected index.
    step_changes: ChangeNotifier,
    /// The owned position source. Taken out while it is driven, so a
    /// listener that re-enters the controller never meets a held borrow.
    owner: RefCell<Option<AnimatedValue<f64>>>,
    /// The owned source's published value, readable while the owner is out.
    owned: AnimatedValueView<f64>,
    /// A drive requested while the owner was out; the outer drive applies it.
    pending_drive: Cell<Option<(f64, Change)>>,
    motion: RefCell<MotionSpec>,
    pager: RefCell<Option<PagerLink>>,
    /// Set while this controller moves the pager. Moving it can stop the
    /// pager's current activity synchronously, and that stop must not commit
    /// the page it was interrupted on over the index just committed.
    driving_pager: Cell<bool>,
    /// Whether the pager's current scroll activity is the user's: a drag or
    /// its fling, which sets a user scroll direction, as opposed to a run this
    /// controller started or a stop caused by teardown. Only a user activity
    /// commits the page it settles on.
    user_scrolling: Cell<bool>,
    /// Set while the attached view is deactivated or being torn down: the
    /// pager's activity then ends because its owner is leaving, not because
    /// the user let go.
    suspended: Cell<bool>,
    /// Whether controller-driven pager changes settle at once (reduced
    /// motion), as the owned position does.
    reduce_pager: Cell<bool>,
    /// The clocks of the widgets that bound this controller, oldest first.
    /// The owned position follows the newest still-live one.
    clocks: RefCell<Vec<(u64, Option<Vsync>)>>,
    next_clock: Cell<u64>,
    /// Notifies whenever the position may have moved, from either source.
    position_listeners: ChangeNotifier,
    /// Where a [`TabBar`](crate::TabBar) registers as the single-pointer path
    /// of the attached [`TabBarView`](crate::TabBarView)'s step commands.
    tab_steps: CompanionLink<TabStep>,
}

/// The default tab-change motion: 300 ms ease-in-out.
fn default_motion() -> MotionSpec {
    MotionSpec::Curve {
        duration: TabController::DEFAULT_DURATION,
        curve: ArcCurve::new(curves::EASE_IN_OUT),
    }
}

impl Shared {
    /// Tell the index listeners and the step actions the selection changed.
    fn notify_index_changed(&self) {
        self.index_listeners.notify();
        self.step_changes.notify_listeners();
    }

    fn index(&self) -> usize {
        self.pair.get().index
    }

    fn pager(&self) -> Option<PageController> {
        self.pager.borrow().as_ref().map(|link| link.pager.clone())
    }

    fn position(&self) -> f64 {
        match self.pager() {
            Some(pager) => pager.page().unwrap_or(self.index() as f64),
            None => self.owned.value(),
        }
    }

    /// Commits `index` as the selection and notifies, unless unchanged.
    fn commit(&self, index: usize) -> bool {
        let current = self.pair.get();
        if index == current.index {
            return false;
        }
        self.pair.set(IndexPair {
            index,
            previous_index: current.index,
        });
        true
    }

    /// Moves the position source toward the committed `index`.
    fn follow(&self, index: usize, change: Change) {
        let Some(pager) = self.pager() else {
            self.drive_owner(index as f64, change);
            return;
        };
        // The controller takes the pager over from whatever the user was doing.
        self.user_scrolling.set(false);
        let _driving = DrivingPager::enter(&self.driving_pager);
        let (duration, curve) = match change {
            // A zero-duration run jumps through the scroll controller, which
            // stops whatever run is driving the pager; `jump_to_page` only
            // writes the offset, and a run in flight would carry on past it.
            Change::Jump => (Duration::ZERO, ArcCurve::new(curves::LINEAR)),
            Change::Animate if self.reduce_pager.get() => {
                (Duration::ZERO, ArcCurve::new(curves::LINEAR))
            }
            Change::Animate => pager_motion(&self.motion.borrow()),
        };
        pager.animate_to_page(index, duration, curve);
        // A pager before its first layout or with a collapsed viewport moves
        // its page without publishing pixels.
        self.position_listeners.notify_listeners();
    }

    /// Retargets or snaps the owned source. Re-entry while the owner is out
    /// records the newest request, which the outer call applies before it
    /// puts the owner back — also when an observer of the step that recorded
    /// it panicked, since the nested call already committed its index. The
    /// first such panic is resumed once the requests are drained and the
    /// owner is back.
    fn drive_owner(&self, target: f64, change: Change) {
        let ran = self.with_owner(|shared, owner| shared.apply_drive(owner, target, change));
        if !ran {
            self.pending_drive.set(Some((target, change)));
        }
    }

    /// One drive step on the owner.
    fn apply_drive(&self, owner: &mut AnimatedValue<f64>, target: f64, change: Change) {
        let motion = self.motion.borrow().clone();
        let result = match change {
            Change::Jump => owner.snap_to(target),
            Change::Animate => owner.retarget(target, motion).map(drop),
        };
        // The index is committed, so a motion the core refuses (a custom
        // curve with a non-finite end slope) still lands on the target.
        if let Err(error) = result {
            tracing::warn!(%error, "TabController: position motion refused; snapping");
            if let Err(error) = owner.snap_to(target) {
                tracing::error!(%error, "TabController: position cannot reach its target");
            }
        }
    }

    /// Runs `first` on the owned source with the owner taken out of its cell,
    /// then applies every drive that observers recorded meanwhile, and puts
    /// the owner back. A panic in a step is held until the requests are
    /// drained, and resumed once the owner is back. Returns whether the owner
    /// was available (it is not while an outer call holds it).
    fn with_owner(&self, first: impl FnOnce(&Self, &mut AnimatedValue<f64>)) -> bool {
        let Some(owner) = self.owner.borrow_mut().take() else {
            return false;
        };
        let mut recovery = PanicRecovery::new();
        {
            let mut slot = OwnerSlot {
                cell: &self.owner,
                owner: Some(owner),
            };
            let owner = slot
                .owner
                .as_mut()
                .expect("BUG: the owner stays in its slot while it is driven");
            recovery.run(|| first(self, owner));
            while let Some((target, change)) = self.pending_drive.take() {
                recovery.run(|| self.apply_drive(owner, target, change));
            }
        }
        recovery.finish();
        true
    }

    /// Runs the owned position on the newest registered clock, or on none
    /// once the last registration has left.
    fn follow_newest_clock(&self) {
        let newest = self
            .clocks
            .borrow()
            .last()
            .and_then(|(_, vsync)| vsync.clone());
        self.rebind(newest.as_ref());
    }

    fn rebind(&self, vsync: Option<&Vsync>) {
        self.with_owner(|_, owner| {
            if let Err(error) = owner.rebind(vsync) {
                tracing::error!(%error, "TabController: position has no clock");
            }
        });
    }

    /// The pager's scroll activity changed. While the user scrolls it, note
    /// that; when that activity stops, commit the page it rests on. A stop
    /// the controller causes while moving the pager, or one from teardown
    /// interrupting a run of its own, is not the user settling a page.
    fn pager_activity_changed(&self) {
        let Some(pager) = self.pager() else {
            return;
        };
        let position = pager.position();
        if position.is_scrolling() {
            if position.user_scroll_direction() != ScrollDirection::Idle {
                self.user_scrolling.set(true);
            }
            return;
        }
        let user_stopped = self.user_scrolling.replace(false);
        if self.driving_pager.get() || self.suspended.get() || !user_stopped {
            return;
        }
        let Some(page) = pager.page().filter(|page| page.is_finite()) else {
            return;
        };
        let last = self.length.saturating_sub(1) as f64;
        // In range and integral after the clamp, so the cast is exact.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to [0, length - 1] and rounded"
        )]
        let settled = page.round().clamp(0.0, last) as usize;
        let committed = self.commit(settled);
        // Paging physics rest on a whole page; a wheel or touchpad pulse
        // writes an arbitrary offset and ends without them, so bring the
        // pager onto the page just committed.
        let resting = (page - settled as f64).abs() < 1e-6;
        let mut recovery = PanicRecovery::new();
        if !resting {
            recovery.run(|| self.follow(settled, Change::Animate));
        }
        if committed {
            recovery.run(|| self.notify_index_changed());
        }
        recovery.finish();
    }
}

/// One widget's registration of its clock with a [`TabController`]; dropping
/// it releases the clock.
pub(crate) struct ClockLease {
    shared: Weak<Shared>,
    id: u64,
}

impl ClockLease {
    /// The widget's ambient clock changed.
    pub(crate) fn set(&self, vsync: Option<Vsync>) {
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        if let Some(entry) = shared
            .clocks
            .borrow_mut()
            .iter_mut()
            .find(|(id, _)| *id == self.id)
        {
            entry.1 = vsync;
        }
        shared.follow_newest_clock();
    }
}

impl Drop for ClockLease {
    fn drop(&mut self) {
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        shared.clocks.borrow_mut().retain(|(id, _)| *id != self.id);
        shared.follow_newest_clock();
    }
}

impl std::fmt::Debug for ClockLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClockLease").finish_non_exhaustive()
    }
}

/// The pager's duration and curve for `motion`. A pager animates over a
/// duration and curve only, so a spring runs the default motion.
fn pager_motion(motion: &MotionSpec) -> (Duration, ArcCurve) {
    match motion {
        // The scroll controller drops a run whose curve the motion core
        // refuses, which would leave the pager on its old page; such a curve
        // jumps instead, as the owned position does.
        MotionSpec::Curve { curve, .. }
            if !(curve.slope(0.0).is_finite() && curve.slope(1.0).is_finite()) =>
        {
            (Duration::ZERO, ArcCurve::new(curves::LINEAR))
        }
        MotionSpec::Curve { duration, curve } => (*duration, curve.clone()),
        _ => (
            TabController::DEFAULT_DURATION,
            ArcCurve::new(curves::EASE_IN_OUT),
        ),
    }
}

/// Marks the controller as moving its pager for one call, clearing the mark
/// also when the call unwinds. Nested entries keep the outer mark.
struct DrivingPager<'a> {
    flag: &'a Cell<bool>,
    outer: bool,
}

impl<'a> DrivingPager<'a> {
    fn enter(flag: &'a Cell<bool>) -> Self {
        let outer = flag.replace(true);
        Self { flag, outer }
    }
}

impl Drop for DrivingPager<'_> {
    fn drop(&mut self) {
        self.flag.set(self.outer);
    }
}

/// Puts a taken-out owner back, also when driving it unwinds.
struct OwnerSlot<'a> {
    cell: &'a RefCell<Option<AnimatedValue<f64>>>,
    owner: Option<AnimatedValue<f64>>,
}

impl Drop for OwnerSlot<'_> {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.take() {
            *self.cell.borrow_mut() = Some(owner);
        }
    }
}

impl Listenable for Shared {
    fn add_observer(&self, observer: ListenerObserver) -> ListenerId {
        self.position_listeners.add_observer(observer)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.position_listeners.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.position_listeners.remove_all_listeners();
    }
}

/// Coordinates tab selection and position for [`TabBar`](crate::TabBar) and
/// [`TabBarView`](crate::TabBarView). See the module docs for how `index` and
/// [`position`](Self::position) relate.
///
/// `Clone` is cheap and shares identity: every clone observes and mutates the
/// same state.
///
/// ```
/// use flui_material::TabController;
///
/// let controller = TabController::new(3, 0);
/// assert_eq!(controller.index(), 0);
/// controller.set_index(2);
/// assert_eq!(controller.index(), 2);
/// assert_eq!(controller.previous_index(), 0);
/// assert_eq!(controller.position().value(), 2.0);
/// ```
#[derive(Clone)]
pub struct TabController {
    shared: Rc<Shared>,
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
    /// The duration of a tab change unless [`with_motion`](Self::with_motion)
    /// says otherwise, eased in and out.
    pub const DEFAULT_DURATION: Duration = Duration::from_millis(300);

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
        Self::with_previous(length, initial_index, initial_index, default_motion())
    }

    /// A controller starting at `index` with a distinct `previous_index` —
    /// the shape [`DefaultTabController`]'s length-change re-creation needs.
    ///
    /// `index` must already be in range for `length`; callers that shrink
    /// length clamp first (see `recreate_for_length_change`).
    fn with_previous(
        length: usize,
        index: usize,
        previous_index: usize,
        motion: MotionSpec,
    ) -> Self {
        assert_construction_tab_index(length, index, "TabController::with_previous");
        let owner = AnimatedValue::with_debug_label(
            "TabController::position",
            index as f64,
            motion.clone(),
            None,
        )
        .expect("BUG: a tab index is a finite position");
        let owned = owner.animation();
        let position_listeners = ChangeNotifier::new();
        let forward = position_listeners.clone();
        // The owner's notifications live as long as the owner, which this
        // controller holds; the subscription needs no explicit removal.
        let _forwarding = owned.add_observer(Rc::new(move |recovery| {
            forward.notify_listeners_with_recovery(recovery);
        }));
        Self {
            shared: Rc::new(Shared {
                length,
                pair: Cell::new(IndexPair {
                    index,
                    previous_index,
                }),
                index_listeners: TabListenerRegistry::default(),
                step_changes: ChangeNotifier::new(),
                owner: RefCell::new(Some(owner)),
                owned,
                pending_drive: Cell::new(None),
                motion: RefCell::new(motion),
                pager: RefCell::new(None),
                driving_pager: Cell::new(false),
                user_scrolling: Cell::new(false),
                suspended: Cell::new(false),
                reduce_pager: Cell::new(false),
                clocks: RefCell::new(Vec::new()),
                next_clock: Cell::new(0),
                position_listeners,
                tab_steps: CompanionLink::new(),
            }),
        }
    }

    /// How [`animate_to`](Self::animate_to) moves the position. Defaults to
    /// [`DEFAULT_DURATION`](Self::DEFAULT_DURATION) eased in and out. An
    /// attached [`TabBarView`](crate::TabBarView)'s pager animates over a
    /// duration and curve only, so it runs a spring as the default motion.
    #[must_use]
    pub fn with_motion(self, motion: MotionSpec) -> Self {
        self.set_motion(motion);
        self
    }

    pub(crate) fn set_motion(&self, motion: MotionSpec) {
        *self.shared.motion.borrow_mut() = motion;
    }

    /// Notified whenever the selected index changes: what a step action
    /// reports as the change of its enablement.
    pub(crate) fn step_changes(&self) -> ChangeNotifier {
        self.shared.step_changes.clone()
    }

    /// The index of the currently selected tab.
    #[must_use]
    pub fn index(&self) -> usize {
        self.shared.index()
    }

    /// The index of the previously selected tab. Initially equal to
    /// [`index`](Self::index).
    #[must_use]
    pub fn previous_index(&self) -> usize {
        self.shared.pair.get().previous_index
    }

    /// The total number of tabs this controller was created for. Immutable
    /// for the lifetime of one `TabController` instance — a change in tab
    /// count is a *new* controller identity, not a mutation (see
    /// [`DefaultTabController`]'s docs).
    #[must_use]
    pub fn length(&self) -> usize {
        self.shared.length
    }

    /// The continuous tab position: `index` at rest, fractional while a tab
    /// change animates or a [`TabBarView`](crate::TabBarView) is dragged.
    /// See the module docs for where it comes from.
    #[must_use]
    pub fn position(&self) -> TabPosition {
        TabPosition {
            shared: Rc::clone(&self.shared),
        }
    }

    /// Selects `index` immediately: the position jumps there.
    ///
    /// Updates [`previous_index`](Self::previous_index) and notifies
    /// listeners — unless `index == self.index()` or `self.length() < 2`, a
    /// no-op without a notification.
    ///
    /// # Panics
    ///
    /// Panics if `index >= length` when `length > 0` (in every build
    /// profile). This panic fires on the caller's thread (gesture / app
    /// code), outside the build-error `ErrorView` boundary.
    pub fn set_index(&self, index: usize) {
        self.change_index(index, Change::Jump);
    }

    /// Selects `index` and animates the position there over the controller's
    /// motion. `index` commits at once; listeners are notified once.
    ///
    /// # Panics
    ///
    /// As [`set_index`](Self::set_index).
    pub fn animate_to(&self, index: usize) {
        self.change_index(index, Change::Animate);
    }

    fn change_index(&self, index: usize, change: Change) {
        assert_set_index_in_range(self.shared.length, index);
        if self.shared.length < 2 || !self.shared.commit(index) {
            return;
        }
        // The index is committed, so its listeners hear of it even when a
        // position observer panics while the source moves; that first
        // failure is resumed once they have.
        let mut recovery = PanicRecovery::new();
        recovery.run(|| self.shared.follow(index, change));
        recovery.run(|| self.shared.notify_index_changed());
        recovery.finish();
    }

    /// Registers a listener, called whenever the selected index actually
    /// changes. It may capture this same `TabController` to read the
    /// just-updated `(index, previous_index)` pair.
    pub fn add_listener(&self, listener: impl Fn() + 'static) -> ListenerId {
        self.shared.index_listeners.add_listener(Rc::new(listener))
    }

    /// Unregisters a previously-registered listener.
    pub fn remove_listener(&self, id: ListenerId) {
        self.shared.index_listeners.remove_listener(id);
    }

    /// The number of currently-registered index listeners, so a consumer's
    /// `dispose()` can be shown to unregister.
    #[must_use]
    pub fn listener_count(&self) -> usize {
        self.shared.index_listeners.len()
    }

    /// Whether an attached pager settles at once on a controller-driven
    /// change, as the owned position does under reduced motion.
    pub(crate) fn set_reduce_pager(&self, reduce: bool) {
        self.shared.reduce_pager.set(reduce);
    }

    /// Registers `vsync`, the ambient clock of a widget that uses this
    /// controller, as the one the owned position runs on. With several
    /// widgets under different clocks the newest registration wins, and
    /// releasing it hands the position back to the one before.
    pub(crate) fn lease_clock(&self, vsync: Option<Vsync>) -> ClockLease {
        let id = self.shared.next_clock.get();
        self.shared.next_clock.set(id + 1);
        self.shared.clocks.borrow_mut().push((id, vsync));
        self.shared.follow_newest_clock();
        ClockLease {
            shared: Rc::downgrade(&self.shared),
            id,
        }
    }

    /// The alternatives to a swipe that a [`TabBarView`](crate::TabBarView) on
    /// this controller offers: "Previous tab" and "Next tab", each selecting
    /// the adjacent tab as tapping it does, and disabled at the ends. They
    /// draw no control. Their single-pointer path is the
    /// [`TabBar`](crate::TabBar) on this controller, which registers on the
    /// controller's companion link while it is mounted, so a view with no bar
    /// leaves them unfulfilled.
    ///
    /// The view builds these itself. They are public so that
    /// `flui_testing::assert_gesture_alternatives` can check the pair against
    /// a mounted view and bar.
    #[must_use]
    pub fn step_alternatives(&self) -> GestureAlternatives {
        GestureAlternatives::new(Alternative::Companion(
            CompanionSpec::new(
                GestureCommand::new("Previous tab", TabStep::Previous)
                    .key(SingleActivator::character("[").control()),
                &self.shared.tab_steps,
            )
            .command(
                GestureCommand::new("Next tab", TabStep::Next)
                    .key(SingleActivator::character("]").control()),
            ),
        ))
        .expect(
            "BUG: two distinct labels, both performing the intent the controller's link carries",
        )
    }

    /// Registers a tab bar as the companion of the step commands, until the
    /// registration drops. `tabs` holds the anchor each tab's node declares,
    /// in tab order. The tab a step performs is read from the selection when
    /// asked, so the bar follows it; none at either end.
    pub(crate) fn register_tab_bar(&self, tabs: Rc<[SemanticsAnchor]>) -> CompanionRegistration {
        // Weak: the registration lives in the controller's own link, and a
        // strong handle would make the controller own itself.
        let shared = Rc::downgrade(&self.shared);
        self.shared.tab_steps.register(move |step| {
            let shared = shared.upgrade()?;
            let target = step.target(shared.index(), shared.length)?;
            tabs.get(target).cloned()
        })
    }

    /// Makes a new pager at the current index the position source until the
    /// returned attachment drops, or returns `None` while another attachment
    /// holds it: the position has one source, so a controller drives at most
    /// one [`TabBarView`](crate::TabBarView) at a time.
    pub(crate) fn attach_pager(&self) -> Option<PagerAttachment> {
        if self.shared.pager.borrow().is_some() {
            return None;
        }
        let pager = PageController::with_params(self.index(), ViewportFraction::FULL);
        let pixels = pager.listenable();
        let forward = self.shared.position_listeners.clone();
        let pixel_listener = pixels.add_observer(Rc::new(move |recovery| {
            forward.notify_listeners_with_recovery(recovery);
        }));
        let weak = Rc::downgrade(&self.shared);
        let activity_listener = pager.position().add_activity_listener(Rc::new(move || {
            if let Some(shared) = weak.upgrade() {
                shared.pager_activity_changed();
            }
        }));
        *self.shared.pager.borrow_mut() = Some(PagerLink {
            pager: pager.clone(),
            pixels,
            pixel_listener,
            activity_listener,
        });
        Some(PagerAttachment {
            shared: Rc::downgrade(&self.shared),
            pager,
        })
    }
}

/// A [`TabBarView`](crate::TabBarView)'s claim on its controller's position:
/// the pager it scrolls. Dropping it returns the position to the owned source
/// at the committed index.
pub(crate) struct PagerAttachment {
    shared: Weak<Shared>,
    pager: PageController,
}

impl PagerAttachment {
    pub(crate) fn pager(&self) -> &PageController {
        &self.pager
    }

    /// A handle for the pages to mark the view as leaving (or back).
    pub(crate) fn suspender(&self) -> PagerSuspender {
        PagerSuspender {
            shared: self.shared.clone(),
        }
    }
}

/// Marks a pager's owner as leaving, so the activity that ends with it is not
/// taken for the user settling a page. Unmounting runs deepest first, so the
/// scrollable stops its activity before the view that owns the attachment is
/// told anything; the pages, which sit below it, are told first.
#[derive(Clone)]
pub(crate) struct PagerSuspender {
    shared: Weak<Shared>,
}

impl PagerSuspender {
    pub(crate) fn set(&self, suspended: bool) {
        if let Some(shared) = self.shared.upgrade() {
            shared.suspended.set(suspended);
        }
    }
}

impl Drop for PagerAttachment {
    fn drop(&mut self) {
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        let link = {
            let mut slot = shared.pager.borrow_mut();
            if slot.as_ref().is_some_and(|link| link.is(&self.pager)) {
                slot.take()
            } else {
                None
            }
        };
        if let Some(link) = link {
            link.unsubscribe();
            shared.drive_owner(shared.index() as f64, Change::Jump);
        }
    }
}

impl std::fmt::Debug for PagerAttachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PagerAttachment")
            .field("pager", &self.pager)
            .finish_non_exhaustive()
    }
}

/// A [`TabController`]'s continuous position: read it with
/// [`value`](Self::value), observe it through [`Listenable`].
///
/// ```
/// use flui_material::TabController;
///
/// let controller = TabController::new(3, 1);
/// assert_eq!(controller.position().value(), 1.0);
/// ```
#[derive(Clone)]
pub struct TabPosition {
    shared: Rc<Shared>,
}

impl TabPosition {
    /// The current position, in tabs: `1.5` is half way between the second
    /// and third tab.
    #[must_use]
    pub fn value(&self) -> f64 {
        self.shared.position()
    }

    /// This position as one shared listenable; every call returns the same
    /// instance, so a repaint subscription can be compared and removed.
    #[must_use]
    pub fn listenable(&self) -> Rc<dyn Listenable> {
        Rc::clone(&self.shared) as Rc<dyn Listenable>
    }

    /// Whether both positions belong to the same controller.
    #[must_use]
    pub fn same_controller(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.shared, &other.shared)
    }
}

impl Listenable for TabPosition {
    fn add_observer(&self, observer: ListenerObserver) -> ListenerId {
        self.shared.add_observer(observer)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.shared.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.shared.remove_all_listeners();
    }
}

impl std::fmt::Debug for TabPosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("TabPosition").field(&self.value()).finish()
    }
}

/// Identity equality — two clones of the same controller are equal; two
/// independently-constructed controllers are not, even with identical
/// `(index, previous_index, length)`. The scope's `update_should_notify`
/// relies on this.
impl PartialEq for TabController {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.shared, &other.shared)
    }
}

impl Eq for TabController {}

impl std::fmt::Debug for TabController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pair = self.shared.pair.get();
        f.debug_struct("TabController")
            .field("index", &pair.index)
            .field("previous_index", &pair.previous_index)
            .field("length", &self.shared.length)
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
        // Only controller identity is compared: position changes reach their
        // readers through `TabController::position`, not this scope.
        self.controller != old.controller
    }
}

impl_inherited_view!(TabControllerScope);

/// Shares one [`TabController`] with a `TabBar`/`TabBarView` pair that don't
/// have a convenient stateful ancestor to own it directly.
///
/// The controller's motion is configured through [`MotionBuilder`]
/// (`duration`, `curve`, `spring`, `motion`); its position runs on the
/// ambient `VsyncScope` clock.
///
/// # Length-change re-creation
///
/// Rebuilding with a different `length` does not mutate the existing
/// controller's `length` field in place (`TabController::length` is
/// immutable per instance — see that type's docs). Instead this creates a
/// **new** `TabController` identity:
///
/// - If the old `index` is still in range for the new `length`, it carries
///   over unchanged, and `previous_index` carries over unchanged too.
/// - If the old `index` is now out of range (the tab list shrank past it),
///   the new controller clamps to `length - 1` and records the *old* index
///   as its `previous_index`.
///
/// Because [`TabController`]'s equality is identity-based, this re-creation
/// is what makes the scope notify: dependents (a `TabBar` re-reading
/// [`DefaultTabController::maybe_of`] every `build`) see a *different*
/// controller and re-resolve their subscription onto it.
///
/// ```
/// use flui_material::DefaultTabController;
/// use flui_sdk::widgets::{MotionBuilder, SizedBox};
/// use std::time::Duration;
///
/// let _root = DefaultTabController::new(3, SizedBox::shrink())
///     .duration(Duration::from_millis(200));
/// ```
#[derive(Clone, StatefulView)]
pub struct DefaultTabController {
    length: usize,
    initial_index: usize,
    motion: MotionSpec,
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
            motion: default_motion(),
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

impl MotionBuilder for DefaultTabController {
    fn motion_mut(&mut self) -> &mut MotionSpec {
        &mut self.motion
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
    vsync: Option<Vsync>,
    /// Registered under the owned controller; replaced with it.
    clock: Option<ClockLease>,
}

impl std::fmt::Debug for DefaultTabControllerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DefaultTabControllerState")
            .field("controller", &self.controller)
            .finish_non_exhaustive()
    }
}

impl StatefulView for DefaultTabController {
    type State = DefaultTabControllerState;

    fn create_state(&self) -> Self::State {
        DefaultTabControllerState {
            controller: TabController::new(self.length, self.initial_index)
                .with_motion(self.motion.clone()),
            vsync: None,
            clock: None,
        }
    }
}

impl ViewState<DefaultTabController> for DefaultTabControllerState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.vsync = VsyncScope::maybe_of(ctx);
        self.clock = Some(self.controller.lease_clock(self.vsync.clone()));
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.vsync = VsyncScope::maybe_of(ctx);
        if let Some(clock) = &self.clock {
            clock.set(self.vsync.clone());
        }
    }

    fn did_update_view(
        &mut self,
        old_view: &DefaultTabController,
        new_view: &DefaultTabController,
    ) {
        if new_view.length != old_view.length {
            self.clock = None;
            self.controller = recreate_for_length_change(&self.controller, new_view.length);
            self.clock = Some(self.controller.lease_clock(self.vsync.clone()));
        }
        self.controller.set_motion(new_view.motion.clone());
    }

    fn build(&self, view: &DefaultTabController, _ctx: &dyn BuildContext) -> impl IntoView {
        TabControllerScope {
            controller: self.controller.clone(),
            child: view.child.clone(),
        }
    }
}

/// The re-creation rule for a `length` change. See [`DefaultTabController`]'s
/// docs for the full contract.
fn recreate_for_length_change(old: &TabController, new_length: usize) -> TabController {
    let old_index = old.index();
    let motion = old.shared.motion.borrow().clone();
    if old_index >= new_length {
        let clamped = new_length.saturating_sub(1);
        TabController::with_previous(new_length, clamped, old_index, motion)
    } else {
        TabController::with_previous(new_length, old_index, old.previous_index(), motion)
    }
}
