//! [`ModalRoute`] — a [`TransitionRoute`] that covers the screen with a barrier
//! and a page.
//!
//! Ported under ADR-0020. **Private:** no `PageRoute`, no `PopupRoute`, no public API.
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
//! ADR-0021 added the fourth: **`offstage` swaps the animation proxies** to
//! `kAlwaysComplete` / `kAlwaysDismissed` (`:1958-1961`), so an offstage route's
//! builders lay it out at its *final* position. That is what lets `HeroController`
//! read a flight's destination one frame early.
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
//! * **Per-route `FocusScope` — landed (ADR-0022).** The page is wrapped in
//!   `FocusScope::with_external_node` (`routes.dart:1201-1202`) and the current
//!   route's scope is the `FocusManager`'s *active scope* — FLUI's analogue of
//!   `setFirstFocus` chaining. Still absent: `traversalEdgeBehavior` (no
//!   node-layer flag) and `requestFocus = false` opt-out.
//! * **No `BlockSemantics`, no barrier semantics.** No `semanticsDismissible`, no
//!   `barrierLabel`, no `Semantics(sortKey: OrdinalSortKey(1.0))`. A covered
//!   route's semantics are still announced. The barrier absorbs *pointers* only.
//! * **No `AnimatedModalBarrier`.** `barrier_color` is a flat colour, not driven
//!   through `barrierCurve` by the route's animation.
//! * **No `IgnorePointer(ignoring: !animation.isForwardOrCompleted)`**
//!   (`routes.dart:2278-2283`): the barrier absorbs pointers for the whole life of
//!   the route, including while it pops.
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

use flui_animation::{Animation, ProxyAnimation};
use flui_foundation::{ChangeNotifier, Listenable, ListenerId};
use flui_types::Color;
use flui_view::prelude::*;
use flui_view::{AnimatedView, BoxedView, ViewExt, impl_animated_view};
use parking_lot::Mutex;

use super::binding::{RouteBindingSlot, TransitionGroup};
use super::hero::{HeroRegistry, HeroScope};
use super::local_history::{LocalHistoryHandle, LocalHistoryRegistry, LocalHistoryScope};
use super::navigator::NavigatorHandle;
use super::overlay_route::{
    NavigatorRoute, RouteAnimation, RouteContentBuilder, RoutePageBuilder, RouteTransitionsBuilder,
};
use super::pop_scope::{PopEntryRegistry, PopEntryScope};
use super::route::{PushCompletion, Route, RouteId, RouteSettings};
use super::subtree::{RouteSubtreeAnchor, RouteSubtreeCell};
use super::transition_route::{
    TransitionHandle, TransitionRoute, always_complete, always_dismissed,
};
use flui_interaction::routing::{FocusManager, FocusScopeNode};

use crate::{
    AbsorbPointer, ColoredBox, FocusScope, GestureDetector, Offstage, SizedBox, Stack, StackFit,
};

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

    /// `ModalRoute._animationProxy` (`routes.dart:1685`, `:1969-1970`).
    ///
    /// **This — not the controller — is what `buildPage` and `buildTransitions`
    /// see.** Its parent is the `TransitionRoute` controller normally, and
    /// `kAlwaysCompleteAnimation` while the route is [`offstage`](Self::offstage)
    /// (`:1958`). That swap is the entire reason an offstage route lays out at its
    /// *final* geometry rather than wherever its entrance transition happens to be:
    /// `HeroController` measures the destination one frame before the flight.
    primary: Arc<ProxyAnimation<f32>>,
    /// `ModalRoute._secondaryAnimationProxy` (`:1686`, `:1973-1974`).
    ///
    /// Parent is the `TransitionRoute` secondary train, or
    /// `kAlwaysDismissedAnimation` while offstage (`:1959-1961`) — an offstage route
    /// must not be pushed aside by whatever sits above it either.
    secondary: Arc<ProxyAnimation<f32>>,

    /// `ModalRoute._subtreeKey` (`routes.dart:2268`) — owned from construction,
    /// filled while the page is mounted. ADR-0021, seam 4.
    subtree: RouteSubtreeCell,

    /// Every `Hero` mounted in this route's page, by tag. Flutter builds the
    /// equivalent map on demand by walking `subtreeContext`'s elements
    /// (`heroes.dart:279-345`); FLUI's heroes register themselves into this one, so
    /// no walk and no downcast is ever needed. ADR-0021
    heroes: HeroRegistry,

    /// Every `PopScope` mounted in this route's page — Flutter's
    /// `ModalRoute._popEntries` (`routes.dart:1980`). Consulted by
    /// [`Route::vetoes_pop`] and notified from [`Route::on_pop_invoked`].
    pop_entries: PopEntryRegistry,

    /// This route's local-history stack — Flutter's `_localHistory`
    /// (`routes.dart:748`). While non-empty, a pop removes the most recent
    /// entry instead of the route (ADR-0025).
    local_history: LocalHistoryRegistry,

    /// `_ModalScopeState.focusScopeNode` (`routes.dart:1095`): the per-route
    /// focus scope. The page is wrapped in a `FocusScope::with_external_node`
    /// over this, and the route lifecycle makes it the manager's **active
    /// scope** while the route is current — ADR-0022
    focus_scope: Arc<FocusScopeNode>,
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

    /// `_buildModalScope` (`routes.dart:2333-2345`), minus `Semantics` and
    /// `PrimaryScrollController`.
    ///
    /// The `Offstage` wraps the whole scope, as Flutter's does — so an offstage
    /// route's transitions still run, its page still lays out at real size, and
    /// nothing of it paints. Inside it, `FocusScope::with_external_node` over
    /// [`focus_scope`](Self::focus_scope) is Flutter's
    /// `FocusScope.withExternalFocusNode` (`routes.dart:1201-1202`): heroes,
    /// text fields and `Focus` widgets in the page attach under the route's own
    /// scope, so traversal stays within the route (ADR-0022).
    fn build_scope(self: &Arc<Self>) -> BoxedView {
        let scope = match self.transition.get() {
            Some(_transition) => ModalScope {
                page: Arc::clone(&self.page),
                transitions: Arc::clone(&self.transitions.lock()),
                primary: Arc::clone(&self.primary),
                secondary: Arc::clone(&self.secondary),
                relay: Arc::clone(&self.relay),
                subtree: self.subtree.clone(),
                heroes: self.heroes.clone(),
                pop_entries: self.pop_entries.clone(),
                local_history: self.local_history_handle(),
            }
            .boxed(),
            // Unreachable in a pushed route: `install()` seeds the `OnceLock`
            // before the overlay ever builds this entry.
            None => SizedBox::shrink().boxed(),
        };

        Offstage::new()
            .offstage(self.offstage.load(Ordering::Relaxed))
            .child(FocusScope::with_external_node(
                Arc::clone(&self.focus_scope),
                scope,
            ))
            .boxed()
    }

    /// Make this route's scope the traversal boundary and move the keyboard
    /// focus into it — FLUI's analogue of
    /// `navigator.focusNode.enclosingScope?.setFirstFocus(focusScopeNode)`
    /// (`routes.dart:1692`, `:1137`). Flutter chains focused children so focus
    /// lands in the current route; FLUI sets the manager's **active scope**
    /// (which confines `focus_next`/`focus_previous` the same way), then
    /// restores the scope's remembered focused child, or unfocuses a field
    /// left focused on a now-covered route so it stops receiving keys.
    fn activate_focus_scope(&self) {
        let manager = FocusManager::global();
        manager.set_active_scope(Some(Arc::clone(&self.focus_scope)));
        match self.focus_scope.focused_child() {
            Some(remembered) => manager.request_focus(remembered),
            None => {
                if manager.primary_focus().is_some()
                    && !self.focus_scope.as_focus_node().has_focus()
                {
                    manager.unfocus();
                }
            }
        }
    }

    /// On dispose: release the active scope **only if it is still ours**. A
    /// popped route's dispose runs when its exit transition ends — after the
    /// revealed route claimed the active scope via `did_change_next(None)` at
    /// pop time — so this fires only for a route torn down while current
    /// (navigator unmount, `remove_route` of the top).
    fn release_focus_scope(&self) {
        let manager = FocusManager::global();
        if Arc::ptr_eq(&manager.active_scope(), &self.focus_scope) {
            manager.set_active_scope(None);
        }
    }

    /// The page-facing local-history capability: the registry plus this
    /// route's `changed_internal_state`, owed on the empty↔non-empty edges
    /// (`routes.dart:886-895`).
    fn local_history_handle(self: &Arc<Self>) -> LocalHistoryHandle {
        let inner = Arc::clone(self);
        LocalHistoryHandle::new(
            self.local_history.clone(),
            Arc::new(move || changed_internal_state(&inner)),
        )
    }

    /// Repoint both proxies at whatever [`offstage`](Self::offstage) currently
    /// implies — Flutter's two lines in the `offstage` setter (`routes.dart:1958-1961`),
    /// hoisted so `install()` can run them too.
    ///
    /// `install()` needs them because a route may be forced offstage before it is
    /// pushed: `ModalHandle` is minted from the *unpushed* route, and
    /// `changed_internal_state` returns early when there is no binding yet. Without
    /// this call the proxies would be seeded from the controller and never swapped.
    fn sync_animation_proxies(&self) {
        let offstage = self.offstage.load(Ordering::Relaxed);
        let transition = self.transition.get();

        self.primary.set_parent(if offstage {
            always_complete()
        } else {
            transition.map_or_else(always_dismissed, TransitionHandle::primary_animation)
        });

        self.secondary.set_parent(if offstage {
            always_dismissed()
        } else {
            transition.map_or_else(always_dismissed, |transition| {
                transition.secondary_animation() as Arc<dyn Animation<f32>>
            })
        });
    }

    /// Point the relay at both **proxies**. Called from `install()`.
    ///
    /// The proxies, not the controller: `ProxyAnimation::set_parent` moves the
    /// listeners with it *and* notifies them, so an offstage swap rebuilds the scope
    /// by itself. That is what carries the completed animation into `buildPage`
    /// within the same frame. (Nothing ticks an offstage route afterwards — its
    /// parent is a constant — which is fine: there is nothing left to animate.)
    fn open_relay(self: &Arc<Self>) {
        let animations: [RouteAnimation; 2] = [
            Arc::clone(&self.primary) as RouteAnimation,
            Arc::clone(&self.secondary) as RouteAnimation,
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
    /// `widget.route.animation` (`routes.dart:1234`) — the **proxy**, so an offstage
    /// route's builders see `kAlwaysCompleteAnimation`.
    primary: Arc<ProxyAnimation<f32>>,
    /// `widget.route.secondaryAnimation` (`:1235`).
    secondary: Arc<ProxyAnimation<f32>>,
    relay: Arc<ChangeNotifier>,
    subtree: RouteSubtreeCell,
    heroes: HeroRegistry,
    /// The route's `PopScope` registry, provided to the page as an ambient.
    pop_entries: PopEntryRegistry,
    /// The route's local-history handle, provided to the page as an ambient
    /// (ADR-0025).
    local_history: LocalHistoryHandle,
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
        let primary: RouteAnimation = Arc::clone(&view.primary) as RouteAnimation;
        let secondary: RouteAnimation = Arc::clone(&view.secondary) as RouteAnimation;
        let page = (view.page)(ctx, &primary, &secondary);
        // The `HeroScope` sits **inside** the subtree anchor, so the anchor stays the
        // route's coordinate root and every hero is a descendant of it — which is what
        // `transform_to(hero, route_subtree)` needs (`heroes.dart:501-509`).
        let anchored = RouteSubtreeAnchor::new(
            view.subtree.clone(),
            HeroScope::new(
                view.heroes.clone(),
                PopEntryScope::new(
                    view.pop_entries.clone(),
                    LocalHistoryScope::new(view.local_history.clone(), page),
                ),
            ),
        )
        .boxed();
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
            // Both rest at `kAlwaysDismissedAnimation` until `install()` points them
            // at the controller — Flutter builds them there too (`routes.dart:1685`),
            // and an unpushed route has no animation to proxy.
            primary: Arc::new(ProxyAnimation::new(always_dismissed())),
            secondary: Arc::new(ProxyAnimation::new(always_dismissed())),
            subtree: RouteSubtreeCell::new(),
            heroes: HeroRegistry::new(),
            pop_entries: PopEntryRegistry::new(),
            local_history: LocalHistoryRegistry::new(),
            focus_scope: FocusScopeNode::with_debug_label("ModalRoute Focus Scope"),
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
    /// Production since ADR-0021: `HeroController` drives `set_offstage` through
    /// the copy this route publishes into the navigator's registry at `install()`.
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
/// This is FLUI's `route.offstage = …` (`routes.dart:1951`). `HeroController` holds
/// one per route, looked up by [`RouteId`] through the navigator's registry.
#[derive(Clone)]
pub(crate) struct ModalHandle {
    inner: Arc<ModalInner>,
}

/// `dead_code` in the lib target: `HeroController` is this handle's only production
/// caller, and it is itself dead until U4's `Hero` widget. See `hero_controller.rs`.
#[allow(dead_code)]
impl ModalHandle {
    /// Deliver `onPopInvokedWithResult(did_pop, …)` to every `PopScope`
    /// registered in this route's page (`routes.dart:2045-2050`). Called by
    /// `NavigatorShared::apply` **outside** the history lock — the callbacks
    /// are user code and may call back into the navigator.
    pub(crate) fn notify_pop_invoked(&self, did_pop: bool) {
        self.inner.pop_entries.notify_pop_invoked(did_pop);
    }

    /// Fire the `on_remove`s owed by local-history pops that happened inside
    /// the flush, and the emptied-edge `changed_internal_state`
    /// (`routes.dart:952-963`). Called by `NavigatorShared::apply` with **no
    /// lock held** (ADR-0025).
    pub(crate) fn drain_local_history(&self) {
        let (callbacks, emptied) = self.inner.local_history.take_owed();
        for callback in callbacks {
            callback();
        }
        if emptied {
            changed_internal_state(&self.inner);
        }
    }

    /// `ModalRoute.offstage = value` (`routes.dart:1951-1962`), whole: the early
    /// return on an unchanged value, the animation-proxy swap, and
    /// `changedInternalState`.
    ///
    /// (ADR-0021 added the proxy swap. Until then this doc read "minus the
    /// animation-proxy swap", which was true when written and a trap afterwards.)
    pub(crate) fn set_offstage(&self, offstage: bool) {
        if self.inner.offstage.swap(offstage, Ordering::Relaxed) == offstage {
            return; // `if (_offstage == value) return;`
        }
        // `_animationProxy!.parent = _offstage ? kAlwaysCompleteAnimation : super.animation;`
        // `_secondaryAnimationProxy!.parent = _offstage ? kAlwaysDismissedAnimation : …`
        // (`routes.dart:1958-1961`) — before `changedInternalState`, so the rebuild it
        // schedules already sees the swapped animations.
        self.inner.sync_animation_proxies();
        changed_internal_state(&self.inner);
    }

    pub(crate) fn offstage(&self) -> bool {
        self.inner.offstage.load(Ordering::Relaxed)
    }

    /// What the route's builders currently see as `route.animation`
    /// (`routes.dart:1969`) — the proxy, so `1.0`/completed while offstage.
    pub(crate) fn primary_animation(&self) -> RouteAnimation {
        Arc::clone(&self.inner.primary) as RouteAnimation
    }

    /// `route.secondaryAnimation` (`:1973`) — `0.0`/dismissed while offstage.
    pub(crate) fn secondary_animation(&self) -> RouteAnimation {
        Arc::clone(&self.inner.secondary) as RouteAnimation
    }

    /// The heroes mounted in this route's page — FLUI's `Hero._allHeroesFor(route)`
    /// (`heroes.dart:279`), as a registry rather than an element walk.
    pub(crate) fn heroes(&self) -> HeroRegistry {
        self.inner.heroes.clone()
    }

    /// There is no `maintainState` setter in Flutter — it is an abstract getter a
    /// subclass overrides, and `changedInternalState` republishes it. This is the
    /// same thing with a cell behind it, which is what lets a test observe the
    /// republish.
    #[cfg(test)]
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
///
/// **What `mark_entry_needs_build` actually rebuilds** is this route's *overlay
/// entry* — `Stack[barrier, Offstage[scope]]` — so a flipped `offstage` reaches the
/// `Offstage` wrapper and the barrier. It is **not** what propagates the animation
/// swap: `ProxyAnimation::set_parent` notifies the relay, and the `ModalScope`
/// rebuilds itself. Delete this call and an offstage route measures correctly while
/// still painting; delete the swap and it paints correctly while measuring wrong.
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

    /// `LocalHistoryRoute.willHandlePopInternally` (`routes.dart:970-972`):
    /// non-empty local history claims the pop.
    fn will_handle_pop_internally(&self) -> bool {
        !self.inner.local_history.is_empty() || self.transition.will_handle_pop_internally()
    }

    /// `OverlayRoute.install` creates the entries, then `TransitionRoute.install`
    /// builds the controller (`routes.dart:69-71`, `:323-334`). FLUI's entry is
    /// created by `push_bound` just before the flush, so the only thing left here
    /// is publishing `maintainState` onto it — Flutter does that at
    /// `createOverlayEntries` (`:2353-2355`).
    fn install(&mut self) {
        self.transition.install();
        // Order: the controller must exist before the proxies can point at it
        // (`routes.dart:1684-1688` — `super.install()` then the two `ProxyAnimation`s),
        // and the relay subscribes to the proxies, so it goes last.
        self.inner.sync_animation_proxies();
        self.inner.open_relay();
        if let Some(binding) = self.binding() {
            binding.set_entry_maintain_state(self.inner.maintain_state.load(Ordering::Relaxed));
            // Registered before the page has ever been built, so the registry
            // knows the route exists; it resolves to `None` until the page mounts.
            binding.publish_subtree(self.inner.subtree.clone());
            // `HeroController` reaches `route.offstage` through this, by id.
            binding.publish_modal(self.handle());
        }
    }

    /// `ModalRoute.didPush` moves the focus into the route's scope
    /// (`routes.dart:1690-1695`); so does `didAdd` (`:1698-1703`).
    fn did_push(&mut self) -> PushCompletion {
        self.inner.activate_focus_scope();
        self.transition.did_push()
    }

    fn did_add(&mut self) {
        self.inner.activate_focus_scope();
        self.transition.did_add();
    }

    fn did_replace(&mut self, previous: Option<RouteId>) {
        self.transition.did_replace(previous);
    }

    /// `LocalHistoryRoute.didPop` (`routes.dart:950-965`): while entries
    /// exist, pop the most recent one and answer `false` — the route stays and
    /// its future stays pending. The entry's `on_remove` (and the emptied-edge
    /// `changed_internal_state`) are **owed**, not fired: this runs under the
    /// history lock, and `NavigatorShared::apply` delivers them outside it
    /// (ADR-0025).
    fn did_pop(&mut self) -> bool {
        if self.inner.local_history.pop_last_deferred() {
            return false;
        }
        self.transition.did_pop()
    }

    fn did_complete(&mut self, result: Option<&T>) {
        self.transition.did_complete(result);
    }

    /// The route above popped: this one is current again, and Flutter
    /// re-focuses it through `changedInternalState` → `_routeSetState`
    /// (`routes.dart:1731-1736`).
    fn did_pop_next(&mut self, popped: RouteId) {
        self.inner.activate_focus_scope();
        self.transition.did_pop_next(popped);
    }

    fn did_change_next(&mut self, next: Option<RouteId>) {
        // Becoming topmost re-activates this route's scope — Flutter's
        // `_routeSetState` re-`setFirstFocus`es whenever `isCurrent` flips
        // (`routes.dart:1731-1736`); a pop announces `did_change_next(None)`
        // to the revealed route.
        if next.is_none() {
            self.inner.activate_focus_scope();
        }
        self.transition.did_change_next(next);
    }

    fn did_change_previous(&mut self, previous: Option<RouteId>) {
        self.transition.did_change_previous(previous);
    }

    /// `ModalRoute.popDisposition`'s `PopEntry` veto (`routes.dart:2033-2042`).
    fn vetoes_pop(&self) -> bool {
        self.inner.pop_entries.any_vetoes()
    }

    /// The route-level hook only. The user-facing `PopScope` fan-out
    /// (`routes.dart:2045-2050`) is **not** fired from here: this runs inside
    /// the flush, under the history lock, where a user callback calling back
    /// into the navigator deadlocks. The flush owes the fan-out through
    /// `FlushOutcome::pop_invoked`, and `apply` delivers it via
    /// [`ModalHandle::notify_pop_invoked`] outside the lock.
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
        // Sever local history first: live entries drop un-fired (Flutter
        // GC-drops the list) and late adds become inert (ADR-0025).
        self.inner.local_history.sever();
        self.inner.release_focus_scope();
        if let Some(binding) = self.binding() {
            binding.withdraw_subtree();
            binding.withdraw_modal();
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
