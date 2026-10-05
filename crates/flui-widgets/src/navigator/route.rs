//! [`Route`] — the typed route trait — and [`ErasedRoute`], the type-erased view
//! of it that a heterogeneous route stack can hold.
//!
//! Private; nothing here is exported.
//!
//! # Two halves of a route
//!
//! | Part | Owner |
//! |---|---|
//! | the lifecycle hooks | [`Route`] — user-implemented, typed |
//! | the pop completer, the `popped` future, the installed flag | [`RouteRecord`] — framework-owned |
//!
//! The split is forced. The pop completer must be completed by machinery that only
//! sees `dyn ErasedRoute`, and a default method on a trait cannot own state.
//! `did_pop` completes the future, and `did_complete` applies the
//! `result ?? current_result` fallback.
//!
//! # The type-erasure boundary — **private, unauthorized**
//!
//! `Vec<Box<dyn Route<Output = T>>>` cannot hold routes with different `T`, so
//! the stack holds `Box<dyn ErasedRoute>` and a pop result crosses a
//! `Box<dyn Any + Send>` boundary, downcast in [`RouteRecord::did_complete`].
//! A mismatched `pop<T>` is therefore a runtime failure rather than a compile error.
//!
//! **This does not authorize the public shape.** A later API sign-off gate still
//! owns that decision, and the erasure is confined to this private module until then.
//! Note also that no gate checks `dyn` boundaries or downcasts in
//! `flui-widgets`, so **nothing would have caught this** — which is a reason
//! to keep it private, not a licence to export.
//!
//! On a type mismatch the framework logs and completes with `None` rather than
//! raising a cast error. A wrong `pop` type is caller error, and
//! [`PANIC-POLICY`](../../../../../docs/PANIC-POLICY.md) reserves panics for
//! framework invariants. Pinned by
//! `a_mismatched_pop_result_is_reported_and_dropped_outside_the_history_lock`.

use std::any::Any;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use flui_scheduler::TickerFuture;

use super::result::{Completer, RouteResult};

/// A pop result, erased — and carrying the name of what was erased.
///
/// See the module docs for the erasure itself. The `type_name` rides along
/// because it cannot be recovered afterwards: `dyn Any` yields a `TypeId`, never a
/// name, so a value that reaches no route could otherwise only be reported as
/// "something was discarded". It is captured once, at the six public erasure
/// sites, where the caller's `T` is still known.
pub(crate) struct AnyResult {
    value: Box<dyn Any + Send>,
    supplied: &'static str,
}

impl AnyResult {
    /// Erase a caller-supplied result, recording what it was.
    pub(crate) fn new<T: Send + 'static>(value: T) -> Self {
        Self {
            value: Box::new(value),
            supplied: std::any::type_name::<T>(),
        }
    }

    /// `type_name` of the value the caller supplied.
    pub(crate) fn supplied(&self) -> &'static str {
        self.supplied
    }

    /// Recover the concrete value, or hand this back unchanged — so a failed
    /// downcast does not lose the provenance the failure report needs.
    pub(crate) fn downcast<T: 'static>(self) -> Result<T, Self> {
        let recovered = self.value.downcast::<T>(); // the reverse of `AnyResult::new`'s own erasure, at the signed-off pop-result boundary (ADR-0019); hands the value back on failure rather than losing its provenance
        match recovered {
            Ok(value) => Ok(*value),
            Err(value) => Err(Self {
                value,
                supplied: self.supplied,
            }),
        }
    }
}

impl fmt::Debug for AnyResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnyResult")
            .field("supplied", &self.supplied)
            .finish_non_exhaustive()
    }
}

/// A caller-supplied result that reached no route, or reached one that could not
/// take it — carried out of the history's locked section and reported there.
///
/// **Why this is a value and not a `tracing` call at the site.** An `AnyResult`
/// wraps a value the *caller* supplied, so dropping it runs user `Drop`; and a
/// `tracing` subscriber is user code too. Both may reach back into the navigator,
/// and the history mutex is not reentrant. The same rule the registry follows for
/// a displaced factory closure, and the same rule `FlushOutcome::deferred`
/// follows for a lifecycle callback.
///
/// One channel with a reason, rather than one vector per kind — the argument
/// `FlushOutcome::deferred` already makes for itself.
pub(crate) enum UndeliveredResult {
    /// No present route to deliver to: an empty stack, one whose top is
    /// mid-exit-transition, or a removal target that had already completed.
    NoTarget {
        /// The value itself, so it drops outside the lock.
        value: AnyResult,
        /// `type_name` of what the caller supplied. In scope at every erasure
        /// site, and the only thing that lets a reader of the log tell *which*
        /// value was lost — its sibling below has always carried type
        /// information and this had none at all.
        supplied: &'static str,
    },
    /// A route received it, and its type did not match that route's `Output`.
    /// The route is logged and completes with `None`; nothing panics.
    TypeMismatch {
        /// The route that could not take it.
        route: RouteId,
        /// `type_name` of the `Output` that route delivers.
        expected: &'static str,
        /// The value itself, so it drops outside the lock.
        value: AnyResult,
    },
}
// Deliberately **not** public. `NavigatorHandle::pop_with<T>` takes a typed `T`
// and erases it here, so the erasure is an implementation detail rather than a
// shape callers must name. The boundary was signed off, not its exposure.

/// Process-unique route identity.
///
/// Not a slab index, so the 1-based `NonZeroUsize` ID convention does not apply.
/// It stands in for route object identity: observers and
/// `did_change_next`/`did_change_previous` receive ids rather than the routes
/// themselves, which keeps this layer pure data — see `history.rs`' note.
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
        Self::next_from(&COUNTER)
    }

    pub(super) fn next_from(counter: &AtomicU64) -> Self {
        // MAX is a permanent exhausted sentinel. Admitted identities are
        // 1..MAX; refusal never advances the counter back into that range.
        let raw = counter
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                if next == 0 { None } else { next.checked_add(1) }
            })
            .unwrap_or_else(|_| panic!("BUG: route identity capacity exhausted"));
        Self(raw)
    }

    /// The raw identifier. Stable for the route's lifetime, never reused.
    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }
}

/// A [`RouteSettings::arguments`] payload, type-erased.
///
/// `Arc`-shared for the same reason as `flui-objects`'
/// `MetaDataPayload` (`interaction/meta_data.rs`): cloning a route's
/// settings must not deep-copy the payload, and that boundary is the
/// established precedent for a type-erased user value crossing FLUI's
/// public surface. This is exactly the shape ADR-0024 named for this
/// field.
pub type RouteArguments = Arc<dyn Any + Send + Sync>;

/// A route's name and arguments payload.
#[derive(Default, Clone)]
pub struct RouteSettings {
    name: Option<String>,
    arguments: Option<RouteArguments>,
}

impl RouteSettings {
    /// Settings carrying a route name.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            arguments: None,
        }
    }

    /// Builder: attach an arguments payload. Used when building the route, e.g.
    /// from a route generator.
    ///
    /// This **allocates a fresh [`Arc`]**, so the payload it attaches is a new
    /// object even when `value` was cloned out of another `RouteSettings`. Since
    /// [`RouteSettings`] compares its payload by pointer identity (see the
    /// `PartialEq` impl below), relaying a payload you already hold must go through
    /// [`with_arguments_shared`](Self::with_arguments_shared) instead — this
    /// method would silently change its identity.
    #[must_use]
    pub fn with_arguments<T: Any + Send + Sync + 'static>(mut self, value: T) -> Self {
        self.arguments = Some(Arc::new(value));
        self
    }

    /// Builder: attach an arguments payload **the caller already holds**,
    /// forwarding it without re-wrapping.
    ///
    /// The identity-preserving counterpart to
    /// [`with_arguments`](Self::with_arguments), for the relay case: reading
    /// [`arguments`](Self::arguments) off one settings object and putting it on
    /// another. `with_arguments` would re-wrap — it takes the payload by value
    /// and mints a new `Arc` — breaking the pointer identity that both this
    /// type's `PartialEq` defines equality by.
    ///
    /// It is not the *only* way to relay a payload: the `Arc` is reachable
    /// through [`arguments`](Self::arguments) and can be carried by hand. What
    /// this buys is that the identity-preserving form is a builder call like any
    /// other, so the safe spelling is no longer than the unsafe one.
    #[must_use]
    pub fn with_arguments_shared(mut self, arguments: RouteArguments) -> Self {
        self.arguments = Some(arguments);
        self
    }

    /// The route's name, if it has one.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The route's arguments payload, if it has one.
    #[must_use]
    pub fn arguments(&self) -> Option<&RouteArguments> {
        self.arguments.as_ref()
    }

    /// Attempts to downcast the arguments payload to the requested concrete
    /// type. Returns `None` if there is no payload or its type doesn't match.
    ///
    /// Named `argument`, singular, per ADR-0024's `settings.argument::<T>()`.
    /// The typed counterpart to [`arguments`](Self::arguments) — the same role
    /// `RenderMetaData::metadata_as` plays for its own erased payload.
    #[must_use]
    pub fn argument<T: Any + Send + Sync + 'static>(&self) -> Option<&T> {
        self.arguments.as_ref()?.downcast_ref::<T>() // RouteSettings.arguments erasure per ADR-0024
    }
}

// A bare name is the overwhelmingly common named-route request, so
// `handle.push_named("/details")` should not make the caller name
// `RouteSettings` at all — the argument-carrying form
// (`RouteSettings::named("/details").with_arguments(id)`) is the exception, and
// reads as one. This is what makes the `impl Into<RouteSettings>` request
// object on every `*_named` entry point ergonomic.
impl From<&str> for RouteSettings {
    fn from(name: &str) -> Self {
        Self::named(name)
    }
}

impl From<String> for RouteSettings {
    fn from(name: String) -> Self {
        Self::named(name)
    }
}

// `dyn Any` implements neither `PartialEq` nor `Eq`, so `arguments` can't
// ride the derive. `name` compares by value; `arguments` by pointer identity
// — "the exact object" is what equality of a payload means here.
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

/// What a back-gesture / `maybe_pop` should do.
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
/// An animating push hands the navigator a `TickerFuture`, and the entry parks in
/// `Pushing` until it resolves. `AnimationController`'s run-starting methods
/// return that kind of future (ADR-0064), so a route passes theirs straight on.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum PushCompletion {
    /// The route is fully pushed already. The entry settles to `Idle` inside the
    /// same flush.
    ///
    /// Even a zero-duration push settles in the first flush; it is never parked in
    /// `Pushing` awaiting a microtask that would force a second flush. A replaced
    /// route emits no removal observation, and removal observations are enqueued
    /// during the first flush either way. Only the *dispose* of a route sitting in
    /// `Removing` lands one flush earlier than a deferred settle would put it.
    Immediate,
    /// The route is animating in. The entry parks in `Pushing` until `future`
    /// resolves — awaited from the navigator's own apply step, once the flush
    /// that installed this entry has released the history lock, never from
    /// inside that flush itself (ADR-0064's constraint for this consumer:
    /// registering mid-flush would let an already-resolved future settle in
    /// the very flush it was returned from). Settles the entry the same way
    /// whether the run completes or is canceled — there is no separate
    /// "canceled" outcome at this layer.
    Animating(TickerFuture),
}

/// A route: something the navigator can push, show and pop with a result.
///
/// The framework-owned state lives elsewhere (see the module docs), and
/// everything the overlay / transition / modal layers add is built on top. This
/// trait is the floor: no overlay entries, no animation, no barrier.
///
/// Every hook has a default, so a test route is a struct with one line.
#[expect(unused_variables)]
pub trait Route: 'static {
    /// The value a `pop` of this route delivers.
    type Output: Send + 'static;

    /// This route's settings.
    fn settings(&self) -> &RouteSettings;

    /// The fallback used when a pop supplies no result: the `??` in
    /// `result ?? current_result`.
    fn current_result(&mut self) -> Option<Self::Output> {
        None
    }

    /// Whether a successful `did_pop` finalizes the route immediately.
    ///
    /// Defaults to `true`; a transition route overrides it to "the controller is
    /// dismissed", which is what defers removal until the exit animation ends.
    fn finished_when_popped(&self) -> bool {
        true
    }

    /// Whether this route pops something of its own instead of leaving the
    /// navigator. Defaults to `false`; a route with local history entries
    /// overrides it.
    ///
    /// Read by `can_pop`, where it lets the *bottom-most* route claim a pop that
    /// would otherwise be refused.
    fn will_handle_pop_internally(&self) -> bool {
        false
    }

    /// Whether this route currently vetoes being popped by `maybe_pop` /
    /// back-navigation: any registered [`PopScope`] with
    /// `can_pop = false`. The "first route bubbles, otherwise pop" base stays in the
    /// history, which owns the stack shape. A veto does **not** block a
    /// programmatic `pop()`.
    ///
    /// [`PopScope`]: super::pop_scope::PopScope
    fn vetoes_pop(&self) -> bool {
        false
    }

    /// The route was inserted into the navigator; the default does nothing.
    fn install(&mut self) {}

    /// The route was pushed. Reports how its entrance transition proceeds.
    fn did_push(&mut self) -> PushCompletion {
        PushCompletion::Immediate
    }

    /// The route was added as part of an initial or declarative stack.
    fn did_add(&mut self) {}

    /// The route replaced `previous`.
    fn did_replace(&mut self, previous: Option<RouteId>) {}

    /// Whether this route consents to being popped.
    ///
    /// The result delivery half lives in
    /// the framework's route record, which calls `did_complete` when this returns
    /// `true`. Returning `false` refuses the pop and the entry returns to `Idle`,
    /// as a route with pending local history entries does.
    fn did_pop(&mut self) -> bool {
        true
    }

    /// The route completed with `result`. *Observation only*: the
    /// completer is completed by the framework's route record immediately afterwards.
    fn did_complete(&mut self, result: Option<&Self::Output>) {}

    /// The route above this one was popped.
    fn did_pop_next(&mut self, popped: RouteId) {}

    /// The route above this one changed.
    fn did_change_next(&mut self, next: Option<RouteId>) {}

    /// The route below this one changed.
    fn did_change_previous(&mut self, previous: Option<RouteId>) {}

    /// A pop was attempted; called with `true` after a real pop.
    ///
    /// # Runs inside the flush, under the navigator's history lock
    ///
    /// Every `Route` lifecycle hook — this one included — is called while the
    /// navigator holds its (non-reentrant) history mutex. **Calling back into
    /// `NavigatorHandle` from here deadlocks the same thread**, including a
    /// pure read such as `can_pop()`. Record what you need and act on it later
    /// (a `PopScope`'s `on_pop_invoked` is delivered *outside* the lock for
    /// exactly this reason, and is the hook user code should prefer).
    fn on_pop_invoked(&mut self, did_pop: bool) {}

    /// The route left the stack for good; release its resources.
    fn dispose(&mut self) {}
}

impl UndeliveredResult {
    /// A result that reached no route at all.
    pub(crate) fn no_target(value: AnyResult) -> Self {
        Self::NoTarget {
            supplied: value.supplied(),
            value,
        }
    }
}

/// The framework's view of a route, with `Output` erased.
///
/// Object-safe by construction: no associated type, no generics, and the only
/// value crossing the boundary is an [`AnyResult`].
pub(crate) trait ErasedRoute {
    fn id(&self) -> RouteId;

    /// Whether `install` has run. Read by `handle_removal`, which disposes a
    /// never-installed route outright.
    fn is_installed(&self) -> bool;

    /// Whether the pop completer has fired.
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
    ///
    /// Any mismatched result comes back rather than being reported here — see
    /// [`UndeliveredResult`].
    fn did_pop(&mut self, result: Option<AnyResult>) -> (bool, Option<UndeliveredResult>);

    /// `Route.didComplete(result)`: completes the future with
    /// `result ?? current_result`. Idempotent.
    ///
    /// Any mismatched result comes back rather than being reported here — see
    /// [`UndeliveredResult`].
    fn did_complete(&mut self, result: Option<AnyResult>) -> Option<UndeliveredResult>;

    fn did_pop_next(&mut self, popped: RouteId);
    fn did_change_next(&mut self, next: Option<RouteId>);
    fn did_change_previous(&mut self, previous: Option<RouteId>);
    fn on_pop_invoked(&mut self, did_pop: bool);
    fn dispose(&mut self);
}

/// A typed [`Route`] plus the framework-owned state: id, pop completer, installed flag.
pub(crate) struct RouteRecord<R: Route> {
    id: RouteId,
    route: super::lifecycle::Terminal<R>,
    completer: super::lifecycle::Terminal<Completer<R::Output>>,
    installed: bool,
}

impl<R: Route> Drop for RouteRecord<R> {
    fn drop(&mut self) {
        let route = self.route.withdraw();
        let completer = self.completer.withdraw();
        drop(route);
        drop(completer);
    }
}

impl<R: Route> RouteRecord<R> {
    /// Box `route` for the stack, and hand back the future its push returns.
    ///
    /// The future exists **before any lifecycle runs**, so a push can return it
    /// before the history flush.
    #[cfg(test)]
    pub(crate) fn erase(route: R) -> (Box<dyn ErasedRoute>, RouteResult<R::Output>) {
        let mut route = super::lifecycle::Terminal::new(route);
        let id = RouteId::next();
        Self::erase_with_id(id, route.take_value())
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
            route: super::lifecycle::Terminal::new(route),
            completer: super::lifecycle::Terminal::new(completer),
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
            "BUG: a route was installed twice; a route object is single-use"
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

    fn did_pop(&mut self, result: Option<AnyResult>) -> (bool, Option<UndeliveredResult>) {
        if !self.route.did_pop() {
            // Refused. The result was never delivered, and it is the caller's
            // value, so it goes back out rather than being dropped under the lock.
            return (false, result.map(UndeliveredResult::no_target));
        }
        (true, self.did_complete(result))
    }

    fn did_complete(&mut self, result: Option<AnyResult>) -> Option<UndeliveredResult> {
        if self.completer.is_completed() {
            return result.map(UndeliveredResult::no_target);
        }

        // `result ?? current_result`. The fallback applies
        // only when no result was supplied — a *mismatched* result is an error,
        // not an absent one, so it must not silently fall back.
        let mut undelivered = None;
        let value = match result {
            None => self.route.current_result(),
            Some(erased) => {
                // The pop-result type-erasure boundary. A heterogeneous route stack
                // cannot carry each route's `Output`, so `pop` erases and the owning
                // record downcasts back. Signed off as the only downcast in
                // `flui-widgets`.
                let typed = erased.downcast::<R::Output>(); // the pop-result erasure boundary itself — a heterogeneous route stack cannot carry each route's `Output`, so `pop` erases and the owning `RouteRecord` recovers its own type here; signed off in ADR-0019's *Public API and sign-off* section
                match typed {
                    Ok(value) => Some(value),
                    Err(mismatched) => {
                        // Neither reported nor dropped here: this runs inside the
                        // flush, under the history mutex, and both a `tracing`
                        // subscriber and the value's own `Drop` are user code that
                        // may re-enter the navigator.
                        undelivered = Some(UndeliveredResult::TypeMismatch {
                            route: self.id,
                            expected: std::any::type_name::<R::Output>(),
                            value: mismatched,
                        });
                        None
                    }
                }
            }
        };

        self.route.did_complete(value.as_ref());
        self.completer.complete(value);
        undelivered
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

#[cfg(test)]
pub(super) fn route_identity_exhaustion_preserves_history_authority() {
    use super::history::RouteHistory;
    use super::overlay_route::SimpleRoute;
    use flui_view::ViewExt;
    let counter = AtomicU64::new(1);
    let older = RouteId::next_from(&counter);
    counter.store(u64::MAX - 1, Ordering::Relaxed);
    let last = RouteId::next_from(&counter);
    assert_eq!(last.get(), u64::MAX - 1);
    let mut history = RouteHistory::new();
    let older_result = history.seed_initial_with_id(
        older,
        SimpleRoute::<()>::new(|_| crate::Text::new("older").boxed()),
    );
    let last_result = history.seed_initial_with_id(
        last,
        SimpleRoute::<()>::new(|_| crate::Text::new("last").boxed()),
    );
    for _ in 0..3 {
        let failure = std::panic::catch_unwind(|| RouteId::next_from(&counter))
            .expect_err("exhausted route identity remains refused");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.as_ref()),
            Some("BUG: route identity capacity exhausted")
        );
        assert_eq!(history.ids(), vec![older, last]);
    }
    assert!(history.remove_route(older, None));
    assert_eq!(older_result.try_take(), Some(None));
    assert_eq!(history.current(), Some(last));
    assert_eq!(
        last_result.try_take(),
        None,
        "removing the older identity cannot complete its sibling"
    );
    assert!(!history.remove_route(older, None));
    assert_eq!(history.current(), Some(last));
    let independent = AtomicU64::new(1);
    let mut recovered = RouteHistory::new();
    let id = RouteId::next_from(&independent);
    recovered.seed_initial_with_id(
        id,
        SimpleRoute::<()>::new(|_| crate::Text::new("recovery").boxed()),
    );
    assert!(recovered.remove_route(id, None));
    assert_eq!(recovered.current(), None);
}
