//! [`ModalRoute`] — a [`TransitionRoute`] that covers the screen with a barrier
//! and a page.
//!
//! ADR-0020 U5.3. **Private.** No `PageRoute`, no `PopupRoute`, no public API.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/routes.dart:1730-2360`
//! (`ModalRoute`), master `3.33.0-0.0.pre-6280-g88e87cd963f`.
//!
//! Three things arrive with this layer, and only these three are claimed:
//!
//! 1. **`maintainState`** (`routes.dart:1893`, written onto the scope entry at
//!    `:2230`). Now real, because `Overlay` honours it (U5.3 Part A). A covered
//!    modal with `maintain_state == false` is *unmounted*; its subtree state is
//!    destroyed and rebuilt fresh when it is uncovered.
//! 2. **`offstage`** (`:1949-1962`). The page keeps its real geometry but is not
//!    painted, hit-tested or announced — [`Offstage`] over the U5.0-fixed
//!    `RenderOffstage`.
//! 3. **`changedInternalState`** (`:2221-2231`), which rebuilds *this route's*
//!    overlay entry and republishes `maintainState`. It does **not** rebuild the
//!    navigator.
//!
//! # One overlay entry, not two
//!
//! Flutter's `createOverlayEntries` returns `[_modalBarrier, _modalScope]`
//! (`:2350-2356`). FLUI's navigator keys **one** entry per route (ADR-0019 U3,
//! `overlay_route.rs`), so this route builds a `Stack[barrier, page]` into a
//! single entry instead. The three properties the overlay reads survive the merge:
//!
//! | Flutter | Merged |
//! |---|---|
//! | `_modalBarrier.opaque = opaque` on transition complete | the one entry's `opaque` |
//! | `_modalScope.maintainState = maintainState` | the one entry's `maintain_state` |
//! | `_modalBarrier.markNeedsBuild()` | the one entry's `mark_needs_build()` |
//!
//! The barrier sits below the page either way, so paint and hit-test order are
//! unchanged. Two costs, both recorded: a `markNeedsBuild` for the barrier alone
//! rebuilds the page too, and a covered `maintainState` route keeps its barrier
//! subtree mounted where Flutter drops it (the barrier is stateless).
//!
//! # Divergences — none of this is parity
//!
//! * **No `FocusScope`.** FLUI has no `FocusScopeNode`, so `_modalScope`'s focus
//!   trapping, `requestFocus`, and `traversalEdgeBehavior` are absent.
//! * **No `BlockSemantics`, no barrier semantics.** No `semanticsDismissible`, no
//!   `barrierLabel`, no `Semantics(sortKey: OrdinalSortKey(1.0))`. A covered
//!   route's semantics are still announced. The barrier absorbs *pointers* only.
//! * **No `AnimatedModalBarrier`.** `barrier_color` is a flat colour, not driven
//!   through `barrierCurve` by the route's animation.
//! * **No `IgnorePointer(ignoring: !animation.isForwardOrCompleted)`**
//!   (`routes.dart:2278-2283`): the barrier absorbs pointers for the whole life of
//!   the route, including while it pops.
//! * **`offstage` does not swap the animations** to `kAlwaysComplete` /
//!   `kAlwaysDismissed` (`:1958-1962`). That exists to let `HeroController` read
//!   final positions; Hero is out of scope.
//! * **No `filter` / `BackdropFilter`, no `PopScope`, no `LocalHistoryRoute`, no
//!   `_modalScopeCache`.**

// `ModalRoute` is private; `PageRoute` / `PopupRoute` (U5.4) are its production
// consumers and do not surface every knob. `ModalHandle::set_offstage` in
// particular has no public caller until `Hero` drives it (B1.4).

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use std::sync::OnceLock;

use flui_foundation::{ChangeNotifier, Listenable, ListenerId};
use flui_types::Color;
use flui_view::prelude::*;
use flui_view::{AnimatedView, BoxedView, ViewExt, impl_animated_view};
use parking_lot::Mutex;

use super::binding::{RouteBindingSlot, TransitionGroup};
use super::navigator::NavigatorHandle;
use super::overlay_route::{
    NavigatorRoute, RouteAnimation, RouteContentBuilder, RoutePageBuilder, RouteTransitionsBuilder,
};
use super::route::{PushCompletion, Route, RouteId, RouteSettings};
use super::subtree::{RouteSubtreeAnchor, RouteSubtreeCell};
use super::transition_route::{TransitionHandle, TransitionRoute};
use crate::{AbsorbPointer, ColoredBox, GestureDetector, Offstage, SizedBox, Stack, StackFit};

/// `_defaultTransitionsBuilder` (`pages.dart:68-75`): a jump cut.
pub(crate) fn default_transitions_builder() -> RouteTransitionsBuilder {
    Arc::new(|_ctx, _animation, _secondary, child| child)
}

/// The mutable half a `ModalRoute` shares with its content builder and its
/// binding. The builder is an `Arc<dyn Fn>` installed in the overlay entry and
/// outlives every borrow of the route, so nothing it reads can live on `self`.
struct ModalInner {
    /// `ModalRoute.offstage` (`routes.dart:1949`).
    offstage: AtomicBool,
    /// `ModalRoute.maintainState` (`:1893`).
    maintain_state: AtomicBool,
    /// `barrierDismissible` (`:1804`): a tap on the barrier pops the route.
    ///
    /// `final` in Flutter. A cell here only because `ModalInner` is `Arc`-shared
    /// with the content builder from the moment the route is constructed, so a
    /// `.barrier_dismissible(true)` builder cannot reach it through `&mut`.
    barrier_dismissible: AtomicBool,
    /// `barrierColor` (`:1774`). `None` means an invisible barrier that still
    /// absorbs pointers — Flutter's `ModalBarrier` with no colour.
    barrier_color: Mutex<Option<Color>>,

    /// `buildPage` (`routes.dart:1455`).
    page: RoutePageBuilder,
    /// `buildTransitions` (`:1591`), defaulting to a jump cut. A cell because the
    /// content closure captures `inner` at construction, before a
    /// `.transitions(…)` builder can run.
    transitions: Mutex<RouteTransitionsBuilder>,
    /// Set once, immediately after the `TransitionRoute` is constructed. The
    /// content builder is `Arc`-captured *before* that route exists, so the two
    /// cannot be wired the other way round.
    ///
    /// It carries both animations the page and transitions builders read, and the
    /// [`RouteBindingSlot`] `changed_internal_state` writes through.
    transition: OnceLock<TransitionHandle>,

    /// One notifier the `_ModalScope` subscribes to, fed by *both* animations.
    ///
    /// Flutter uses `Listenable.merge([animation, secondaryAnimation])`
    /// (`routes.dart:1101`); `flui_foundation::Listenable` has no `merge`. A relay
    /// is the equivalent, and it has the property `AnimatedView` needs: the same
    /// object every time `listenable()` is called, even though the `ModalScope`
    /// view is rebuilt on every overlay-entry build.
    relay: Arc<ChangeNotifier>,
    /// The relay's subscriptions to the two animations, opened in `install` and
    /// closed in `dispose`. `Listenable` has no `Drop`-based unsubscribe.
    relay_subscriptions: Mutex<Vec<(RouteAnimation, ListenerId)>>,

    /// `ModalRoute._subtreeKey` (`routes.dart:2268`) — owned from construction,
    /// filled while the page is mounted. ADR-0021 U2, seam 4.
    subtree: RouteSubtreeCell,
}

impl ModalInner {
    /// `_buildModalBarrier` + `buildModalBarrier` (`routes.dart:2273-2330`),
    /// reduced to the primitives FLUI has.
    ///
    /// `!offstage` gates the barrier, exactly as `buildModalBarrier` does
    /// (`:2301`) — an offstage route must not eat pointers.
    ///
    /// The [`AbsorbPointer`] is what makes the barrier a barrier: it is hit within
    /// its own bounds whether or not it has a child, so a *colourless* barrier
    /// still stops the pointer reaching the routes beneath. Giving it a
    /// `ColoredBox` child instead would have blocked pointers too — that box is
    /// itself hit-testable — but only when `barrier_color` is set, which is not the
    /// contract. (Found by red-check: with `absorbing(false)` and a colour, every
    /// test stayed green.)
    fn build_barrier(&self, ctx: &dyn BuildContext) -> BoxedView {
        if self.offstage.load(Ordering::Relaxed) {
            return AbsorbPointer::new().absorbing(false).boxed();
        }

        let mut barrier = AbsorbPointer::new().absorbing(true);
        if let Some(color) = *self.barrier_color.lock() {
            barrier = barrier.child(ColoredBox::new(color));
        }

        if !self.barrier_dismissible.load(Ordering::Relaxed) {
            return barrier.boxed();
        }

        // `ModalBarrier`'s `onDismiss ?? () => Navigator.maybePop(context)`
        // (`modal_barrier.dart`). The handle is cloned out from under the tree
        // borrow here and popped later, from the gesture callback.
        let navigator = NavigatorHandle::maybe_of(ctx);
        GestureDetector::new()
            .on_tap(move || {
                if let Some(navigator) = &navigator {
                    navigator.maybe_pop();
                }
            })
            .child(barrier)
            .boxed()
    }

    /// `_buildModalScope` (`routes.dart:2333-2345`), minus `Semantics`,
    /// `PrimaryScrollController` and `FocusScope`.
    ///
    /// The `Offstage` wraps the whole scope, as Flutter's does — so an offstage
    /// route's transitions still run, its page still lays out at real size, and
    /// nothing of it paints.
    fn build_scope(self: &Arc<Self>) -> BoxedView {
        let scope = match self.transition.get() {
            Some(transition) => ModalScope {
                page: Arc::clone(&self.page),
                transitions: Arc::clone(&self.transitions.lock()),
                transition: transition.clone(),
                relay: Arc::clone(&self.relay),
                subtree: self.subtree.clone(),
            }
            .boxed(),
            // Unreachable in a pushed route: `install()` seeds the `OnceLock`
            // before the overlay ever builds this entry.
            None => SizedBox::shrink().boxed(),
        };

        Offstage::new()
            .offstage(self.offstage.load(Ordering::Relaxed))
            .child(scope)
            .boxed()
    }

    /// Point the relay at both animations. Called from `install()`, once the
    /// controller exists.
    fn open_relay(self: &Arc<Self>, transition: &TransitionHandle) {
        let animations: [RouteAnimation; 2] = [
            transition.primary_animation(),
            transition.secondary_animation(),
        ];
        let mut subscriptions = self.relay_subscriptions.lock();
        for animation in animations {
            let relay = Arc::clone(&self.relay);
            let id = animation.add_listener(Arc::new(move || relay.notify_listeners()));
            subscriptions.push((animation, id));
        }
    }

    /// Drop them again. Called from `dispose()`, **before** the controller is.
    fn close_relay(&self) {
        for (animation, id) in self.relay_subscriptions.lock().drain(..) {
            animation.remove_listener(id);
        }
    }
}

// ============================================================================
// _ModalScope — the animation-driven half of the entry
// ============================================================================

/// Flutter's `_ModalScope` (`routes.dart:1055-1250`), reduced to the one job FLUI
/// can do today: rebuild the page and its transitions when either animation ticks.
///
/// Flutter caches the page in `_page ??= …` so only the transitions rebuild per
/// frame (`routes.dart:1229-1240`). FLUI's `BoxedView` is not cloneable, so the
/// page builder re-runs on every tick. Element reconciliation preserves the page's
/// `ViewState`, so this is a **cost**, not a state difference; recorded, not
/// claimed as parity.
///
/// An [`AnimatedView`], which is `AnimatedWidget` — the framework subscribes to
/// [`listenable`](AnimatedView::listenable) on mount and unsubscribes on unmount.
/// `AnimatedBuilder` could not be used: its builder takes no `BuildContext`, and
/// `buildPage` needs one.
#[derive(Clone)]
struct ModalScope {
    page: RoutePageBuilder,
    transitions: RouteTransitionsBuilder,
    transition: TransitionHandle,
    relay: Arc<ChangeNotifier>,
    subtree: RouteSubtreeCell,
}

impl_animated_view!(ModalScope);

impl AnimatedView for ModalScope {
    fn listenable(&self) -> Arc<dyn Listenable> {
        Arc::clone(&self.relay) as Arc<dyn Listenable>
    }
}

impl StatefulView for ModalScope {
    type State = ModalScopeState;

    fn create_state(&self) -> Self::State {
        ModalScopeState
    }
}

/// Stateless beyond the subscription `AnimatedView` manages.
pub(crate) struct ModalScopeState;

impl ViewState<ModalScope> for ModalScopeState {
    /// `buildTransitions(context, animation, secondaryAnimation, buildPage(…))`
    /// (`routes.dart:1229-1240`, `:1656`).
    ///
    /// The [`RouteSubtreeAnchor`] wraps **only** the page, inside the transitions —
    /// exactly where Flutter hangs `_subtreeKey`, on the `RepaintBoundary` around
    /// `buildPage` and nothing else (`routes.dart:1229-1231`). Anchoring outside
    /// the transitions would give `HeroController` the transition's coordinate
    /// space (mid-slide, mid-scale) instead of the page's.
    fn build(&self, view: &ModalScope, ctx: &dyn BuildContext) -> impl IntoView {
        let primary = view.transition.primary_animation();
        let secondary: RouteAnimation = view.transition.secondary_animation();
        let page = (view.page)(ctx, &primary, &secondary);
        let anchored = RouteSubtreeAnchor::new(view.subtree.clone(), page).boxed();
        (view.transitions)(ctx, &primary, &secondary, anchored)
    }
}

/// A route that covers the routes below it with a barrier and a page.
///
/// Private: `modal_route_is_not_exported` keeps it that way until U5.4's parity +
/// sign-off gate.
pub(crate) struct ModalRoute<T> {
    transition: TransitionRoute<T>,
    inner: Arc<ModalInner>,
}

impl<T: Send + Sync + Clone + 'static> ModalRoute<T> {
    /// A modal showing `page`, entering and leaving over `duration`, with a
    /// jump-cut transition.
    ///
    /// Defaults match Flutter's `ModalRoute`: `maintain_state = true`,
    /// `offstage = false`, no barrier colour, not dismissible, not opaque.
    pub(crate) fn new(duration: Duration, page: RoutePageBuilder) -> Self {
        let inner = Arc::new(ModalInner {
            offstage: AtomicBool::new(false),
            maintain_state: AtomicBool::new(true),
            barrier_dismissible: AtomicBool::new(false),
            barrier_color: Mutex::new(None),
            page,
            transitions: Mutex::new(default_transitions_builder()),
            transition: OnceLock::new(),
            relay: Arc::new(ChangeNotifier::new()),
            relay_subscriptions: Mutex::new(Vec::new()),
            subtree: RouteSubtreeCell::new(),
        });

        let content = {
            let inner = Arc::clone(&inner);
            move |ctx: &dyn BuildContext| -> BoxedView {
                // Barrier first: it paints below the page and is hit-tested after
                // it, matching `[_modalBarrier, _modalScope]` entry order.
                let children = vec![inner.build_barrier(ctx), inner.build_scope()];
                Stack::new(children).fit(StackFit::Expand).boxed()
            }
        };

        let transition = TransitionRoute::new(duration, content);
        // The content closure captured `inner` before the route existed, so the
        // handle can only be wired in afterwards. `OnceLock` makes that a fact of
        // the type rather than a comment.
        let _ = inner.transition.set(transition.handle());

        Self { transition, inner }
    }

    /// The builders below mutate `inner` *before* the route is pushed, so no
    /// `changed_internal_state` is needed — the entry does not exist yet.
    pub(crate) fn named(mut self, name: impl Into<String>) -> Self {
        self.transition = self.transition.named(name);
        self
    }

    /// `TransitionRoute.opaque`. `PageRoute` sets this; `PopupRoute` does not.
    pub(crate) fn opaque(mut self, opaque: bool) -> Self {
        self.transition = self.transition.opaque(opaque);
        self
    }

    /// `buildTransitions` (`routes.dart:1591`).
    pub(crate) fn transitions(self, transitions: RouteTransitionsBuilder) -> Self {
        *self.inner.transitions.lock() = transitions;
        self
    }

    /// `transitionDuration` (`routes.dart:140-147`).
    pub(crate) fn duration(mut self, duration: Duration) -> Self {
        self.transition = self.transition.duration(duration);
        self
    }

    /// `reverseTransitionDuration` (`routes.dart:148`).
    pub(crate) fn reverse_duration(mut self, duration: Duration) -> Self {
        self.transition = self.transition.reverse_duration(duration);
        self
    }

    /// The transition family — see [`TransitionGroup`].
    pub(crate) fn group(mut self, group: TransitionGroup) -> Self {
        self.transition = self.transition.group(group);
        self
    }

    /// The `result ?? currentResult` fallback (`navigator.dart:426`).
    pub(crate) fn with_current_result(mut self, result: T) -> Self {
        self.transition = self.transition.with_current_result(result);
        self
    }

    /// `ModalRoute.maintainState` (`routes.dart:1893`).
    pub(crate) fn maintain_state(self, maintain_state: bool) -> Self {
        self.inner
            .maintain_state
            .store(maintain_state, Ordering::Relaxed);
        self
    }

    /// `barrierDismissible` (`routes.dart:1804`).
    pub(crate) fn barrier_dismissible(self, dismissible: bool) -> Self {
        self.inner
            .barrier_dismissible
            .store(dismissible, Ordering::Relaxed);
        self
    }

    /// `barrierColor` (`routes.dart:1774`).
    pub(crate) fn barrier_color(self, color: Color) -> Self {
        *self.inner.barrier_color.lock() = Some(color);
        self
    }

    /// A cloneable view of this route's modal state, obtainable **before** the
    /// route is moved into `NavigatorHandle::push`.
    ///
    /// The `offstage` half of [`ModalHandle`] is the seam `Hero` will drive
    /// (B1.4); no production caller exists yet, so the whole handle is reachable
    /// only from tests. It is kept — not deleted — because U5.3 verified it
    /// against `routes.dart:1949-1962` and B1.4 names it as a precondition.
    #[cfg(test)]
    pub(crate) fn handle(&self) -> ModalHandle {
        ModalHandle {
            inner: Arc::clone(&self.inner),
        }
    }

    /// The transition handle, for driving the animation by hand.
    #[cfg(test)]
    pub(crate) fn transition_handle(&self) -> super::transition_route::TransitionHandle {
        self.transition.handle()
    }
}

impl<T> fmt::Debug for ModalRoute<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModalRoute")
            .field("offstage", &self.inner.offstage.load(Ordering::Relaxed))
            .field(
                "maintain_state",
                &self.inner.maintain_state.load(Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

/// An owned, `'static` capability to drive a pushed [`ModalRoute`]'s internal
/// state — the ADR-0019 §3.2 pattern, again: the route itself lives behind
/// `Box<dyn ErasedRoute>` inside the history's mutex and cannot be reached.
///
/// Reachable only from tests until `Hero` (B1.4) drives `offstage`.
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct ModalHandle {
    inner: Arc<ModalInner>,
}

#[cfg(test)]
impl ModalHandle {
    /// `ModalRoute.offstage = value` (`routes.dart:1951-1962`), minus the
    /// animation-proxy swap.
    pub(crate) fn set_offstage(&self, offstage: bool) {
        if self.inner.offstage.swap(offstage, Ordering::Relaxed) == offstage {
            return; // `if (_offstage == value) return;`
        }
        changed_internal_state(&self.inner);
    }

    pub(crate) fn offstage(&self) -> bool {
        self.inner.offstage.load(Ordering::Relaxed)
    }

    /// There is no `maintainState` setter in Flutter — it is an abstract getter a
    /// subclass overrides, and `changedInternalState` republishes it. This is the
    /// same thing with a cell behind it, which is what lets a test observe the
    /// republish.
    pub(crate) fn set_maintain_state(&self, maintain_state: bool) {
        if self
            .inner
            .maintain_state
            .swap(maintain_state, Ordering::Relaxed)
            == maintain_state
        {
            return;
        }
        changed_internal_state(&self.inner);
    }
}

/// `ModalRoute.changedInternalState` (`routes.dart:2221-2231`).
///
/// Rebuilds this route's overlay entry and republishes `maintainState`. Flutter's
/// `schedulerPhase != persistentCallbacks` guard has no analogue: FLUI's
/// `mark_needs_build` only inserts an id into an inbox the next `build_scope`
/// drains, so it is already safe from any phase (`entry.rs` module docs).
#[cfg(test)]
fn changed_internal_state(inner: &ModalInner) {
    let Some(binding) = inner.transition.get().and_then(TransitionHandle::binding) else {
        return;
    };
    binding.set_entry_maintain_state(inner.maintain_state.load(Ordering::Relaxed));
    binding.mark_entry_needs_build();
}

// ============================================================================
// Route delegation
// ============================================================================

impl<T: Send + Sync + Clone + 'static> ModalRoute<T> {
    /// This route's navigator capability, or `None` before it is pushed.
    fn binding(&self) -> Option<super::binding::RouteBinding> {
        self.inner
            .transition
            .get()
            .and_then(TransitionHandle::binding)
    }
}

impl<T: Send + Sync + Clone + 'static> Route for ModalRoute<T> {
    type Output = T;

    fn settings(&self) -> &RouteSettings {
        self.transition.settings()
    }

    fn current_result(&mut self) -> Option<T> {
        self.transition.current_result()
    }

    fn finished_when_popped(&self) -> bool {
        self.transition.finished_when_popped()
    }

    fn will_handle_pop_internally(&self) -> bool {
        self.transition.will_handle_pop_internally()
    }

    /// `OverlayRoute.install` creates the entries, then `TransitionRoute.install`
    /// builds the controller (`routes.dart:69-71`, `:323-334`). FLUI's entry is
    /// created by `push_bound` just before the flush, so the only thing left here
    /// is publishing `maintainState` onto it — Flutter does that at
    /// `createOverlayEntries` (`:2353-2355`).
    fn install(&mut self) {
        self.transition.install();
        self.inner
            .open_relay(self.inner.transition.get().expect("BUG: set in `new`"));
        if let Some(binding) = self.binding() {
            binding.set_entry_maintain_state(self.inner.maintain_state.load(Ordering::Relaxed));
            // Registered before the page has ever been built, so the registry
            // knows the route exists; it resolves to `None` until the page mounts.
            binding.publish_subtree(self.inner.subtree.clone());
        }
    }

    fn did_push(&mut self) -> PushCompletion {
        self.transition.did_push()
    }

    fn did_add(&mut self) {
        self.transition.did_add();
    }

    fn did_replace(&mut self, previous: Option<RouteId>) {
        self.transition.did_replace(previous);
    }

    fn did_pop(&mut self) -> bool {
        self.transition.did_pop()
    }

    fn did_complete(&mut self, result: Option<&T>) {
        self.transition.did_complete(result);
    }

    fn did_pop_next(&mut self, popped: RouteId) {
        self.transition.did_pop_next(popped);
    }

    fn did_change_next(&mut self, next: Option<RouteId>) {
        self.transition.did_change_next(next);
    }

    fn did_change_previous(&mut self, previous: Option<RouteId>) {
        self.transition.did_change_previous(previous);
    }

    fn on_pop_invoked(&mut self, did_pop: bool) {
        self.transition.on_pop_invoked(did_pop);
    }

    /// Close the relay **before** `TransitionRoute::dispose` drops the controller:
    /// a live listener on a disposed controller is a use-after-free of the
    /// notifier list.
    ///
    /// The subtree registration goes with it. The page's own `dispose`/`detach`
    /// will empty the cell when the overlay entry is removed, but the *entry* must
    /// go now: a disposed route that a `HeroController` can still name is a route
    /// it can still measure.
    fn dispose(&mut self) {
        if let Some(binding) = self.binding() {
            binding.withdraw_subtree();
        }
        self.inner.close_relay();
        self.transition.dispose();
    }
}

impl<T: Send + Sync + Clone + 'static> NavigatorRoute for ModalRoute<T> {
    fn content_builder(&self) -> RouteContentBuilder {
        self.transition.content_builder()
    }

    fn binding_slot(&self) -> Option<&RouteBindingSlot> {
        self.transition.binding_slot()
    }
}
