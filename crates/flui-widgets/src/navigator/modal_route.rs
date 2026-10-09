//! [`ModalRoute`] — a [`TransitionRoute`] that covers the screen with a barrier
//! and a page.
//!
//! Ported under ADR-0020. **Private:** no `PageRoute`, no `PopupRoute`, no public API.
//!
//! # What this layer provides
//!
//! Three things arrive with this layer, and only these three are claimed:
//!
//! 1. **`maintain_state`**, written onto the route's overlay entry. Now real,
//!    because `Overlay` honours it. A covered modal with `maintain_state == false`
//!    is *unmounted*; its subtree state is destroyed and rebuilt fresh when it is
//!    uncovered.
//! 2. **`offstage`**. The page keeps its real geometry but is not painted,
//!    hit-tested or announced — [`Offstage`] over the fixed `RenderOffstage`.
//! 3. **`changed_internal_state`**, which rebuilds *this route's* overlay entry and
//!    republishes `maintain_state`. It does **not** rebuild the navigator.
//!
//! ADR-0021 added the fourth: **`offstage` swaps the animation proxies** to
//! always-complete / always-dismissed, so an offstage route's builders lay it out
//! at its *final* position. That is what lets `HeroController` read a flight's
//! destination one frame early.
//!
//! # One overlay entry, not two
//!
//! FLUI's navigator keys **one** entry per route (see `overlay_route.rs`), so this
//! route builds a `Stack[barrier, page]` into a single entry rather than separate
//! barrier and page entries. The three properties the overlay reads live on that
//! one entry: `opaque`, `maintain_state` and `mark_needs_build()`.
//!
//! The barrier sits below the page either way, so paint and hit-test order are
//! unchanged. Two costs, both recorded: a rebuild for the barrier alone rebuilds
//! the page too, and a covered `maintain_state` route keeps its barrier subtree
//! mounted (the barrier is stateless).
//!
//! # Current limits
//!
//! * **Per-route `FocusScope` — landed (ADR-0026).** The page is wrapped in
//!   `FocusScope::with_external_node` and the current route's scope is installed
//!   through the enclosing scope's first-focus history chain. Still absent:
//!   `traversalEdgeBehavior` (no node-layer flag) and a `requestFocus = false`
//!   opt-out.
//! * **No `BlockSemantics`, no barrier semantics.** No `semanticsDismissible`, no
//!   `barrierLabel`, no semantics sort key. A covered route's semantics are still
//!   announced. The barrier absorbs *pointers* only.
//! * **No animated modal barrier.** `barrier_color` is a flat colour, not driven
//!   through a barrier curve by the route's animation.
//! * **The barrier is not pointer-ignoring while the route pops.** It absorbs
//!   pointers for the whole life of the route, including while it pops.
//! * **No `filter` / `BackdropFilter`, and no cached modal scope.**

// `ModalRoute` is private; `PageRoute` / `PopupRoute` are its production
// consumers and do not surface every knob. `ModalHandle::set_offstage` in
// particular has no public caller until `Hero` drives it.

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use std::sync::OnceLock;

use flui_animation::{Animation, ProxyAnimation};
use flui_foundation::{ChangeNotifier, Listenable, ListenerId};
use flui_painting::styling::Color;
use flui_view::prelude::*;
use flui_view::{AnimatedView, BoxedView, ViewExt, impl_animated_view};
use parking_lot::Mutex;

use super::back_gesture::BackGestureDetector;
use super::binding::{RouteBindingSlot, TransitionGroup};
use super::hero::{HeroHandle, HeroRegistry, HeroScope, HeroTag};
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
use flui_interaction::routing::FocusScopeNode;

use crate::{
    AbsorbPointer, ColoredBox, FocusScope, GestureDetector, Offstage, SizedBox, Stack, StackFit,
};

/// The default transitions builder: a jump cut.
pub(crate) fn default_transitions_builder() -> RouteTransitionsBuilder {
    Rc::new(|_ctx, _animation, _secondary, child| child)
}

/// The mutable half a `ModalRoute` shares with its content builder and its
/// binding. The builder is an `Rc<dyn Fn>` installed in the overlay entry and
/// outlives every borrow of the route, so nothing it reads can live on `self`.
struct ModalInner {
    /// Whether the route is forced offstage.
    offstage: AtomicBool,
    /// Whether a covered route keeps its subtree mounted.
    maintain_state: AtomicBool,
    /// Whether a tap on the barrier pops the route.
    ///
    /// Fixed after construction. A cell here only because `ModalInner` is `Arc`-shared
    /// with the content builder from the moment the route is constructed, so a
    /// `.barrier_dismissible(true)` builder cannot reach it through `&mut`.
    barrier_dismissible: AtomicBool,
    /// Whether this route opts into the edge-swipe-back gesture substrate
    /// (`back_gesture.rs`). Default `false` — see `PageRoute::back_gesture`'s
    /// doc for why. Set only before the route is pushed, exactly like
    /// `maintain_state`/`barrier_dismissible`: toggling it mid-life is not a
    /// supported path, so the detector wrapper's presence in the built tree
    /// never flips (`ModalScopeState::build` reads it once per build, but the
    /// value itself never changes after construction).
    back_gesture_enabled: AtomicBool,
    /// The barrier colour. `None` means an invisible barrier that still absorbs
    /// pointers.
    barrier_color: Mutex<Option<Color>>,

    /// Builds the page.
    page: super::lifecycle::Terminal<RoutePageBuilder>,
    /// Builds the transitions, defaulting to a jump cut. A cell because the
    /// content closure captures `inner` at construction, before a
    /// `.transitions(…)` builder can run.
    transitions: super::lifecycle::Terminal<Mutex<RouteTransitionsBuilder>>,
    /// Set once, immediately after the `TransitionRoute` is constructed. The
    /// content builder is `Arc`-captured *before* that route exists, so the two
    /// cannot be wired the other way round.
    ///
    /// It carries both animations the page and transitions builders read, and the
    /// [`RouteBindingSlot`] `changed_internal_state` writes through.
    transition: OnceLock<TransitionHandle>,

    /// One notifier the modal scope subscribes to, fed by *both* animations.
    ///
    /// `flui_foundation::Listenable` has no `merge`, so a relay stands in, and it
    /// has the property `AnimatedView` needs: the same
    /// object every time `listenable()` is called, even though the `ModalScope`
    /// view is rebuilt on every overlay-entry build.
    relay: super::lifecycle::Terminal<Rc<ChangeNotifier>>,
    /// The relay's subscriptions to the two animations, opened in `install` and
    /// closed in `dispose`. `Listenable` has no `Drop`-based unsubscribe.
    relay_subscriptions: Mutex<Vec<(RouteAnimation, ListenerId)>>,

    /// The primary animation proxy.
    ///
    /// **This — not the controller — is what the page and transitions builders
    /// see.** Its parent is the `TransitionRoute` controller normally, and an
    /// always-complete animation while the route is [`offstage`](Self::offstage).
    /// That swap is the entire reason an offstage route lays out at its
    /// *final* geometry rather than wherever its entrance transition happens to be:
    /// `HeroController` measures the destination one frame before the flight.
    primary: super::lifecycle::Terminal<Rc<ProxyAnimation<f64>>>,
    /// The secondary animation proxy.
    ///
    /// Parent is the `TransitionRoute` secondary train, or an always-dismissed
    /// animation while offstage — an offstage route must not be pushed aside by
    /// whatever sits above it either.
    secondary: super::lifecycle::Terminal<Rc<ProxyAnimation<f64>>>,

    /// The route's page subtree, owned from construction and filled while the
    /// page is mounted. ADR-0021, seam 4.
    subtree: RouteSubtreeCell,

    /// Every `Hero` mounted in this route's page, by tag. FLUI's heroes register
    /// themselves into this one, so no element walk and no downcast is ever
    /// needed. ADR-0021
    heroes: super::lifecycle::Terminal<HeroRegistry>,

    /// Every `PopScope` mounted in this route's page. Consulted by
    /// [`Route::vetoes_pop`] and notified from [`Route::on_pop_invoked`].
    pop_entries: super::lifecycle::Terminal<PopEntryRegistry>,

    /// This route's local-history stack. While non-empty, a pop removes the most recent
    /// entry instead of the route (ADR-0025).
    local_history: super::lifecycle::Terminal<LocalHistoryRegistry>,

    /// The per-route focus scope. The page is wrapped in a `FocusScope::with_external_node`
    /// over this, and the route lifecycle promotes it through native
    /// first-focus history while the route is current.
    focus_scope: super::lifecycle::Terminal<Rc<FocusScopeNode>>,
}

impl Drop for ModalInner {
    fn drop(&mut self) {
        let page = self.page.withdraw();
        let transitions = self.transitions.withdraw();
        let transition = super::lifecycle::Terminal::new(self.transition.take());
        let relay = self.relay.withdraw();
        let subscriptions =
            super::lifecycle::RetiredValues(std::mem::take(self.relay_subscriptions.get_mut()));
        let primary = self.primary.withdraw();
        let secondary = self.secondary.withdraw();
        let heroes = self.heroes.withdraw();
        let pop_entries = self.pop_entries.withdraw();
        let local_history = self.local_history.withdraw();
        let focus_scope = self.focus_scope.withdraw();
        drop((
            page,
            transitions,
            transition,
            relay,
            subscriptions,
            primary,
            secondary,
            heroes,
            pop_entries,
            local_history,
            focus_scope,
        ));
    }
}

impl ModalInner {
    /// Build the modal barrier from the primitives FLUI has.
    ///
    /// `!offstage` gates the barrier — an offstage route must not eat pointers.
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
        let color = *self.barrier_color.lock();
        if let Some(color) = color {
            barrier = barrier.child(ColoredBox::new(color));
        }

        if !self.barrier_dismissible.load(Ordering::Relaxed) {
            return barrier.boxed();
        }

        // A tap on the barrier pops the route. The handle is cloned out from under the tree
        // borrow here and popped later, from the gesture callback.
        let navigator = NavigatorHandle::maybe_of(ctx);
        GestureDetector::new()
            .on_tap(move |_cx| {
                if let Some(navigator) = &navigator {
                    navigator.maybe_pop();
                }
            })
            .child(barrier)
            .boxed()
    }

    /// Build the modal scope, without `Semantics` or a primary scroll controller.
    ///
    /// The `Offstage` wraps the whole scope — so an offstage route's transitions
    /// still run, its page still lays out at real size, and nothing of it paints.
    /// Inside it, `FocusScope::with_external_node` over
    /// [`focus_scope`](Self::focus_scope): heroes, text fields and `Focus`
    /// widgets in the page attach under the route's own scope, so traversal stays
    /// within the route (ADR-0026).
    fn build_scope(self: &Rc<Self>) -> BoxedView {
        let scope = match self.transition.get() {
            Some(transition) => ModalScope {
                page: super::lifecycle::Terminal::new(Rc::clone(&self.page)),
                transitions: super::lifecycle::Terminal::new(Rc::clone(&self.transitions.lock())),
                transition: super::lifecycle::Terminal::new(transition.clone()),
                primary: super::lifecycle::Terminal::new(Rc::clone(&self.primary)),
                secondary: super::lifecycle::Terminal::new(Rc::clone(&self.secondary)),
                relay: super::lifecycle::Terminal::new(Rc::clone(&self.relay)),
                subtree: self.subtree.clone(),
                heroes: super::lifecycle::Terminal::new(self.heroes.clone()),
                pop_entries: super::lifecycle::Terminal::new(self.pop_entries.clone()),
                local_history: super::lifecycle::Terminal::new(self.local_history_handle()),
                back_gesture_enabled: self.back_gesture_enabled.load(Ordering::Relaxed),
            }
            .boxed(),
            // Unreachable in a pushed route: `install()` seeds the `OnceLock`
            // before the overlay ever builds this entry.
            None => SizedBox::shrink().boxed(),
        };

        Offstage::new()
            .offstage(self.offstage.load(Ordering::Relaxed))
            .child(FocusScope::with_external_node(
                Rc::clone(&self.focus_scope),
                scope,
            ))
            .boxed()
    }

    /// Make this route's scope the traversal boundary and move the keyboard
    /// focus into it, so focus lands in the current route. The scope itself resolves its owning
    /// presentation and updates enclosing-scope history; there is no global
    /// active-scope override.
    fn activate_focus_scope(&self) {
        let _ = self.focus_scope.set_first_focus();
    }

    /// The page-facing local-history capability: the registry plus this
    /// route's `changed_internal_state`, owed on the empty↔non-empty edges.
    fn local_history_handle(self: &Rc<Self>) -> LocalHistoryHandle {
        let inner = Rc::clone(self);
        LocalHistoryHandle::new(
            self.local_history.clone(),
            Rc::new(move || changed_internal_state(&inner)),
        )
    }

    /// Repoint both proxies at whatever [`offstage`](Self::offstage) currently
    /// implies — hoisted out of the `offstage` setter so `install()` can run it
    /// too.
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
                transition.secondary_animation() as std::rc::Rc<dyn Animation<f64>>
            })
        });
    }

    /// Point the relay at both **proxies**. Called from `install()`.
    ///
    /// The proxies, not the controller: `ProxyAnimation::set_parent` moves the
    /// listeners with it *and* notifies them, so an offstage swap rebuilds the scope
    /// by itself. That is what carries the completed animation into the page builder
    /// within the same frame. (Nothing ticks an offstage route afterwards — its
    /// parent is a constant — which is fine: there is nothing left to animate.)
    fn open_relay(self: &Rc<Self>) {
        let animations: [RouteAnimation; 2] = [
            Rc::clone(&self.primary) as RouteAnimation,
            Rc::clone(&self.secondary) as RouteAnimation,
        ];
        let mut subscriptions = self.relay_subscriptions.lock();
        for animation in animations {
            let relay = Rc::clone(&self.relay);
            let id = animation.add_listener(std::rc::Rc::new(move || relay.notify_listeners()));
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
// ModalScope — the animation-driven half of the entry
// ============================================================================

/// The modal scope, reduced to the one job FLUI can do today: rebuild the page and
/// its transitions when either animation ticks.
///
/// The page is not cached across ticks, so only the transitions would need to
/// rebuild per frame; but FLUI's `BoxedView` is not cloneable, so the page builder
/// re-runs on every tick. Element reconciliation preserves the page's `ViewState`,
/// so this is a **cost**, not a state difference.
///
/// An [`AnimatedView`] — the framework subscribes to
/// [`listenable`](AnimatedView::listenable) on mount and unsubscribes on unmount.
/// `AnimatedBuilder` could not be used: its builder takes no `BuildContext`, and
/// the page builder needs one.
struct ModalScope {
    page: super::lifecycle::Terminal<RoutePageBuilder>,
    transitions: super::lifecycle::Terminal<RouteTransitionsBuilder>,
    transition: super::lifecycle::Terminal<TransitionHandle>,
    /// The route's animation — the **proxy**, so an offstage route's builders see
    /// an always-complete animation.
    primary: super::lifecycle::Terminal<Rc<ProxyAnimation<f64>>>,
    /// The route's secondary animation proxy.
    secondary: super::lifecycle::Terminal<Rc<ProxyAnimation<f64>>>,
    relay: super::lifecycle::Terminal<Rc<ChangeNotifier>>,
    subtree: RouteSubtreeCell,
    heroes: super::lifecycle::Terminal<HeroRegistry>,
    /// The route's `PopScope` registry, provided to the page as an ambient.
    pop_entries: super::lifecycle::Terminal<PopEntryRegistry>,
    /// The route's local-history handle, provided to the page as an ambient
    /// (ADR-0025).
    local_history: super::lifecycle::Terminal<LocalHistoryHandle>,
    /// Whether this route opted into `back_gesture.rs`'s edge-swipe-back
    /// detector — a per-route, construction-time flag (`ModalRoute::back_gesture`),
    /// never toggled mid-life.
    back_gesture_enabled: bool,
}

impl Clone for ModalScope {
    fn clone(&self) -> Self {
        Self {
            page: super::lifecycle::Terminal::new(self.page.clone()),
            transitions: super::lifecycle::Terminal::new(self.transitions.clone()),
            transition: super::lifecycle::Terminal::new(self.transition.clone()),
            primary: super::lifecycle::Terminal::new(self.primary.clone()),
            secondary: super::lifecycle::Terminal::new(self.secondary.clone()),
            relay: super::lifecycle::Terminal::new(self.relay.clone()),
            heroes: super::lifecycle::Terminal::new(self.heroes.clone()),
            pop_entries: super::lifecycle::Terminal::new(self.pop_entries.clone()),
            local_history: super::lifecycle::Terminal::new(self.local_history.clone()),
            subtree: self.subtree.clone(),
            back_gesture_enabled: self.back_gesture_enabled,
        }
    }
}

impl Drop for ModalScope {
    fn drop(&mut self) {
        let page = self.page.withdraw();
        let transitions = self.transitions.withdraw();
        let transition = self.transition.withdraw();
        let primary = self.primary.withdraw();
        let secondary = self.secondary.withdraw();
        let relay = self.relay.withdraw();
        let heroes = self.heroes.withdraw();
        let pop_entries = self.pop_entries.withdraw();
        let local_history = self.local_history.withdraw();
        drop((
            page,
            transitions,
            transition,
            primary,
            secondary,
            relay,
            heroes,
            pop_entries,
            local_history,
        ));
    }
}

impl_animated_view!(ModalScope);

impl AnimatedView for ModalScope {
    fn listenable(&self) -> std::rc::Rc<dyn Listenable> {
        Rc::clone(&self.relay) as std::rc::Rc<dyn Listenable>
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
    /// Builds the transitions around the page.
    ///
    /// The [`RouteSubtreeAnchor`] wraps **only** the page, inside the transitions.
    /// Anchoring outside the transitions would give `HeroController` the transition's coordinate
    /// space (mid-slide, mid-scale) instead of the page's.
    fn build(&self, view: &ModalScope, ctx: &dyn BuildContext) -> impl IntoView {
        view.transition.drain_pending_statuses();

        let primary: RouteAnimation = Rc::clone(&view.primary) as RouteAnimation;
        let secondary: RouteAnimation = Rc::clone(&view.secondary) as RouteAnimation;
        let page = (view.page)(ctx, &primary, &secondary);
        // The `HeroScope` sits **inside** the subtree anchor, so the anchor stays the
        // route's coordinate root and every hero is a descendant of it — which is what
        // `transform_to(hero, route_subtree)` needs.
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
        // The wrapper's presence never depends on anything read here besides
        // `back_gesture_enabled` itself (a construction-time flag) — the page
        // subtree's identity does not flip across builds. `maybe_of`/`binding`/
        // `controller` are all `Some` for any route that reached this `build`
        // through the normal push → install → mount path; the `None` arm only
        // guards the same "unreachable in a pushed route" bootstrapping window
        // `build_scope`'s own `None` arm documents.
        let anchored = if view.back_gesture_enabled {
            match (
                NavigatorHandle::maybe_of(ctx),
                view.transition.binding(),
                view.transition.controller(),
            ) {
                (Some(navigator), Some(binding), Some(controller)) => {
                    let route = binding.route_id();
                    let enabled_navigator = navigator.clone();
                    BackGestureDetector::new(
                        navigator,
                        route,
                        controller,
                        Rc::new(move || enabled_navigator.pop_gesture_enabled(route)),
                        anchored,
                    )
                    .boxed()
                }
                _ => anchored,
            }
        } else {
            anchored
        };
        (view.transitions)(ctx, &primary, &secondary, anchored)
    }
}

/// A route that covers the routes below it with a barrier and a page.
///
/// Private: not exported until its parity + sign-off gate.
pub struct ModalRoute<T> {
    transition: super::lifecycle::Terminal<TransitionRoute<T>>,
    inner: super::lifecycle::Terminal<Rc<ModalInner>>,
}

impl<T> Drop for ModalRoute<T> {
    fn drop(&mut self) {
        let transition = self.transition.withdraw();
        let inner = self.inner.withdraw();
        drop((transition, inner));
    }
}

impl<T: Send + Clone + 'static> ModalRoute<T> {
    /// A modal showing `page`, entering and leaving over `duration`, with a
    /// jump-cut transition.
    ///
    /// Defaults: `maintain_state = true`, `offstage = false`, no barrier colour,
    /// not dismissible, not opaque.
    pub fn new(duration: Duration, page: RoutePageBuilder) -> Self {
        let inner = Rc::new(ModalInner {
            offstage: AtomicBool::new(false),
            maintain_state: AtomicBool::new(true),
            barrier_dismissible: AtomicBool::new(false),
            back_gesture_enabled: AtomicBool::new(false),
            barrier_color: Mutex::new(None),
            page: super::lifecycle::Terminal::new(page),
            transitions: super::lifecycle::Terminal::new(Mutex::new(default_transitions_builder())),
            transition: OnceLock::new(),
            relay: super::lifecycle::Terminal::new(Rc::new(ChangeNotifier::new())),
            relay_subscriptions: Mutex::new(Vec::new()),
            // Both rest at an always-dismissed animation until `install()` points
            // them at the controller — an unpushed route has no animation to proxy.
            primary: super::lifecycle::Terminal::new(Rc::new(ProxyAnimation::new(
                always_dismissed(),
            ))),
            secondary: super::lifecycle::Terminal::new(Rc::new(ProxyAnimation::new(
                always_dismissed(),
            ))),
            subtree: RouteSubtreeCell::new(),
            heroes: super::lifecycle::Terminal::new(HeroRegistry::new()),
            pop_entries: super::lifecycle::Terminal::new(PopEntryRegistry::new()),
            local_history: super::lifecycle::Terminal::new(LocalHistoryRegistry::new()),
            focus_scope: super::lifecycle::Terminal::new(FocusScopeNode::with_debug_label(
                "ModalRoute Focus Scope",
            )),
        });

        let content = {
            let inner = Rc::clone(&inner);
            move |ctx: &dyn BuildContext| -> BoxedView {
                // Barrier first: it paints below the page and is hit-tested after
                // it.
                let children = vec![inner.build_barrier(ctx), inner.build_scope()];
                Stack::new(children).fit(StackFit::Expand).boxed()
            }
        };

        let transition = TransitionRoute::new(duration, content);
        transition.set_status_wake(Rc::clone(&inner.relay));
        // The content closure captured `inner` before the route existed, so the
        // handle can only be wired in afterwards. `OnceLock` makes that a fact of
        // the type rather than a comment.
        let _ = inner.transition.set(transition.handle());

        Self {
            transition: super::lifecycle::Terminal::new(transition),
            inner: super::lifecycle::Terminal::new(inner),
        }
    }

    /// The builders below mutate `inner` *before* the route is pushed, so no
    /// `changed_internal_state` is needed — the entry does not exist yet.
    pub(crate) fn named(mut self, name: impl Into<String>) -> Self {
        self.transition = super::lifecycle::Terminal::new(self.transition.take_value().named(name));
        self
    }

    /// Whether the route is opaque. `PageRoute` sets this; `PopupRoute` does not.
    #[must_use]
    pub fn opaque(mut self, opaque: bool) -> Self {
        self.transition =
            super::lifecycle::Terminal::new(self.transition.take_value().opaque(opaque));
        self
    }

    /// The transitions builder.
    pub(crate) fn transitions(self, transitions: RouteTransitionsBuilder) -> Self {
        let _prev = std::mem::replace(&mut *self.inner.transitions.lock(), transitions);
        self
    }

    /// The forward transition duration.
    pub(crate) fn duration(mut self, duration: Duration) -> Self {
        self.transition =
            super::lifecycle::Terminal::new(self.transition.take_value().duration(duration));
        self
    }

    /// The reverse transition duration.
    pub(crate) fn reverse_duration(mut self, duration: Duration) -> Self {
        self.transition = super::lifecycle::Terminal::new(
            self.transition.take_value().reverse_duration(duration),
        );
        self
    }

    /// The transition family — see [`TransitionGroup`].
    pub(crate) fn group(mut self, group: TransitionGroup) -> Self {
        self.transition =
            super::lifecycle::Terminal::new(self.transition.take_value().group(group));
        self
    }

    /// The result a pop with no explicit result delivers.
    pub(crate) fn with_current_result(mut self, result: T) -> Self {
        self.transition = super::lifecycle::Terminal::new(
            self.transition.take_value().with_current_result(result),
        );
        self
    }

    /// Whether a covered route keeps its subtree mounted.
    #[must_use]
    pub fn maintain_state(self, maintain_state: bool) -> Self {
        self.inner
            .maintain_state
            .store(maintain_state, Ordering::Relaxed);
        self
    }

    /// Whether a tap on the barrier pops the route.
    #[must_use]
    pub fn barrier_dismissible(self, dismissible: bool) -> Self {
        self.inner
            .barrier_dismissible
            .store(dismissible, Ordering::Relaxed);
        self
    }

    /// Opt into the edge-swipe-back gesture substrate (`back_gesture.rs`).
    /// See `PageRoute::back_gesture`'s doc for the full contract.
    pub(crate) fn back_gesture(self, enabled: bool) -> Self {
        self.inner
            .back_gesture_enabled
            .store(enabled, Ordering::Relaxed);
        self
    }

    /// The barrier colour.
    #[must_use]
    pub fn barrier_color(self, color: Color) -> Self {
        *self.inner.barrier_color.lock() = Some(color);
        self
    }

    /// A cloneable view of this route's modal state, obtainable **before** the
    /// route is moved into `NavigatorHandle::push`.
    ///
    /// Production since ADR-0021: `HeroController` drives `set_offstage` through
    /// the copy this route publishes into the navigator's registry at `install()`.
    #[must_use]
    pub fn handle(&self) -> ModalHandle {
        ModalHandle {
            inner: Rc::clone(&self.inner),
        }
    }

    /// The transition handle, for driving the animation by hand.
    #[must_use]
    pub fn transition_handle(&self) -> super::transition_route::TransitionHandle {
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
/// state — the same pattern used elsewhere in the navigator: the route itself
/// lives behind `Box<dyn ErasedRoute>` inside the history's mutex and cannot
/// be reached directly.
///
/// This is how a route is forced offstage. `HeroController` holds
/// one per route, looked up by [`RouteId`] through the navigator's registry.
#[derive(Clone)]
pub struct ModalHandle {
    inner: Rc<ModalInner>,
}

impl fmt::Debug for ModalHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModalHandle")
            .field("offstage", &self.inner.offstage.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

/// `dead_code` in the lib target: `HeroController` is this handle's only production
/// caller, and it is itself dead until the `Hero` widget uses it. See `hero_controller.rs`.
#[expect(dead_code)]
impl ModalHandle {
    /// Deliver the pop-invoked callback (`did_pop`, …) to every `PopScope`
    /// registered in this route's page. Called by
    /// `NavigatorShared::apply` **outside** the history lock — the callbacks
    /// are user code and may call back into the navigator.
    pub(crate) fn notify_pop_invoked(&self, did_pop: bool) {
        self.inner.pop_entries.notify_pop_invoked(did_pop);
    }

    /// Fire the `on_remove`s owed by local-history pops that happened inside
    /// the flush, and the emptied-edge `changed_internal_state`. Called by
    /// `NavigatorShared::apply` with **no lock held** (ADR-0025).
    /// Make this route's focus scope the active one and restore the focus it
    /// remembers. Called by the navigator from
    /// `apply`, **outside** the history lock: this moves the primary focus, and
    /// the listeners that fire are user code.
    pub(crate) fn activate_focus_scope(&self) {
        self.inner.activate_focus_scope();
    }

    pub(crate) fn drain_local_history(&self) {
        let (callbacks, emptied) = self.inner.local_history.take_owed();
        for callback in callbacks {
            callback();
        }
        if emptied {
            changed_internal_state(&self.inner);
        }
    }

    /// Force the route offstage (or back), whole: the early return on an
    /// unchanged value, the animation-proxy swap (ADR-0021), and
    /// `changed_internal_state`.
    pub fn set_offstage(&self, offstage: bool) {
        if self.inner.offstage.swap(offstage, Ordering::Relaxed) == offstage {
            return; // Unchanged.
        }
        // Offstage points the primary proxy at an always-complete animation and the
        // secondary at an always-dismissed one; back onstage restores the real
        // ones. This runs before `changed_internal_state`, so the rebuild it
        // schedules already sees the swapped animations.
        self.inner.sync_animation_proxies();
        changed_internal_state(&self.inner);
    }

    /// Whether the route is forced offstage.
    #[must_use]
    pub fn offstage(&self) -> bool {
        self.inner.offstage.load(Ordering::Relaxed)
    }

    /// What the route's builders currently see as the route animation — the
    /// proxy, so `1.0`/completed while offstage.
    pub(crate) fn primary_animation(&self) -> RouteAnimation {
        Rc::clone(&self.inner.primary) as RouteAnimation
    }

    /// The route's secondary animation — `0.0`/dismissed while offstage.
    pub(crate) fn secondary_animation(&self) -> RouteAnimation {
        Rc::clone(&self.inner.secondary) as RouteAnimation
    }

    /// The heroes mounted in this route's page, as a registry rather than an
    /// element walk.
    #[must_use]
    pub fn heroes(&self) -> HeroRegistry {
        self.inner.heroes.clone()
    }

    /// Every hero visible for a flight through this route: this route's own,
    /// plus — recursively — whatever each nested `Navigator` mounted inside it
    /// publishes for its own current top `PageRoute`.
    #[must_use]
    pub fn all_heroes(&self) -> HashMap<HeroTag, HeroHandle> {
        self.inner.heroes.all_heroes()
    }

    /// Whether this route keeps its subtree built while covered. Read by
    /// `HeroController::maybe_start`'s gesture-pop sync fast path — a
    /// destination that does not maintain state may not be laid out yet, so
    /// only a `true` here can skip the offstage measurement dance.
    #[must_use]
    pub fn maintain_state(&self) -> bool {
        self.inner.maintain_state.load(Ordering::Relaxed)
    }

    /// Set `maintain_state` and republish it through `changed_internal_state`.
    /// The value sits in a cell, which is what lets a test observe the republish.
    pub fn set_maintain_state(&self, maintain_state: bool) {
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

/// Rebuilds this route's overlay entry and republishes `maintain_state`. No
/// scheduler-phase guard is needed: FLUI's `mark_needs_build` only inserts an id
/// into an inbox the next `build_scope` drains, so it is already safe from any
/// phase (`entry.rs` module docs).
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

impl<T: Send + Clone + 'static> ModalRoute<T> {
    /// This route's navigator capability, or `None` before it is pushed.
    fn binding(&self) -> Option<super::binding::RouteBinding> {
        self.inner
            .transition
            .get()
            .and_then(TransitionHandle::binding)
    }
}

impl<T: Send + Clone + 'static> Route for ModalRoute<T> {
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

    /// Non-empty local history claims the pop.
    fn will_handle_pop_internally(&self) -> bool {
        !self.inner.local_history.is_empty() || self.transition.will_handle_pop_internally()
    }

    /// The transition route builds the controller. FLUI's entry is created by
    /// `push_bound` just before the flush, so the only thing left here is
    /// publishing `maintain_state` onto it.
    fn install(&mut self) {
        self.transition.install();
        // Order: the controller must exist before the proxies can point at it
        // (transition install, then the two proxies), and the relay subscribes to the proxies, so it goes last.
        self.inner.sync_animation_proxies();
        self.inner.open_relay();
        if let Some(binding) = self.binding() {
            binding.set_entry_maintain_state(self.inner.maintain_state.load(Ordering::Relaxed));
            // Registered before the page has ever been built, so the registry
            // knows the route exists; it resolves to `None` until the page mounts.
            binding.publish_subtree(self.inner.subtree.clone());
            // `HeroController` reaches the route's `offstage` through this, by id.
            binding.publish_modal(self.handle());
        }
    }

    /// A push (like an add) moves the focus into the route's scope.
    /// Focus activation is **not** done here: this runs inside the flush, under
    /// the history lock, and moving the focus fires user listeners (a `Focus`
    /// widget's `on_focus_change` and its rebuild) that may call back into the
    /// navigator — a same-thread deadlock on the non-reentrant mutex. The
    /// navigator activates the new top route's scope from `apply`, once the
    /// lock is released.
    fn did_push(&mut self) -> PushCompletion {
        self.transition.did_push()
    }

    fn did_add(&mut self) {
        self.transition.did_add();
    }

    fn did_replace(&mut self, previous: Option<RouteId>) {
        self.transition.did_replace(previous);
    }

    /// While local-history entries exist, pop the most recent one and answer `false` — the route stays and
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

    /// The route above popped: this one is current again, and is re-focused.
    fn did_pop_next(&mut self, popped: RouteId) {
        self.transition.did_pop_next(popped);
    }

    fn did_change_next(&mut self, next: Option<RouteId>) {
        // Becoming topmost re-activates this route's scope whenever it becomes
        // current; a pop announces `did_change_next(None)` to the revealed route.
        self.transition.did_change_next(next);
    }

    fn did_change_previous(&mut self, previous: Option<RouteId>) {
        self.transition.did_change_previous(previous);
    }

    /// Whether any registered `PopScope` vetoes the pop.
    fn vetoes_pop(&self) -> bool {
        self.inner.pop_entries.any_vetoes()
    }

    /// The route-level hook only. The user-facing `PopScope` fan-out is **not** fired from here: this runs inside
    /// the flush, under the history lock, where a user callback calling back
    /// into the navigator deadlocks. The flush owes the fan-out through
    /// `FlushOutcome::pop_invoked`, and `apply` delivers it via
    /// `ModalHandle::notify_pop_invoked` outside the lock.
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
        // Sever local history first: live entries drop un-fired and late adds
        // become inert (ADR-0025).
        self.inner.local_history.sever();
        if let Some(binding) = self.binding() {
            binding.withdraw_subtree();
            binding.withdraw_modal();
        }
        self.inner.close_relay();
        self.transition.dispose();
    }
}

impl<T: Send + Clone + 'static> NavigatorRoute for ModalRoute<T> {
    fn content_builder(&self) -> RouteContentBuilder {
        self.transition.content_builder()
    }

    fn binding_slot(&self) -> Option<&RouteBindingSlot> {
        self.transition.binding_slot()
    }
}
