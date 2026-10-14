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
    AnimatedValue, AnimatedValueView, Animation as _, ArcCurve, MotionSpec, Vsync, curves,
};
use flui_sdk::foundation::panic::PanicRecovery;
use flui_sdk::foundation::{ChangeNotifier, Listenable, ListenerId, ListenerObserver};
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, InheritedView, impl_inherited_view};
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::{MotionBuilder, PageController};

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
        let mut recovery = PanicRecovery::new();
        for listener in snapshot {
            recovery.run(|| listener());
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
    /// Notifies whenever the position may have moved, from either source.
    position_listeners: ChangeNotifier,
}

/// The default tab-change motion: 300 ms ease-in-out.
fn default_motion() -> MotionSpec {
    MotionSpec::Curve {
        duration: TabController::DEFAULT_DURATION,
        curve: ArcCurve::new(curves::EASE_IN_OUT),
    }
}

impl Shared {
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
        let _driving = DrivingPager::enter(&self.driving_pager);
        let (duration, curve) = match change {
            // A zero-duration run jumps through the scroll controller, which
            // stops whatever run is driving the pager; `jump_to_page` only
            // writes the offset, and a run in flight would carry on past it.
            Change::Jump => (Duration::ZERO, ArcCurve::new(curves::LINEAR)),
            Change::Animate => pager_motion(&self.motion.borrow()),
        };
        pager.animate_to_page(index, duration, curve);
    }

    /// Retargets or snaps the owned source. Re-entry while the owner is out
    /// records the newest request, which the outer call applies before it
    /// puts the owner back — also when an observer of the step that recorded
    /// it panicked, since the nested call already committed its index. The
    /// first such panic is resumed once the requests are drained and the
    /// owner is back.
    fn drive_owner(&self, target: f64, change: Change) {
        let Some(owner) = self.owner.borrow_mut().take() else {
            self.pending_drive.set(Some((target, change)));
            return;
        };
        let mut recovery = PanicRecovery::new();
        {
            let mut slot = OwnerSlot {
                cell: &self.owner,
                owner: Some(owner),
            };
            let mut request = Some((target, change));
            while let Some((target, change)) = request.take() {
                let motion = self.motion.borrow().clone();
                let owner = slot
                    .owner
                    .as_mut()
                    .expect("BUG: the owner stays in its slot while it is driven");
                recovery.run(|| {
                    let result = match change {
                        Change::Jump => owner.snap_to(target),
                        Change::Animate => owner.retarget(target, motion).map(drop),
                    };
                    if let Err(error) = result {
                        tracing::warn!(%error, "TabController: position motion refused");
                    }
                });
                request = self.pending_drive.take();
            }
        }
        recovery.finish();
    }

    fn rebind(&self, vsync: Option<&Vsync>) {
        let Some(owner) = self.owner.borrow_mut().take() else {
            return;
        };
        let mut slot = OwnerSlot {
            cell: &self.owner,
            owner: Some(owner),
        };
        let owner = slot
            .owner
            .as_mut()
            .expect("BUG: the owner stays in its slot while it is rebound");
        if let Err(error) = owner.rebind(vsync) {
            tracing::error!(%error, "TabController: position has no clock");
        }
    }

    /// The pager stopped scrolling: commit the page it rests on, unless this
    /// controller is moving the pager and so caused the stop.
    fn commit_settled_page(&self) {
        if self.driving_pager.get() {
            return;
        }
        let Some(pager) = self.pager() else {
            return;
        };
        if pager.position().is_scrolling() {
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
        if self.commit(settled) {
            self.index_listeners.notify();
        }
    }
}

/// The pager's duration and curve for `motion`. A pager animates over a
/// duration and curve only, so a spring runs the default motion.
fn pager_motion(motion: &MotionSpec) -> (Duration, ArcCurve) {
    match motion {
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
                owner: RefCell::new(Some(owner)),
                owned,
                pending_drive: Cell::new(None),
                motion: RefCell::new(motion),
                pager: RefCell::new(None),
                driving_pager: Cell::new(false),
                position_listeners,
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
        recovery.run(|| self.shared.index_listeners.notify());
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

    /// Drives the owned position source on `vsync`, the ambient clock of the
    /// widget that binds it.
    pub(crate) fn bind(&self, vsync: Option<&Vsync>) {
        self.shared.rebind(vsync);
    }

    /// Makes a new pager at the current index the position source until the
    /// returned attachment drops, or returns `None` while another attachment
    /// holds it: the position has one source, so a controller drives at most
    /// one [`TabBarView`](crate::TabBarView) at a time.
    pub(crate) fn attach_pager(&self) -> Option<PagerAttachment> {
        if self.shared.pager.borrow().is_some() {
            return None;
        }
        let pager = PageController::with_params(self.index(), 1.0);
        let pixels = pager.listenable();
        let forward = self.shared.position_listeners.clone();
        let pixel_listener = pixels.add_observer(Rc::new(move |recovery| {
            forward.notify_listeners_with_recovery(recovery);
        }));
        let weak = Rc::downgrade(&self.shared);
        let activity_listener = pager.position().add_activity_listener(Rc::new(move || {
            if let Some(shared) = weak.upgrade() {
                shared.commit_settled_page();
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
        }
    }
}

impl ViewState<DefaultTabController> for DefaultTabControllerState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.vsync = VsyncScope::maybe_of(ctx);
        self.controller.bind(self.vsync.as_ref());
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.vsync = VsyncScope::maybe_of(ctx);
        self.controller.bind(self.vsync.as_ref());
    }

    fn did_update_view(
        &mut self,
        old_view: &DefaultTabController,
        new_view: &DefaultTabController,
    ) {
        if new_view.length != old_view.length {
            self.controller = recreate_for_length_change(&self.controller, new_view.length);
            self.controller.bind(self.vsync.as_ref());
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
