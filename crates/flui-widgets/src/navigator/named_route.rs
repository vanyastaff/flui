//! Named-route generation: [`GeneratedRoute`], [`NamedRouteError`], and the
//! private registry that [`NavigatorHandle`]'s `*_named` entry points resolve
//! through.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/navigator.dart` at tag `3.44.0`,
//! **by symbol** (this ADR's original line citations had all moved by the time
//! its gate ran): `RouteFactory` (the `Route<dynamic>? Function(RouteSettings)`
//! typedef), `Navigator.onGenerateRoute` / `Navigator.onUnknownRoute`, and
//! `NavigatorState._routeNamed`, which builds the settings, calls the generator,
//! and falls back to `onUnknownRoute`. The `routes: Map<String, WidgetBuilder>`
//! table is not on `Navigator` at all — `WidgetsApp._onGenerateRoute`
//! (`lib/src/widgets/app.dart`) folds `home`, `routes` and the user's
//! `onGenerateRoute` into the single hook the navigator reads. FLUI folds the
//! same three sources here, so one navigator has one resolution path.
//!
//! # What is erased, and what is not
//!
//! The route a factory returns stays **concrete**: `PageRoute<bool>` keeps
//! `Output = bool`, `Route::current_result` clones a `bool`, and
//! `NavigatorHandle::pop_with(true)` downcasts to `bool`. Nothing about the pop
//! path changes. What is erased is the *result handle* — a `RouteResult<T>`
//! travelling back out of a push whose route type the caller never named.
//!
//! Erasing the route's own `Output` instead was proposed, and does not compile:
//! every shipped route is bounded `T: Send + Clone + 'static` because
//! `Route::current_result` returns by value, and `Box<dyn Any + Send>` is not
//! `Clone`. It would also have silently destroyed every named route's pop
//! result, since `RouteRecord::did_complete` downcasts the payload to
//! `R::Output`. Both defects and the replacement are recorded in
//! [`ADR-0024`](../../../../../docs/adr/ADR-0024-named-routes-seam.md) §7.2.
//!
//! # Not implemented, and not claimed
//!
//! `Navigator.initialRoute` / `Navigator.defaultRouteName` /
//! `Navigator.defaultGenerateInitialRoutes` — the initial-route back-stack
//! synthesis. Deferred by decision (ADR-0024 U3, reaffirmed by its §7.1 gate);
//! FLUI bootstraps with `NavigatorHandle::seed_initial` meanwhile.
//!
//! It is **not** a deep-link-only feature, though U3 originally said so:
//! **any** initial name but `/` takes
//! `Navigator.defaultGenerateInitialRoutes`' expansion branch, which seeds `/`
//! first, so `initialRoute: "/settings"` is `["/", "/settings"]` — an ordinary
//! `MaterialApp` setting, and a two-deep stack whose back button returns home.
//! A one-deep stack would make back exit the app. The correction is recorded in
//! ADR-0024 §7.6.
//!
//! What the gap owes when it is built:
//!
//! - build the prefix chain for the requested name, seeding `/` first;
//! - filter out segments the registry does not resolve (Flutter's
//!   `result.removeWhere`), so an unmatched *middle* segment is a gap, not a
//!   failure;
//! - treat an unmatched **final** segment as an error: dispose every route
//!   generated for the attempt, and seed `/` alone;
//! - carry the two upstream cases that pin those last two apart —
//!   `'Initial route can have gaps'` and
//!   `'The full initial route has to be matched'`.
//!
//! Also absent: `restorablePushNamed` (restoration is unbuilt) and
//! `replaceNamed` (`replace` itself is private).

use std::any::{Any, TypeId, type_name};
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::rc::Rc;

use parking_lot::Mutex;

use super::navigator::NavigatorHandle;
use super::overlay_route::NavigatorRoute;
use super::result::RouteResult;
use super::route::{AnyResult, Route, RouteId, RouteSettings};

/// Builds the route a [`RouteRequest`] resolves to, or `None` to pass the
/// request on to the next resolution stage.
///
/// Flutter's `RouteFactory` typedef, plus the navigator — see [`RouteRequest`]
/// for why. `Rc`, not `Arc`: a route owns owner-local view builders
/// ([`RouteContentBuilder`](super::overlay_route::RouteContentBuilder) is
/// already `Rc`), and [`NavigatorHandle`] is `!Send + !Sync` by construction, so
/// a `Send + Sync` bound here would be unsatisfiable by the very routes the
/// factory exists to build.
pub(crate) type RouteFactory = Rc<dyn Fn(&RouteRequest<'_>) -> Option<GeneratedRoute>>;

/// What a route factory is asked: the [`RouteSettings`] of the request, and the
/// [`NavigatorHandle`] that is asking.
///
/// # Why the navigator is here rather than captured
///
/// Flutter's `RouteFactory` takes only settings, and a Dart factory that needs
/// to navigate closes over `Navigator.of(context)` freely — the cycle that
/// creates is collected. Rust does not collect cycles, and the registry is owned
/// by the navigator, so a factory that captured a [`NavigatorHandle`] would
/// close `Arc<NavigatorShared>` → registry → `Rc<dyn Fn>` → handle → back on
/// itself, and the navigator's storage would never be reclaimed — not even after
/// it unmounted. Passing the handle in removes the reason to capture one.
///
/// The route's *content* never needed a captured handle either:
/// [`RouteContentBuilder`](super::overlay_route::RouteContentBuilder) receives a
/// `&dyn BuildContext`, and `NavigatorHandle::maybe_of(ctx)` resolves the
/// navigator from it exactly as Flutter's `Navigator.of(context)` does. So after
/// this, capturing a handle in a factory has no remaining justification. If you
/// capture one anyway the cycle is still yours to break — nothing here can do it
/// for you.
///
/// # Re-entrancy is supported, not merely survivable
///
/// [`navigator`](Self::navigator) may be used to push, register, or query while
/// the factory runs: every factory is invoked with the registry lock released,
/// since `parking_lot::Mutex` is not reentrant. Pinned by
/// `a_factory_that_pushes_re_entrantly_does_not_deadlock`.
#[derive(Debug, Clone, Copy)]
pub struct RouteRequest<'a> {
    settings: &'a RouteSettings,
    navigator: &'a NavigatorHandle,
}

impl<'a> RouteRequest<'a> {
    /// The request, as a whole.
    ///
    /// Flutter's factories hand this straight to the route
    /// (`MaterialPageRoute(settings: settings)`); **FLUI's route builders take
    /// no settings**, so the analogous relay does not exist here. Read what you
    /// need — [`name`](Self::name), [`argument`](Self::argument) — and move it
    /// into the route's content builder, which is strictly better than Dart's
    /// ambient read-back. The gap and the one change that would make it wrong
    /// are recorded in `ARCHITECTURE.md`'s `## Mapping decisions`.
    #[must_use]
    pub fn settings(&self) -> &'a RouteSettings {
        self.settings
    }

    /// The requested route name, if the request carried one.
    #[must_use]
    pub fn name(&self) -> Option<&'a str> {
        self.settings.name()
    }

    /// The request's arguments payload, downcast — shorthand for
    /// [`RouteSettings::argument`].
    #[must_use]
    pub fn argument<T: Any + Send + Sync + 'static>(&self) -> Option<&'a T> {
        self.settings.argument::<T>()
    }

    /// The navigator resolving this request. Usable re-entrantly; see the type
    /// docs.
    #[must_use]
    pub fn navigator(&self) -> &'a NavigatorHandle {
        self.navigator
    }

    /// Assemble a request. `pub(super)` — only the resolving handle makes one.
    pub(super) fn new(settings: &'a RouteSettings, navigator: &'a NavigatorHandle) -> Self {
        Self {
            settings,
            navigator,
        }
    }
}

/// Which of [`NavigatorHandle`]'s typed front doors an erased push goes through.
///
/// Named routes share one history state machine with unnamed ones: every arm
/// here dispatches to the *existing* public push, so the observer stream a
/// `push_named` produces is the stream `push` produces.
pub(super) enum PushMode<'a> {
    /// [`NavigatorHandle::push`].
    Push,
    /// [`NavigatorHandle::push_replacement`], delivering `result` to the
    /// replaced route when present.
    Replace {
        /// What the replaced route's [`RouteResult`] resolves with.
        result: Option<AnyResult>,
    },
    /// [`NavigatorHandle::push_and_remove_until`].
    RemoveUntil {
        /// Stops the downward sweep at the first route it accepts.
        keep: &'a mut dyn FnMut(RouteId) -> bool,
    },
}

/// A [`NavigatorRoute`] whose push — or whose *disposal* — is callable without
/// naming its type.
///
/// Object-safe by construction: no associated type, no generics, and the only
/// value crossing the boundary is a `Box<dyn Any>` carrying one
/// [`RouteResult`].
trait ErasedPush {
    /// Push `self` through `mode`, returning the new route's id and its boxed
    /// `RouteResult<Self::Output>`.
    fn push_erased(
        self: Box<Self>,
        handle: &NavigatorHandle,
        mode: PushMode<'_>,
    ) -> (RouteId, Box<dyn Any>);

    /// Dispose a route that never reached a navigator.
    ///
    /// [`Route::dispose`] is an explicit method here, not `Drop` — so a
    /// generated route that is refused (wrong `T`) or simply dropped would
    /// otherwise never run it. Flutter has the same obligation and discharges it
    /// the same way: `Navigator.defaultGenerateInitialRoutes`' failure branch
    /// walks its partial result calling `route?.dispose()`.
    fn dispose_unpushed(self: Box<Self>);
}

impl<R: NavigatorRoute> ErasedPush for R {
    fn push_erased(
        self: Box<Self>,
        handle: &NavigatorHandle,
        mode: PushMode<'_>,
    ) -> (RouteId, Box<dyn Any>) {
        let (id, result): (RouteId, RouteResult<R::Output>) = match mode {
            PushMode::Push => handle.push_reporting_id(*self),
            PushMode::Replace { result } => {
                handle.push_replacement_erased_reporting_id(*self, result)
            }
            PushMode::RemoveUntil { keep } => {
                handle.push_and_remove_until_reporting_id(*self, keep)
            }
        };
        (id, Box::new(result))
    }

    fn dispose_unpushed(mut self: Box<Self>) {
        Route::dispose(&mut *self);
    }
}

/// A route a [route factory](NavigatorHandle::on_generate_route) produced, with
/// its concrete type erased so one generator can answer many names.
///
/// Construct one from any [`NavigatorRoute`] — a shipped [`PageRoute`],
/// [`PopupRoute`] or [`SimpleRoute`], or a route an app implements itself.
/// Registration stays typed underneath, so nothing about the route's own
/// `Output`, its `current_result` fallback, or its pop-result delivery changes.
///
/// # Example
///
/// ```
/// use flui_widgets::prelude::*;
/// use flui_widgets::{GeneratedRoute, NavigatorHandle, RouteRequest, Text};
///
/// let navigator = NavigatorHandle::new();
/// navigator.on_generate_route(|request: &RouteRequest<'_>| {
///     let title = request.name()?.to_owned();
///     Some(GeneratedRoute::new(
///         SimpleRoute::<()>::new(move |_ctx| Text::new(title.clone()).into_view().boxed()),
///     ))
/// });
/// ```
///
/// [`PageRoute`]: super::page_route::PageRoute
/// [`PopupRoute`]: super::page_route::PopupRoute
/// [`SimpleRoute`]: super::overlay_route::SimpleRoute
pub struct GeneratedRoute {
    /// `TypeId::of::<R::Output>()`, captured at construction so a wrong `T` is
    /// refused *before* anything is pushed. See [`Self::checked`].
    output: TypeId,
    /// `type_name::<R::Output>()`, for [`NamedRouteError::ResultType`] only.
    output_name: &'static str,
    /// `None` once the route has been handed to a navigator. `Option` rather
    /// than a bare box because this type has a [`Drop`] impl, which forbids
    /// moving the field out — the push takes it, and the drop that follows then
    /// finds nothing left to dispose.
    push: Option<Box<dyn ErasedPush>>, // PORT-CHECK-OK-DYN: the named-route result-handle erasure (ADR-0024 §7.2) — one generator answers many names, so the route type cannot appear in the factory's signature; private to this module and reachable only through `GeneratedRoute`
}

/// A generated route that never reached a navigator still owes
/// [`Route::dispose`].
///
/// `dispose` is an explicit trait method, not `Drop`, and the machinery already
/// assumes a never-installed route gets disposed. Two routine paths end here:
/// [`NavigatorHandle::push_named_typed`] refusing a mismatched `T`, and a
/// factory that builds a route and then decides to answer `None` after all.
/// Flutter carries the same obligation —
/// `Navigator.defaultGenerateInitialRoutes`' failure branch disposes every route
/// it had generated.
impl Drop for GeneratedRoute {
    fn drop(&mut self) {
        if let Some(unpushed) = self.push.take() {
            unpushed.dispose_unpushed();
        }
    }
}

impl fmt::Debug for GeneratedRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GeneratedRoute")
            .field("output", &self.output_name)
            .field("pushed", &self.push.is_none())
            .finish_non_exhaustive()
    }
}

impl GeneratedRoute {
    /// Erase `route` for a route table or generator hook.
    ///
    /// Dropping the result without pushing it disposes `route` — see the
    /// [`Drop`] impl above — so discarding one is safe, not a leak.
    #[must_use]
    pub fn new<R: NavigatorRoute>(route: R) -> Self {
        Self {
            output: TypeId::of::<R::Output>(),
            output_name: type_name::<R::Output>(),
            push: Some(Box::new(route)),
        }
    }

    /// Push the route through `mode`, reporting its new [`RouteId`] and its
    /// still-erased result handle.
    ///
    /// The untyped entry points keep the id and drop the handle; dropping a
    /// [`RouteResult`] cancels nothing, exactly as an unawaited Dart `Future`
    /// completes anyway.
    pub(super) fn push(
        mut self,
        handle: &NavigatorHandle,
        mode: PushMode<'_>,
    ) -> (RouteId, Box<dyn Any>) {
        self.push
            .take()
            .expect(
                "BUG: a GeneratedRoute is pushed at most once — `push` consumes it by value and \
                 `checked` moves it into the TypedPush token, so an empty slot here means the \
                 carrier was reconstructed around an already-pushed route",
            )
            .push_erased(handle, mode)
    }

    /// Confirm this route delivers `T`, yielding the token that can push it.
    ///
    /// This is the whole content of ADR-0024 §7.3's "with nothing pushed": the
    /// comparison happens against a `TypeId` captured at construction, so a
    /// mismatched `T` is refused while the stack is still untouched. Flutter
    /// re-types through an unchecked `as Route<T?>?` and never notices.
    ///
    /// For the **keyed** path its remit is now narrow, and deliberately so. Two
    /// registration sites disagreeing about one name are caught earlier, at the
    /// registration itself — see [`RouteRegistry::register_named`]. What cannot
    /// be caught there is the erased fall-through: a table entry that *declines*
    /// and an `on_generate_route` / `on_unknown_route` that answers with a
    /// differently-typed route. Nothing at those hooks' registration sites knows
    /// what `Output` they will produce, so the push is the first moment the
    /// answer exists. That one shape is what this check still earns for keyed
    /// callers; for `push_named_typed` it remains the general guard.
    ///
    /// On refusal `self` is dropped here, which disposes the route.
    pub(super) fn checked<T: Send + 'static>(
        self,
        request: &RouteSettings,
    ) -> Result<TypedPush<T>, NamedRouteError> {
        if self.output != TypeId::of::<T>() {
            return Err(NamedRouteError::ResultType {
                name: requested_name(request),
                expected: type_name::<T>(),
                actual: self.output_name,
            });
        }
        Ok(TypedPush {
            route: self,
            output: PhantomData,
        })
    }
}

/// A [`GeneratedRoute`] whose `Output` has been confirmed to be `T`.
///
/// Constructing one *is* the type check and consuming one *is* the push, so
/// [`NavigatorHandle::push_named_typed`] cannot mutate the stack before it has
/// checked. It owns the carrier rather than the box, so a token that is dropped
/// unpushed still disposes its route through the carrier's [`Drop`].
#[must_use = "a checked route reaches the stack only when `push` is called"]
pub(super) struct TypedPush<T> {
    route: GeneratedRoute,
    /// Covariant in `T`, and owns no `T` — this is a witness that the check in
    /// `GeneratedRoute::checked` passed, not a value. (Variance is inert either
    /// way: `Route::Output` is `'static` at every use site, so there is no
    /// lifetime to vary.)
    output: PhantomData<fn() -> T>,
}

impl<T: Send + 'static> TypedPush<T> {
    /// Push the route through `mode` and re-type its result handle.
    pub(super) fn push(
        self,
        handle: &NavigatorHandle,
        mode: PushMode<'_>,
    ) -> (RouteId, RouteResult<T>) {
        let (id, erased) = self.route.push(handle, mode);
        let typed = *erased
            .downcast::<RouteResult<T>>() // PORT-CHECK-OK-DOWNCAST: the named-route result-handle erasure (ADR-0024 §7.2); `GeneratedRoute::checked` compared `TypeId::of::<T>()` against the route's own `Output` before this token could exist
            .expect(
                "BUG: a TypedPush<T> exists only after TypeId::of::<T>() matched the route's \
                 Output, so the boxed result is a RouteResult<T>; reaching this means something \
                 constructed the token without going through GeneratedRoute::checked",
            );
        (id, typed)
    }
}

/// A route name that remembers what its route delivers.
///
/// The string path (`route` / `push_named` / `push_named_typed`) is Flutter's,
/// and it loses the result type at the registration boundary: nothing connects
/// `"/details"` to the `PageRoute<Order>` behind it, so
/// [`push_named_typed`](NavigatorHandle::push_named_typed) has to check at
/// runtime and can answer [`NamedRouteError::ResultType`]. A `RouteKey<T>`
/// carries the type alongside the name, so
/// [`route_keyed`](NavigatorHandle::route_keyed) rejects a mismatched route
/// **at the registration site, at compile time** — the check a string name
/// cannot carry.
///
/// It does not make [`NamedRouteError::ResultType`] unreachable, and this type's
/// docs used to claim it did. What survives is a *registry* collision: the table
/// is name-keyed, so two registration sites that disagree about one name still
/// meet at run time — including two `RouteKey`s that happen to spell the same
/// string, each self-consistent and both compiling. See
/// [`push_keyed`](NavigatorHandle::push_keyed).
///
/// Declare them once, as constants:
///
/// ```
/// use flui_widgets::prelude::*;
/// use flui_widgets::{NavigatorHandle, RouteKey, RouteRequest, Text};
///
/// const DETAILS: RouteKey<u32> = RouteKey::new("/details");
///
/// let navigator = NavigatorHandle::new();
/// navigator.route_keyed(DETAILS, |_request: &RouteRequest<'_>| {
///     Some(SimpleRoute::<u32>::new(|_ctx| Text::new("Details").into_view().boxed()))
/// });
///
/// // `T` comes from the key; nothing is spelled twice and nothing can drift.
/// let details = navigator.push_keyed(DETAILS)?;
/// # let _ = details;
/// # Ok::<(), flui_widgets::NamedRouteError>(())
/// ```
///
/// Registering `SimpleRoute::<String>` under `DETAILS` above is a type error,
/// not a runtime `ResultType`.
pub struct RouteKey<T> {
    name: &'static str,
    /// Covariant in `T` and owning none — the key is a name plus a promise.
    output: PhantomData<fn() -> T>,
}

// Written out rather than derived: `#[derive]` would add a spurious `T: Clone`
// / `T: Eq` / `T: Hash` bound, and a key's identity is its *name* — the `T` is a
// compile-time promise with no runtime representation to compare or hash.
impl<T> Clone for RouteKey<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for RouteKey<T> {}

impl<T> PartialEq for RouteKey<T> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl<T> Eq for RouteKey<T> {}

impl<T> Hash for RouteKey<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl<T> fmt::Debug for RouteKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteKey")
            .field("name", &self.name)
            .field("output", &type_name::<T>())
            .finish()
    }
}

impl<T> RouteKey<T> {
    /// Declare a key. `const`, so an app's route table is a set of constants.
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            output: PhantomData,
        }
    }

    /// The name this key registers and resolves under — the same string the
    /// untyped [`route`](NavigatorHandle::route) path would use.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Attach an arguments payload, producing the request
    /// [`push_keyed`](NavigatorHandle::push_keyed) takes.
    ///
    /// Deliberately **not** a `push_keyed_with(key, arguments)` method: on this
    /// handle `_with` means "a result delivered to the departing route"
    /// throughout ([`pop_with`](NavigatorHandle::pop_with),
    /// [`push_replacement_with`](NavigatorHandle::push_replacement_with),
    /// [`remove_route_with`](NavigatorHandle::remove_route_with),
    /// [`maybe_pop_with`](NavigatorHandle::maybe_pop_with)), and spending that
    /// suffix on arguments would make one word mean two things.
    #[must_use]
    pub fn request<A: Any + Send + Sync + 'static>(self, arguments: A) -> KeyedRequest<T> {
        KeyedRequest {
            settings: RouteSettings::named(self.name).with_arguments(arguments),
            output: PhantomData,
        }
    }
}

/// A [`RouteKey`] request, with arguments if it has any.
///
/// The keyed counterpart to the string path's `impl Into<RouteSettings>`: a bare
/// key converts into one, so `push_keyed(DETAILS)` and
/// `push_keyed(DETAILS.request(id))` are the same call.
pub struct KeyedRequest<T> {
    settings: RouteSettings,
    output: PhantomData<fn() -> T>,
}

// Hand-written for the same reason as [`RouteKey`]'s: `#[derive(Debug)]` would
// generate `impl<T: Debug> Debug`, so a `KeyedRequest<T>` whose route `Output`
// is merely `Send + 'static` would not be `Debug` and could not sit in a
// caller's own derived `Debug` struct. `T` has no runtime representation here to
// format.
impl<T> fmt::Debug for KeyedRequest<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyedRequest")
            .field("settings", &self.settings)
            .field("output", &type_name::<T>())
            .finish()
    }
}

impl<T> KeyedRequest<T> {
    /// The settings this request resolves with.
    #[must_use]
    pub fn settings(&self) -> &RouteSettings {
        &self.settings
    }

    /// Consume into the settings the string path resolves with.
    pub(super) fn into_settings(self) -> RouteSettings {
        self.settings
    }
}

impl<T> From<RouteKey<T>> for KeyedRequest<T> {
    fn from(key: RouteKey<T>) -> Self {
        Self {
            settings: RouteSettings::named(key.name()),
            output: PhantomData,
        }
    }
}

/// Why a named-route request produced no route.
///
/// Both variants are *caller* errors — a route name is input, not a framework
/// invariant — which is why [`PANIC-POLICY`](../../../../../docs/PANIC-POLICY.md)
/// puts them on the `Result` side. Flutter throws for the first and never
/// detects the second.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NamedRouteError {
    /// No registered factory answered the request: the table had no entry, and
    /// neither [`on_generate_route`](NavigatorHandle::on_generate_route) nor
    /// [`on_unknown_route`](NavigatorHandle::on_unknown_route) produced a route
    /// (or neither was registered). Flutter's
    /// `'Navigator.onGenerateRoute returned null'` assertion.
    #[error("no route was generated for the name {name:?}")]
    Unresolved {
        /// The requested name, or `""` when the request carried none.
        name: String,
    },
    /// A route *was* generated, but it delivers a different result type than
    /// the one requested. **Nothing was pushed** — the stack is unchanged.
    #[error("route {name:?} delivers {actual}, but {expected} was requested")]
    ResultType {
        /// The requested name, or `""` when the request carried none.
        name: String,
        /// `type_name` of the `T` the caller asked
        /// [`push_named_typed`](NavigatorHandle::push_named_typed) for.
        expected: &'static str,
        /// `type_name` of the generated route's own `Route::Output`.
        actual: &'static str,
    },
}

/// The name to blame in a [`NamedRouteError`]. A request may legitimately carry
/// none (`RouteSettings::default()` plus arguments), which reports as `""`
/// rather than costing every variant an `Option`.
pub(super) fn requested_name(request: &RouteSettings) -> String {
    request.name().unwrap_or_default().to_owned()
}

/// One navigator's name → route table and its two generator hooks.
///
/// Owned by `NavigatorShared`, so two navigators resolve the same name
/// independently — there is no process-global route table.
#[derive(Default)]
pub(super) struct RouteRegistry {
    registrations: Mutex<Registrations>,
}

#[derive(Default)]
struct Registrations {
    table: HashMap<String, RegisteredRoute>,
    generate: Option<RouteFactory>,
    unknown: Option<RouteFactory>,
    /// Re-registrations that changed a name's `Output` type. Counted in full;
    /// only the first is warned about.
    conflicts_seen: usize,
    /// Warnings actually emitted. The latch: `register_named` warns only while
    /// this is zero, so a registration loop reports once rather than per call.
    warns_emitted: usize,
}

/// One table entry: the factory, plus what the route it builds delivers.
///
/// The `Output` type is stored because [`NavigatorHandle::route`] has `R` in
/// scope and [`NavigatorHandle::route_keyed`] knows the key's `T`, so a
/// disagreement between two registrations of one name is detectable **at the
/// site where it was made** — which is where a programmer error at startup
/// should surface, rather than at some later push.
struct RegisteredRoute {
    factory: RouteFactory,
    output: TypeId,
    output_name: &'static str,
}

impl RouteRegistry {
    /// Bind `name` to `factory`, replacing any previous binding.
    ///
    /// Replacing is legitimate and stays legitimate — `ARCHITECTURE.md` §6's
    /// contract is that an app builder replaces the table wholesale at mount, so
    /// an app that deliberately changes a route's `Output` between rebuilds is
    /// making a valid change and must not be refused. What is *reported* is the
    /// case that is almost never intended: a re-registration that changes the
    /// name's `Output` **type**, which is how two registration sites silently
    /// disagree about one name and defeat a [`RouteKey`]'s compile-time promise.
    ///
    /// Latched — the first conflict warns, later ones are counted only, because
    /// an unlatched warn in a registration loop is noise.
    pub(super) fn register_named(
        &self,
        name: String,
        factory: RouteFactory,
        output: TypeId,
        output_name: &'static str,
    ) {
        let displaced = {
            let mut registrations = self.registrations.lock();
            let previous = registrations.table.insert(
                name.clone(),
                RegisteredRoute {
                    factory,
                    output,
                    output_name,
                },
            );
            match previous {
                Some(previous) if previous.output != output => {
                    registrations.conflicts_seen += 1;
                    let first = registrations.warns_emitted == 0;
                    first.then_some(previous.output_name)
                }
                _ => None,
            }
        };

        // Emitted with the registry lock released, like every other call out of
        // this module: a `tracing` subscriber is user code too.
        if let Some(previous_output) = displaced {
            tracing::warn!(
                route = %name,
                previous_output,
                replacement_output = output_name,
                "a route name was re-registered with a different result type; a RouteKey's \
                 compile-time promise only covers its own registration site, so the last \
                 registration wins and a keyed push of this name will report NamedRouteError::\
                 ResultType. Later conflicts on this navigator are counted but not warned."
            );
            self.registrations.lock().warns_emitted += 1;
        }
    }

    /// Drop every registration. Called from `NavigatorState::dispose`.
    ///
    /// Terminal, not a detach: `activate` is the reattach hook and nothing
    /// reparents through `dispose`. This is what makes the hazard note on
    /// [`NavigatorHandle::on_generate_route`] performable — a factory that
    /// captured a handle anyway has its cycle broken when the navigator it was
    /// registered on unmounts, rather than the caller being told to break a
    /// cycle the surface gave them no way to reach.
    pub(super) fn clear(&self) {
        let mut registrations = self.registrations.lock();
        registrations.table.clear();
        registrations.generate = None;
        registrations.unknown = None;
    }

    /// Re-registrations that changed a name's `Output`. Test-facing.
    #[cfg(test)]
    pub(super) fn conflicts_seen(&self) -> usize {
        self.registrations.lock().conflicts_seen
    }

    /// Conflict warnings actually emitted — the latch's own count, incremented
    /// in the same block that calls `tracing::warn!`, so a test asserting on it
    /// is asserting on the warn rather than on a parallel predicate.
    #[cfg(test)]
    pub(super) fn warns_emitted(&self) -> usize {
        self.registrations.lock().warns_emitted
    }

    /// Install the catch-all generator — Flutter's `Navigator.onGenerateRoute`.
    pub(super) fn register_generator(&self, factory: RouteFactory) {
        self.registrations.lock().generate = Some(factory);
    }

    /// Install the last-resort fallback — Flutter's `Navigator.onUnknownRoute`.
    pub(super) fn register_unknown_fallback(&self, factory: RouteFactory) {
        self.registrations.lock().unknown = Some(factory);
    }

    /// Resolve `request`: table entry → generator → unknown-route fallback,
    /// the order `WidgetsApp._onGenerateRoute` feeding `NavigatorState._routeNamed`
    /// produces. A stage that answers `None` passes the request on.
    ///
    /// # Every factory runs with the registry lock released
    ///
    /// A factory is user code, and user code may register another route or push
    /// re-entrantly. `parking_lot::Mutex` is not reentrant, so the `Rc` is
    /// cloned out first and invoked outside the guard — the same hazard
    /// [`Route::on_pop_invoked`] and
    /// [`NavigatorHandle::push_and_remove_until`]'s predicate already document.
    /// Pinned by `a_factory_that_pushes_re_entrantly_does_not_deadlock`.
    pub(super) fn resolve(&self, request: &RouteRequest<'_>) -> Option<GeneratedRoute> {
        if let Some(name) = request.name()
            && let Some(from_table) = self.pick(|registrations| {
                registrations
                    .table
                    .get(name)
                    .map(|entry| entry.factory.clone())
            })
            && let Some(route) = from_table(request)
        {
            return Some(route);
        }
        if let Some(generate) = self.pick(|registrations| registrations.generate.clone())
            && let Some(route) = generate(request)
        {
            return Some(route);
        }
        let unknown = self.pick(|registrations| registrations.unknown.clone())?;
        unknown(request)
    }

    /// Clone one factory out from under the lock. The guard is dropped as this
    /// returns — nothing here may call the factory.
    fn pick(
        &self,
        choose: impl FnOnce(&Registrations) -> Option<RouteFactory>,
    ) -> Option<RouteFactory> {
        choose(&self.registrations.lock())
    }
}
