//! [`Route`] — the typed route trait — and [`ErasedRoute`], the type-erased view
//! of it that a heterogeneous route stack can hold.
//!
//! Private; nothing here is exported.
//!
//! # Flutter parity
//!
//! `navigator.dart:161` (`abstract class Route<T>`) and `:424-643`. FLUI splits
//! Flutter's one class in two:
//!
//! | Flutter `Route<T>` owns | FLUI |
//! |---|---|
//! | the lifecycle hooks | [`Route`] — user-implemented, typed |
//! | `_popCompleter` / `popped` / `_installed` / `_navigator` | [`RouteRecord`] — framework-owned |
//!
//! The split is forced. `_popCompleter` must be completed by machinery that only
//! sees `dyn ErasedRoute`, and a default method on a trait cannot own state.
//! Behavior is unchanged: `did_pop` still completes the future, `did_complete`
//! still applies the `result ?? currentResult` fallback.
//!
//! # The type-erasure boundary — **private, unauthorized**
//!
//! `Vec<Box<dyn Route<Output = T>>>` cannot hold routes with different `T`, so
//! the stack holds `Box<dyn ErasedRoute>` and a pop result crosses a
//! `Box<dyn Any + Send>` boundary, downcast in [`RouteRecord::did_complete`].
//! Flutter has the same runtime failure mode (`Route<dynamic>` plus an unchecked
//! `pop<T>`), but Rust would not otherwise need it.
//!
//! **This does not authorize the public shape.** A later API sign-off gate still
//! owns that decision, and the erasure is confined to this private module until then.
//! Note also that `flui-widgets` is outside port-check's FR-036 (`dyn`-boundary
//! registry, trigger 9) and FR-033 (downcast) scopes, so **no gate would have
//! caught this** — which is a reason to keep it private, not a licence to export.
//!
//! On a type mismatch FLUI logs and completes with `None`, where Flutter throws a
//! cast error. A wrong `pop` type is caller error, and
//! [`PANIC-POLICY`](../../../../../docs/PANIC-POLICY.md) reserves panics for
//! framework invariants. Pinned by `pop_with_mismatched_result_type_yields_none`.

use std::any::Any;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::result::{Completer, RouteResult};

/// A pop result, erased. See the module docs.
pub(crate) type AnyResult = Box<dyn Any + Send>;
// Deliberately **not** public. `NavigatorHandle::pop_with<T>` takes a typed `T`
// and erases it here, so the erasure is an implementation detail rather than a
// shape callers must name. The boundary was signed off, not its exposure.

/// Process-unique route identity.
///
/// Not a slab index, so the 1-based `NonZeroUsize` ID convention does not apply.
/// It stands in for Flutter's `Route` object identity: the reference passes
/// `Route` objects to observers and to `didChangeNext`/`didChangePrevious`, and
/// compares them with `==`. Passing ids keeps this layer pure data — see
/// `history.rs`' divergence note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RouteId(u64);

impl RouteId {
    /// Mint the next id.
    ///
    /// `pub(crate)` because `NavigatorHandle::push_bound` needs the
    /// id **before** the route is boxed, so it can hand the route a
    /// `RouteBinding` pre-bound to it.
    pub(crate) fn next() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// The raw identifier. Stable for the route's lifetime, never reused.
    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }
}

/// A [`RouteSettings::arguments`] payload, type-erased like Flutter's
/// `Object? arguments`.
///
/// `Arc`-shared for the same reason as `flui-objects`'
/// `MetaDataPayload` (`interaction/meta_data.rs`): cloning a route's
/// settings must not deep-copy the payload, and that boundary is the
/// established precedent for a type-erased user value crossing FLUI's
/// public surface. This is exactly the shape ADR-0024 §4.1 named for this
/// field.
pub type RouteArguments = Arc<dyn Any + Send + Sync>;

/// Flutter's `RouteSettings` (`navigator.dart:670-687`).
#[derive(Default, Clone)]
pub struct RouteSettings {
    name: Option<String>,
    arguments: Option<RouteArguments>,
}

impl RouteSettings {
    /// Settings carrying a route name — Flutter's `RouteSettings(name:)`.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            arguments: None,
        }
    }

    /// Builder: attach an arguments payload — Flutter's
    /// `RouteSettings(arguments:)`. Used when building the route, e.g. from
    /// `Navigator.onGenerateRoute`.
    #[must_use]
    pub fn with_arguments<T: Any + Send + Sync + 'static>(mut self, value: T) -> Self {
        self.arguments = Some(Arc::new(value));
        self
    }

    /// The route's name, if it has one.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The route's arguments payload, if it has one — Flutter's
    /// `RouteSettings.arguments`.
    #[must_use]
    pub fn arguments(&self) -> Option<&RouteArguments> {
        self.arguments.as_ref()
    }

    /// Attempts to downcast the arguments payload to the requested concrete
    /// type. Returns `None` if there is no payload or its type doesn't match.
    ///
    /// Named `argument`, singular, per ADR-0024 §4.1's `settings.argument::<T>()`.
    /// The typed counterpart to [`arguments`](Self::arguments) — the same role
    /// `RenderMetaData::metadata_as` plays for its own erased payload.
    #[must_use]
    pub fn argument<T: Any + Send + Sync + 'static>(&self) -> Option<&T> {
        self.arguments.as_ref()?.downcast_ref::<T>() // PORT-CHECK-OK-DOWNCAST: RouteSettings.arguments erasure per ADR-0024 §4.1; Gate sign-off still outstanding, see ADR-0024 §6
    }
}

// `dyn Any` implements neither `PartialEq` nor `Eq`, so `arguments` can't
// ride the derive. `name` compares by value; `arguments` by pointer identity
// — the same notion of equality Flutter's `same(arguments)` oracle assertion
// uses, since `RouteSettings` has no `==` override and Dart's default
// identity equality is what "the exact object" means there.
impl PartialEq for RouteSettings {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && match (&self.arguments, &other.arguments) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
}

impl Eq for RouteSettings {}

// `dyn Any` has no `Debug` bound, so the payload prints as presence only —
// the same choice `RenderMetaData`'s manual `Debug` makes for its erased
// metadata field.
impl fmt::Debug for RouteSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteSettings")
            .field("name", &self.name)
            .field("has_arguments", &self.arguments.is_some())
            .finish()
    }
}

/// What a back-gesture / `maybe_pop` should do. Flutter's `RoutePopDisposition`
/// (`navigator.dart:117-136`).
///
/// `DoNotPop`'s producer is [`Route::vetoes_pop`] — a mounted
/// [`PopScope`](super::pop_scope::PopScope) with `can_pop = false`
/// (2026-07-10; page-based `canPop` remains deferred with Navigator 2.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoutePopDisposition {
    /// Pop the route.
    Pop,
    /// Refuse, and tell the route (`on_pop_invoked(false)`).
    DoNotPop,
    /// Not ours to handle — let an ancestor, or the system, deal with it.
    Bubble,
}

/// What [`Route::did_push`] reports about its entrance transition.
///
/// Flutter's `didPush()` returns a `TickerFuture`, and `handlePush` parks the
/// entry in `pushing` until it resolves (`navigator.dart:3273-3290`). FLUI has no
/// animation at this layer, so the route says which of the two it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushCompletion {
    /// The route is fully pushed already. The entry settles to `Idle` inside the
    /// same flush.
    ///
    /// **Documented divergence.** Flutter parks even a zero-duration push in
    /// `pushing` and flips it to `idle` on a *microtask*, forcing a second flush
    /// (`:3276-3290`). FLUI settles it in the first flush. The end state and the
    /// entire observer stream are identical — a replaced route emits no removal
    /// observation (`_reportRemovalToObserver == false`), and removal
    /// observations are enqueued during the first flush either way. Only the
    /// *dispose* of a route sitting in `Removing` moves one flush earlier.
    Immediate,
    /// The route is animating in. The entry parks in `Pushing` until
    /// `RouteHistory::notify_push_completed` fires — the `TransitionRoute` seam,
    /// and the analogue of Flutter's `whenCompleteOrCancel`.
    Animating,
}

/// A route: something the navigator can push, show and pop with a result.
///
/// Flutter's `Route<T>` (`navigator.dart:161`), minus the framework-owned state
/// (see the module docs) and minus everything the `OverlayRoute` /
/// `TransitionRoute` / `ModalRoute` layers add. This trait implements the
/// floor: no overlay entries, no animation, no barrier.
///
/// Every hook has a default, so a test route is a struct with one line.
#[allow(unused_variables)]
pub trait Route: 'static {
    /// The value a `pop` of this route delivers.
    type Output: Send + 'static;

    /// This route's settings — Flutter's `Route.settings`.
    fn settings(&self) -> &RouteSettings;

    /// The fallback used when a pop supplies no result — Flutter's
    /// `currentResult` (`navigator.dart:426`), the `??` in
    /// `_popCompleter.complete(result ?? currentResult)`.
    fn current_result(&mut self) -> Option<Self::Output> {
        None
    }

    /// Whether a successful `did_pop` finalizes the route immediately.
    ///
    /// Flutter's `OverlayRoute.finishedWhenPopped` (`routes.dart:84`), default
    /// `true`; `TransitionRoute` overrides it to `controller.isDismissed`
    /// (`routes.dart:178`), which is what defers removal until the exit
    /// animation ends.
    fn finished_when_popped(&self) -> bool {
        true
    }

    /// Whether this route pops something of its own instead of leaving the
    /// navigator. Flutter's `Route.willHandlePopInternally` (`navigator.dart:566`),
    /// default `false`; `LocalHistoryRoute` overrides it (`routes.dart:970`).
    ///
    /// Read by `can_pop`, where it lets the *bottom-most* route claim a pop that
    /// would otherwise be refused.
    fn will_handle_pop_internally(&self) -> bool {
        false
    }

    /// Whether this route currently vetoes being popped by `maybe_pop` /
    /// back-navigation — the `PopEntry` half of `ModalRoute.popDisposition`
    /// (`routes.dart:2033-2042`): any registered [`PopScope`] with
    /// `can_pop = false`. The `isFirst ? bubble : pop` base stays in the
    /// history, which owns the stack shape. A veto does **not** block a
    /// programmatic `pop()`, exactly as in Flutter.
    ///
    /// [`PopScope`]: super::pop_scope::PopScope
    fn vetoes_pop(&self) -> bool {
        false
    }

    /// Flutter's `Route.install()` (`navigator.dart:257`) — an empty default
    /// there too. `OverlayRoute` overrides it to create overlay entries.
    fn install(&mut self) {}

    /// Flutter's `Route.didPush()` (`:270`).
    fn did_push(&mut self) -> PushCompletion {
        PushCompletion::Immediate
    }

    /// Flutter's `Route.didAdd()`.
    fn did_add(&mut self) {}

    /// Flutter's `Route.didReplace(oldRoute)`.
    fn did_replace(&mut self, previous: Option<RouteId>) {}

    /// Whether this route consents to being popped.
    ///
    /// This is the **return value** of Flutter's `Route.didPop(result)`
    /// (`navigator.dart:458`); the result delivery half lives in
    /// the framework's route record, which calls `did_complete` when this returns
    /// `true`. Returning `false` refuses the pop and the entry returns to `Idle`
    /// — `LocalHistoryRoute.didPop` (`routes.dart:950`) is the reference user.
    fn did_pop(&mut self) -> bool {
        true
    }

    /// Flutter's `Route.didComplete(result)` (`:480`), *observation only*: the
    /// completer is completed by the framework's route record immediately afterwards.
    fn did_complete(&mut self, result: Option<&Self::Output>) {}

    /// Flutter's `Route.didPopNext(nextRoute)`.
    fn did_pop_next(&mut self, popped: RouteId) {}

    /// Flutter's `Route.didChangeNext(nextRoute)`.
    fn did_change_next(&mut self, next: Option<RouteId>) {}

    /// Flutter's `Route.didChangePrevious(previousRoute)`.
    fn did_change_previous(&mut self, previous: Option<RouteId>) {}

    /// Flutter's `Route.onPopInvokedWithResult(didPop, result)` (`:410`), called
    /// with `true` after a real pop.
    /// # Runs inside the flush, under the navigator's history lock
    ///
    /// Every `Route` lifecycle hook — this one included — is called while the
    /// navigator holds its (non-reentrant) history mutex. **Calling back into
    /// `NavigatorHandle` from here deadlocks the same thread**, including a
    /// pure read such as `can_pop()`. Record what you need and act on it later
    /// (a `PopScope`'s `on_pop_invoked` is delivered *outside* the lock for
    /// exactly this reason, and is the hook user code should prefer).
    fn on_pop_invoked(&mut self, did_pop: bool) {}

    /// Flutter's `Route.dispose()` (`:574`).
    fn dispose(&mut self) {}
}

/// The framework's view of a route, with `Output` erased.
///
/// Object-safe by construction: no associated type, no generics, and the only
/// value crossing the boundary is an [`AnyResult`].
pub(crate) trait ErasedRoute {
    fn id(&self) -> RouteId;

    /// Flutter's `Route._installed` (`navigator.dart:180`). Read by
    /// `handle_removal`, which disposes a never-installed route outright.
    fn is_installed(&self) -> bool;

    /// Flutter's `route._popCompleter.isCompleted` (`:3361`).
    fn is_completed(&self) -> bool;

    fn finished_when_popped(&self) -> bool;
    fn will_handle_pop_internally(&self) -> bool;
    fn vetoes_pop(&self) -> bool;

    fn install(&mut self);
    fn did_push(&mut self) -> PushCompletion;
    fn did_add(&mut self);
    fn did_replace(&mut self, previous: Option<RouteId>);

    /// `Route.didPop(result)`: completes the future and returns `true`, or
    /// refuses and returns `false` without completing.
    fn did_pop(&mut self, result: Option<AnyResult>) -> bool;

    /// `Route.didComplete(result)`: completes the future with
    /// `result ?? current_result`. Idempotent.
    fn did_complete(&mut self, result: Option<AnyResult>);

    fn did_pop_next(&mut self, popped: RouteId);
    fn did_change_next(&mut self, next: Option<RouteId>);
    fn did_change_previous(&mut self, previous: Option<RouteId>);
    fn on_pop_invoked(&mut self, did_pop: bool);
    fn dispose(&mut self);
}

/// A typed [`Route`] plus the state Flutter keeps on its `Route` base class.
pub(crate) struct RouteRecord<R: Route> {
    id: RouteId,
    route: R,
    completer: Completer<R::Output>,
    installed: bool,
}

impl<R: Route> RouteRecord<R> {
    /// Box `route` for the stack, and hand back the future its push returns.
    ///
    /// The future exists **before any lifecycle runs**, exactly as
    /// `Navigator.push` returns `route.popped` before `_flushHistoryUpdates`
    /// (`navigator.dart:5060-5063`).
    #[cfg(test)]
    pub(crate) fn erase(route: R) -> (Box<dyn ErasedRoute>, RouteResult<R::Output>) {
        Self::erase_with_id(RouteId::next(), route)
    }

    /// Box `route` under an id minted by the caller.
    ///
    /// `NavigatorHandle::push_bound` mints first so it can bind the route to its
    /// own id before boxing.
    pub(crate) fn erase_with_id(
        id: RouteId,
        route: R,
    ) -> (Box<dyn ErasedRoute>, RouteResult<R::Output>) {
        let (completer, result) = Completer::new();
        let record = Self {
            id,
            route,
            completer,
            installed: false,
        };
        (Box::new(record), result)
    }
}

impl<R: Route> ErasedRoute for RouteRecord<R> {
    fn id(&self) -> RouteId {
        self.id
    }

    fn is_installed(&self) -> bool {
        self.installed
    }

    fn is_completed(&self) -> bool {
        self.completer.is_completed()
    }

    fn finished_when_popped(&self) -> bool {
        self.route.finished_when_popped()
    }

    fn will_handle_pop_internally(&self) -> bool {
        self.route.will_handle_pop_internally()
    }

    fn vetoes_pop(&self) -> bool {
        self.route.vetoes_pop()
    }

    fn install(&mut self) {
        debug_assert!(
            !self.installed,
            "BUG: a route was installed twice — Flutter asserts \
             'The pushed route has already been used' (navigator.dart:3265)"
        );
        self.installed = true;
        self.route.install();
    }

    fn did_push(&mut self) -> PushCompletion {
        self.route.did_push()
    }

    fn did_add(&mut self) {
        self.route.did_add();
    }

    fn did_replace(&mut self, previous: Option<RouteId>) {
        self.route.did_replace(previous);
    }

    fn did_pop(&mut self, result: Option<AnyResult>) -> bool {
        if !self.route.did_pop() {
            return false;
        }
        self.did_complete(result);
        true
    }

    fn did_complete(&mut self, result: Option<AnyResult>) {
        if self.completer.is_completed() {
            return;
        }

        // `result ?? currentResult` (navigator.dart:481). The fallback applies
        // only when no result was supplied — a *mismatched* result is an error,
        // not an absent one, so it must not silently fall back.
        let value = match result {
            None => self.route.current_result(),
            Some(erased) => {
                // The pop-result type-erasure boundary. A heterogeneous route stack
                // cannot carry each route's `Output`, so `pop` erases and the owning
                // record downcasts back. Signed off as the only downcast in
                // `flui-widgets`, and port-check's FR-033/widgets grep keeps it that way.
                let typed = erased.downcast::<R::Output>(); // PORT-CHECK-OK-DOWNCAST: signed-off pop-result erasure boundary, see module docs
                if let Ok(value) = typed {
                    Some(*value)
                } else {
                    tracing::error!(
                        route = self.id.get(),
                        expected = std::any::type_name::<R::Output>(),
                        "pop result has the wrong type for this route; completing with None. \
                         Flutter throws a cast error here"
                    );
                    None
                }
            }
        };

        self.route.did_complete(value.as_ref());
        self.completer.complete(value);
    }

    fn did_pop_next(&mut self, popped: RouteId) {
        self.route.did_pop_next(popped);
    }

    fn did_change_next(&mut self, next: Option<RouteId>) {
        self.route.did_change_next(next);
    }

    fn did_change_previous(&mut self, previous: Option<RouteId>) {
        self.route.did_change_previous(previous);
    }

    fn on_pop_invoked(&mut self, did_pop: bool) {
        self.route.on_pop_invoked(did_pop);
    }

    fn dispose(&mut self) {
        self.route.dispose();
    }
}
