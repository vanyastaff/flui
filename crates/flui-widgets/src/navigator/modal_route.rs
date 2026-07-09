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

// `ModalRoute` is reached only through `NavigatorHandle::push_bound`, whose
// production caller is `PageRoute` / `PopupRoute` (U5.4). Until then only the
// tests push one, so rustc sees most of this file as dead. Same posture, and the
// same promise, as `transition_route.rs`.
#![allow(dead_code)]

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use flui_types::Color;
use flui_view::{BoxedView, BuildContext, ViewExt};
use parking_lot::Mutex;

use super::binding::{BoundRoute, RouteBinding};
use super::navigator::NavigatorHandle;
use super::overlay_route::{NavigatorRoute, RouteContentBuilder};
use super::route::{PushCompletion, Route, RouteId, RouteSettings};
use super::transition_route::TransitionRoute;
use crate::{AbsorbPointer, ColoredBox, GestureDetector, Offstage, Stack, StackFit};

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
    /// Published by `bind`, before `install`. `None` for an unpushed route, which
    /// makes [`changed_internal_state`] correctly inert.
    binding: Mutex<Option<RouteBinding>>,
}

impl ModalInner {
    /// `_buildModalBarrier` + `buildModalBarrier` (`routes.dart:2273-2330`),
    /// reduced to the primitives FLUI has.
    ///
    /// `!offstage` gates the barrier, exactly as `buildModalBarrier` does
    /// (`:2301`) — an offstage route must not eat pointers.
    fn build_barrier(&self, ctx: &dyn BuildContext) -> BoxedView {
        if self.offstage.load(Ordering::Relaxed) {
            return AbsorbPointer::new().absorbing(false).boxed();
        }

        // `ColoredBox` is the hit-testable full-size box; a `None` colour paints
        // nothing but is still hit, which is what a dismissible-but-invisible
        // barrier needs.
        let colored = ColoredBox::new(self.barrier_color.lock().unwrap_or(Color::TRANSPARENT));

        if !self.barrier_dismissible.load(Ordering::Relaxed) {
            return AbsorbPointer::new().absorbing(true).child(colored).boxed();
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
            .child(AbsorbPointer::new().absorbing(true).child(colored))
            .boxed()
    }

    /// `_buildModalScope` (`routes.dart:2333-2345`), minus `Semantics`,
    /// `_ModalScope` and its `FocusScope`.
    fn build_scope(&self, page: &RouteContentBuilder, ctx: &dyn BuildContext) -> BoxedView {
        Offstage::new()
            .offstage(self.offstage.load(Ordering::Relaxed))
            .child(page(ctx))
            .boxed()
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
    /// A modal showing `page`, entering and leaving over `duration`.
    ///
    /// Defaults match Flutter's `ModalRoute`: `maintain_state = true`
    /// (`routes.dart:2394` for `PopupRoute`, and `PageRoute`'s constructor
    /// argument defaults to `true`), `offstage = false`, no barrier colour, not
    /// dismissible, not opaque.
    pub(crate) fn new(
        duration: Duration,
        page: impl Fn(&dyn BuildContext) -> BoxedView + Send + Sync + 'static,
    ) -> Self {
        let inner = Arc::new(ModalInner {
            offstage: AtomicBool::new(false),
            maintain_state: AtomicBool::new(true),
            barrier_dismissible: AtomicBool::new(false),
            barrier_color: Mutex::new(None),
            binding: Mutex::new(None),
        });

        let page: RouteContentBuilder = Arc::new(page);
        let content = {
            let inner = Arc::clone(&inner);
            move |ctx: &dyn BuildContext| -> BoxedView {
                // Barrier first: it paints below the page and is hit-tested after
                // it, matching `[_modalBarrier, _modalScope]` entry order.
                let children = vec![inner.build_barrier(ctx), inner.build_scope(&page, ctx)];
                Stack::new(children).fit(StackFit::Expand).boxed()
            }
        };

        Self {
            transition: TransitionRoute::new(duration, content),
            inner,
        }
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
    /// route is moved into `push_bound`.
    pub(crate) fn handle(&self) -> ModalHandle {
        ModalHandle {
            inner: Arc::clone(&self.inner),
        }
    }

    /// The transition handle, for driving the animation by hand.
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
#[derive(Clone)]
pub(crate) struct ModalHandle {
    inner: Arc<ModalInner>,
}

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
fn changed_internal_state(inner: &ModalInner) {
    let binding = inner.binding.lock().clone();
    let Some(binding) = binding else { return };
    binding.set_entry_maintain_state(inner.maintain_state.load(Ordering::Relaxed));
    binding.mark_entry_needs_build();
}

// ============================================================================
// Route delegation
// ============================================================================

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
        if let Some(binding) = self.inner.binding.lock().as_ref() {
            binding.set_entry_maintain_state(self.inner.maintain_state.load(Ordering::Relaxed));
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

    fn dispose(&mut self) {
        self.transition.dispose();
    }
}

impl<T: Send + Sync + Clone + 'static> NavigatorRoute for ModalRoute<T> {
    fn content_builder(&self) -> RouteContentBuilder {
        self.transition.content_builder()
    }
}

impl<T: Send + Sync + Clone + 'static> BoundRoute for ModalRoute<T> {
    fn bind(&mut self, binding: RouteBinding) {
        *self.inner.binding.lock() = Some(binding.clone());
        self.transition.bind(binding);
    }
}
