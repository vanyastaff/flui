//! [`Navigator`], [`NavigatorState`] and [`NavigatorHandle`].
//!
//! ADR-0019 U3. **Private**: nothing here is exported from the crate root or the
//! prelude. U4's parity + sign-off gate decides what, if anything, becomes public.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/navigator.dart` (master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`): `NavigatorState`, `Navigator.of` /
//! `maybeOf` (`:2947-3001`), `canPop` (`:5551`), `maybePop` (`:5582`),
//! `_allRouteOverlayEntries` (`:4151`), and `build` returning an `Overlay`
//! (`:5984`).
//!
//! # How this avoids Flutter's two `GlobalKey`s, and the lock hazard behind them
//!
//! ADR-0019 §3.2. `BuildContext::find_ancestor_state` yields `&dyn Any` —
//! *immutable* — while the element tree is borrowed. So `Navigator::of` can never
//! return `&mut NavigatorState`, and it must not perform a second lookup inside
//! that callback: Flutter's `_overlayKey.currentState` would take the GlobalKey
//! registry's `WidgetsBinding::inner.read()` while the tree borrow is held, and
//! `parking_lot::RwLock` is not reentrant.
//!
//! Both problems dissolve the same way. `Navigator::of` clones an owned,
//! `'static` [`NavigatorHandle`] out of the state *inside* the callback and does
//! nothing else there; every mutation runs after the borrow is released. Navigator
//! and Overlay couple through an `Arc`, not through the tree — so
//! `GlobalKey<OverlayState>` is not ported either, and `navigator_uses_no_global_key`
//! keeps it that way.
//!
//! # Not implemented, and not claimed
//!
//! No `TransitionRoute` / `ModalRoute` / `PageRoute` (so: no animation, no
//! barrier, no focus scope), no `Hero`, no page-based routing, no restoration, no
//! named-route generation, no `PopScope`, no `LocalHistoryRoute`, no
//! `HeroControllerScope` / `NavigationNotification` / pointer-cancelling wrapper
//! that Flutter's `build` adds (`:5946-5998`). ADR-0019 §5–§6 owns the sequence.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use flui_view::BuildContextExt;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use parking_lot::Mutex;

use super::history::{FlushOutcome, RouteHistory};
use super::observer::NavigatorObserver;
use super::overlay_route::NavigatorRoute;
use super::result::RouteResult;
use super::route::{AnyResult, RouteId, RoutePopDisposition};
use crate::overlay::{Overlay, OverlayEntry, OverlayHandle};

/// Everything a [`NavigatorHandle`] and the mounted [`NavigatorState`] share.
///
/// The route stack lives behind a private `Mutex` because `ViewState::build` takes
/// `&self` and nothing can obtain `&mut NavigatorState` — ADR-0019 §3.2. That is
/// not a workaround: Flutter's `NavigatorState` mutates `_history` from `this` too.
struct NavigatorShared {
    history: Mutex<RouteHistory>,

    /// The overlay this navigator presents its routes in. Flutter reaches it
    /// through `GlobalKey<OverlayState>` (`navigator.dart:3746`); we hold the
    /// capability directly.
    overlay: OverlayHandle,

    /// `RouteId -> OverlayEntry`. Flutter stores these on the route
    /// (`OverlayRoute.overlayEntries`); see `overlay_route.rs` for why FLUI cannot.
    entries: Mutex<HashMap<RouteId, OverlayEntry>>,
}

impl NavigatorShared {
    /// Apply what a flush left behind — Flutter's tail of `_flushHistoryUpdates`
    /// (`navigator.dart:4609-4613`), in that order:
    ///
    /// 1. remove each disposed route's overlay entries (`_disposeRouteEntry`);
    /// 2. `overlay.rearrange(_allRouteOverlayEntries)`, but **only** when the
    ///    flush asked for it. `pop` and `remove_route` pass `rearrangeOverlay:
    ///    false` (`:5671`, `:5747`) precisely because step 1 already updated the
    ///    overlay's list.
    fn apply(&self, outcome: &FlushOutcome) {
        {
            let mut entries = self.entries.lock();
            for id in &outcome.disposed {
                if let Some(entry) = entries.remove(id)
                    && entry.is_attached()
                {
                    entry.remove();
                }
            }
        }

        if !outcome.rearrange_overlay {
            return;
        }

        // `_allRouteOverlayEntries`: the entries of every route in `_history`
        // order, bottom → top (`navigator.dart:4151-4153`).
        let ordered: Vec<OverlayEntry> = {
            let ids = self.history.lock().ids();
            let entries = self.entries.lock();
            ids.iter()
                .filter_map(|id| entries.get(id).cloned())
                .collect()
        };
        self.overlay.rearrange(&ordered);
    }

    /// Run `mutate` against the stack, then apply whatever it flushed.
    ///
    /// The history lock is **released before** the overlay work, so no lock is
    /// held across `RebuildHandle::schedule`.
    fn mutate<R>(&self, mutate: impl FnOnce(&mut RouteHistory) -> R) -> R {
        let (value, outcome) = {
            let mut history = self.history.lock();
            let value = mutate(&mut history);
            (value, history.take_outcome())
        };
        if let Some(outcome) = outcome {
            self.apply(&outcome);
        }
        value
    }
}

/// An owned, `'static` capability to drive a [`Navigator`].
///
/// This is what `Navigator::of` returns — never `&mut NavigatorState`, which no
/// caller can obtain. Cloneable, `Send + Sync`, and inert once the navigator
/// unmounts.
#[derive(Clone)]
pub struct NavigatorHandle {
    shared: Arc<NavigatorShared>,
}

impl NavigatorHandle {
    /// A handle to an empty, unmounted navigator. Seed it, hand it to
    /// [`Navigator::new`], and keep a clone.
    #[must_use]
    pub fn new() -> Self {
        Self {
            shared: Arc::new(NavigatorShared {
                history: Mutex::new(RouteHistory::new()),
                overlay: OverlayHandle::new(),
                entries: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Register an observer. Flutter's `Navigator.observers`.
    pub fn add_observer(&self, observer: Arc<dyn NavigatorObserver>) {
        self.shared.history.lock().add_observer(observer);
    }

    /// Whether the navigator is mounted. Flutter's `State.mounted`, consulted by
    /// `maybePop` (`navigator.dart:5595`).
    ///
    /// Derived from the overlay rather than a separate flag: the overlay is this
    /// navigator's only child, so it is mounted exactly when the navigator is.
    #[must_use]
    pub fn is_mounted(&self) -> bool {
        self.shared.overlay.is_mounted()
    }

    /// Seed an initial route **without flushing** — Flutter's `restoreState`
    /// (`navigator.dart:3900-3934`), which appends every route
    /// `onGenerateInitialRoutes` produced and flushes exactly once, on mount.
    ///
    /// Seed before handing the handle to [`Navigator::new`]. A deep link's
    /// synthesized back-stack is several `seed_initial` calls.
    pub fn seed_initial<R: NavigatorRoute>(&self, route: R) -> RouteResult<R::Output> {
        let builder = route.content_builder();
        let (id, result) = self.shared.history.lock().seed_initial(route);
        self.shared
            .entries
            .lock()
            .insert(id, OverlayEntry::new(move |ctx| builder(ctx)));
        result
    }

    /// Flutter's `NavigatorState.push` (`navigator.dart:5060-5063`). The future is
    /// created before any lifecycle runs.
    pub fn push<R: NavigatorRoute>(&self, route: R) -> RouteResult<R::Output> {
        let builder = route.content_builder();

        // The entry must exist before the flush's overlay work runs, but the id is
        // minted by `push`. Insert between the two, with the history lock released.
        let (id, result, outcome) = {
            let mut history = self.shared.history.lock();
            let (id, result) = history.push(route);
            (id, result, history.take_outcome())
        };
        self.shared
            .entries
            .lock()
            .insert(id, OverlayEntry::new(move |ctx| builder(ctx)));

        if let Some(outcome) = outcome {
            self.shared.apply(&outcome);
        }
        result
    }

    fn pop_erased(&self, result: Option<AnyResult>) -> bool {
        self.shared.mutate(|history| history.pop(result))
    }

    fn remove_route_erased(&self, id: RouteId, result: Option<AnyResult>) -> bool {
        self.shared
            .mutate(|history| history.remove_route(id, result))
    }

    /// Pop the top route with no result — Flutter's `Navigator.pop()`
    /// (`navigator.dart:5642-5675`).
    ///
    /// The popped route's future resolves with its `current_result()` fallback, or
    /// `None`. Returns whether a present route was found. A route that refuses
    /// (`Route::did_pop` → `false`) stays, and this still returns `true`.
    pub fn pop(&self) -> bool {
        self.pop_erased(None)
    }

    /// Pop the top route, delivering `result` to whoever awaits its
    /// [`RouteResult`] — Flutter's `Navigator.pop(result)`.
    ///
    /// `T` is checked at **delivery**, not at the call site: the navigator holds a
    /// heterogeneous stack and cannot know the top route's `Output`. Passing the
    /// wrong type logs an error and completes the future with `None` rather than
    /// panicking. ADR-0019 §4; Flutter throws a cast error here.
    pub fn pop_with<T: Send + 'static>(&self, result: T) -> bool {
        self.pop_erased(Some(Box::new(result)))
    }

    /// Remove `id` without popping it — Flutter's `Navigator.removeRoute`
    /// (`:5733-5751`).
    ///
    /// **The removed route still completes its future**, with its
    /// `current_result()` fallback or `None`. A port that completed only on `pop`
    /// would hang every awaiter.
    pub fn remove_route(&self, id: RouteId) -> bool {
        self.remove_route_erased(id, None)
    }

    /// Remove `id`, delivering `result`. Same type contract as
    /// [`pop_with`](NavigatorHandle::pop_with).
    pub fn remove_route_with<T: Send + 'static>(&self, id: RouteId, result: T) -> bool {
        self.remove_route_erased(id, Some(Box::new(result)))
    }

    /// Flutter's `NavigatorState.canPop` (`:5551-5566`).
    #[must_use]
    pub fn can_pop(&self) -> bool {
        self.shared.history.lock().can_pop()
    }

    /// Flutter's `NavigatorState.maybePop` (`:5582-5615`), minus the deprecated
    /// `willPop` await — which is the only reason Flutter's is `async`. The
    /// remaining logic is a synchronous `switch` on `popDisposition`, and porting
    /// it as `async fn` would buy nothing and violate the no-async-in-hot-paths
    /// rule.
    ///
    /// Returns whether the pop request was **handled**. `false` means "bubble":
    /// nobody here dealt with it, so an ancestor navigator or the system should.
    fn maybe_pop_erased(&self, result: Option<AnyResult>) -> bool {
        if !self.is_mounted() {
            // "Forget about this pop, we were disposed in the meantime." (`:5595`)
            return true;
        }

        let Some(disposition) = self.shared.history.lock().pop_disposition_of_top() else {
            return false;
        };

        match disposition {
            RoutePopDisposition::Bubble => false,
            RoutePopDisposition::Pop => {
                self.pop_erased(result);
                true
            }
            RoutePopDisposition::DoNotPop => {
                self.shared.mutate(RouteHistory::notify_pop_refused);
                true
            }
        }
    }

    /// Consult the top route's `popDisposition` and act on it, with no result.
    ///
    /// Returns whether the pop request was **handled**. `false` means "bubble":
    /// nothing here dealt with it, so an ancestor navigator or the system should —
    /// which is what a lone route does (`popDisposition` is `isFirst ? bubble : pop`).
    pub fn maybe_pop(&self) -> bool {
        self.maybe_pop_erased(None)
    }

    /// [`maybe_pop`](NavigatorHandle::maybe_pop), delivering `result` if it pops.
    pub fn maybe_pop_with<T: Send + 'static>(&self, result: T) -> bool {
        self.maybe_pop_erased(Some(Box::new(result)))
    }

    /// The topmost present route.
    #[must_use]
    pub fn current(&self) -> Option<RouteId> {
        self.shared.history.lock().current()
    }

    /// The route stack, bottom → top.
    #[must_use]
    pub fn route_ids(&self) -> Vec<RouteId> {
        self.shared.history.lock().ids()
    }

    /// The overlay entries currently presented, bottom → top. Test-facing.
    #[cfg(test)]
    pub(crate) fn overlay_handle(&self) -> &OverlayHandle {
        &self.shared.overlay
    }

    /// How many `RouteId -> OverlayEntry` pairs the navigator is holding.
    ///
    /// Test-facing. Must track the route count exactly: an entry left behind for
    /// a disposed route is invisible in the overlay (it was removed from *its*
    /// list) but leaks here, forever.
    #[cfg(test)]
    pub(crate) fn tracked_entry_count(&self) -> usize {
        self.shared.entries.lock().len()
    }

    // ── Lookup ───────────────────────────────────────────────────────────────

    /// The nearest enclosing navigator, or `None`.
    ///
    /// Flutter's `Navigator.maybeOf(context)` (`navigator.dart:2992-3001`).
    ///
    /// Clones an owned handle out under the tree borrow and returns it; it takes
    /// no second lock and consults no `GlobalKey` registry. See the module docs.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<Self> {
        ctx.find_state::<NavigatorState, _>(NavigatorState::handle)
    }

    /// The **root-most** navigator — Flutter's `Navigator.of(context,
    /// rootNavigator: true)` → `findRootAncestorStateOfType<NavigatorState>()`
    /// (`navigator.dart:2947-2968`), which is how you push above every nested
    /// navigator.
    ///
    /// Flutter falls back to the local navigator when the root walk finds none;
    /// here the root walk cannot find fewer navigators than the nearest walk, so
    /// the fallback is unreachable and omitted.
    #[must_use]
    pub fn maybe_of_root(ctx: &dyn BuildContext) -> Option<Self> {
        ctx.find_root_state::<NavigatorState, _>(NavigatorState::handle)
    }
}

impl Default for NavigatorHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for NavigatorHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NavigatorHandle")
            .field("routes", &self.shared.history.lock().len())
            .field("mounted", &self.is_mounted())
            .finish()
    }
}

// ============================================================================
// THE VIEW
// ============================================================================

/// A stack of routes, presented in an overlay.
///
/// The stack lives in the [`NavigatorHandle`] the caller supplies, so it survives
/// this view being rebuilt and can be driven from outside the tree.
#[derive(Clone)]
pub struct Navigator {
    handle: NavigatorHandle,
}

impl Navigator {
    /// A navigator backed by `handle`. Seed its initial route(s) first.
    #[must_use]
    pub fn new(handle: NavigatorHandle) -> Self {
        Self { handle }
    }
}

impl fmt::Debug for Navigator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Navigator").finish_non_exhaustive()
    }
}

impl View for Navigator {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for Navigator {
    type State = NavigatorState;

    fn create_state(&self) -> Self::State {
        NavigatorState {
            shared: Arc::clone(&self.handle.shared),
        }
    }
}

/// Persistent state for [`Navigator`]. Flutter's `NavigatorState`.
///
/// Holds nothing of its own: the stack and the overlay live behind the shared
/// `Arc`, because they must be reachable from an owned handle that outlives any
/// borrow of this state.
pub struct NavigatorState {
    shared: Arc<NavigatorShared>,
}

impl fmt::Debug for NavigatorState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NavigatorState")
            .field("routes", &self.shared.history.lock().len())
            .finish_non_exhaustive()
    }
}

impl NavigatorState {
    /// The owned capability `Navigator::of` hands out. **This is the only thing a
    /// lookup takes from the state**, and it is a clone of two `Arc`s.
    fn handle(&self) -> NavigatorHandle {
        NavigatorHandle {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl ViewState<Navigator> for NavigatorState {
    /// Flush the seeded initial routes, exactly once — Flutter's `restoreState`
    /// tail (`navigator.dart:3922-3934`), which asserts the history is non-empty
    /// and then calls `_flushHistoryUpdates()`.
    ///
    /// The overlay is not mounted yet (it is this view's child, built next), so
    /// the rearrange only fills the overlay's entry list; its first `build` reads
    /// it. Mutating an unmounted `OverlayHandle` is defined behavior (U1).
    ///
    /// No `rebuild_handle()` is acquired here or anywhere in this file: the
    /// overlay owns its own rebuild, so trigger #22 has nothing to guard.
    fn init_state(&mut self, _ctx: &dyn BuildContext) {
        debug_assert!(
            self.shared.history.lock().len() > 0,
            "BUG: a Navigator was mounted with no routes — seed one before mounting \
             (navigator.dart:3922 asserts the same)"
        );
        self.shared.mutate(|history| {
            history.flush(true);
        });
    }

    /// Flutter's `NavigatorState.build` returns an `Overlay` and nothing else that
    /// matters here (`navigator.dart:5984-5990`); its `HeroControllerScope`,
    /// `NavigationNotification` listener, pointer-cancelling `Listener` and
    /// `FocusTraversalGroup` all belong to features deferred by ADR-0019 §6.
    fn build(&self, _view: &Navigator, _ctx: &dyn BuildContext) -> impl IntoView {
        Overlay::new(self.shared.overlay.clone())
    }
}
