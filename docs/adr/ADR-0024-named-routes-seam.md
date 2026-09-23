# ADR-0024: Named routes (`on_generate_route`, `push_named`, `RouteSettings.arguments`)

- **Status:** Deprecated. String-named routes are replaced by the typed Router (roadmap track
  A3: routes as a typed tree, the URL as source of truth). `RouteKey<T>` (§3) is the part to
  carry over into it. The shipped surface below stays until the Router supersedes it.
- **Date:** 2026-07-10

## Context

Flutter routes by name: `Navigator.pushNamed(context, '/settings', arguments: …)` resolves
through `onGenerateRoute: Route<dynamic>? Function(RouteSettings)`, falling back to
`onUnknownRoute`; `WidgetsApp` folds its `routes` table and `home` into that one hook. FLUI's
`Navigator` pushed only concrete typed routes, and `MaterialApp.routes`-style tables needed a
name layer on top of the existing erasure and delivery-time-typed result channel (ADR-0019).

## Decision (as shipped)

### 1. Registration is typed; only the result handle is erased

Routes register on `NavigatorHandle` (`route`, `route_keyed`, `on_generate_route`,
`on_unknown_route`); the factory returns a concrete `R: NavigatorRoute`, so a `PageRoute<bool>`
keeps `Output = bool` and `pop_with(true)` still types correctly. The factory's route feeds the
existing typed `push`/`push_replacement`/`push_and_remove_until` front doors; the named path
erases only the returned result handle, internally. There is no new public `dyn Any` boundary
beyond `RouteSettings.arguments` (`Option<Arc<dyn Any + Send + Sync>>`, read with
`settings.argument::<T>()`).

Erasing the route's `Output` instead was rejected: no shipped route type could register
(`Box<dyn Any>` is not `Clone`), and every named route would lose its pop result to a
double-boxed downcast.

The factory receives settings, name and arguments, not a navigator handle; a redirect is
expressed by returning a different route. Factories are `Rc<dyn Fn>`, since `NavigatorHandle` is
owner-affine (ADR-0027). The push path still captures the target route before resolving, so an
operation acts on the route the caller named even if a factory mutates the stack through a
captured handle.

### 2. Errors are a `Result`, across two entry points

The ordinary operations (`push_named`, `push_replacement_named[_with]`,
`pop_and_push_named[_with]`, `push_named_and_remove_until`) return
`Result<RouteId, NamedRouteError>` whose only error is `Unresolved { name }`; they never check
the result type, so reaching a screen does not require knowing its `Output`.
`push_named_typed::<T>` compares `TypeId`s before pushing, so a mismatch is
`NamedRouteError::ResultType` with the stack unmutated. Flutter re-types through an unchecked
`as Route<T?>?` cast; a panic was rejected because a route name is caller input
(`docs/PANIC-POLICY.md`). A generated route that is never pushed is disposed on drop.

### 3. `RouteKey<T>`: the name carries its result type

```rust
const SETTINGS: RouteKey<i32> = RouteKey::new("/settings");
handle.route_keyed(SETTINGS, |_| Some(PageRoute::<i32>::new(page)));
let result: RouteResult<i32> = handle.push_keyed(SETTINGS)?;
```

`route_keyed` is bounded `R: NavigatorRoute<Output = T>`, so a registration whose route
disagrees with its key does not compile. The table is still string-keyed at run time: two keys
sharing a name with different `T` both compile, the last registration wins with a latched
per-navigator `tracing::warn!`, and `push_keyed` on the stale key returns `ResultType`.
Closing that gap needs a registry keyed by type, not by string — which is the typed Router.

## Deferred

`initialRoute` hierarchy synthesis (`'/settings'` seeds `['/', '/settings']` in Flutter),
`onGenerateInitialRoutes`, `restorablePushNamed`. All three belong to the Router.
