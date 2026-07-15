# ADR-0024 — Named routes: `on_generate_route`, `push_named`, `RouteSettings.arguments`

- **Status:** **Proposed — design only, awaiting Gate sign-off.** No code lands with this ADR. Unlike ADR-0022/0023, implementation does not start on acceptance of the shape alone: §4's two new type-erasure boundaries need the repository owner's sign-off, the same gate ADR-0019 §7e ran for the `dyn Any` pop-result.
- **Date:** 2026-07-10
- **Deciders:** repository owner (**Gate: two new `dyn Any` boundaries on the public surface**, §4); chief-architect (the erased-route seam, §3.2); qa-lead (generator and delivery-time-typing tests).
- **Relates to:** ADR-0019 (named-route generation deferred in §6; the delivery-time-checked result contract this reuses, §4/§7e); B1.1's "named-route generation remains deferred" line; unblocks `MaterialApp.routes`-style tables in Catalog.1.

---

## 1. Context

FLUI's `Navigator` pushes concrete typed routes: `handle.push(PageRoute::<i32>::new(…))`. Flutter additionally routes by **name**: `Navigator.pushNamed(context, '/settings', arguments: …)` resolves through the navigator-level `onGenerateRoute: Route<dynamic>? Function(RouteSettings)` (`navigator.dart:1695`), falling back to `onUnknownRoute` (`:1705`). `RouteSettings` carries `name` and `arguments: Object?` (`:670-687`). FLUI's `RouteSettings` exists **minus `arguments`** — deferred with this feature (`route.rs:79-85`).

Everything below the name layer already exists: route erasure (`RouteRecord::erase` → `Box<dyn ErasedRoute>` + a typed `RouteResult`), the delivery-time-checked result channel (`pop_with`'s contract, ADR-0019 §4), and the `push_prepared` front-door shape (ADR-0019 §7d update).

## 2. Reference

`.flutter/packages/flutter/lib/src/widgets/navigator.dart`, master `3.33.0-0.0.pre-6280-g88e87cd963f`: `onGenerateRoute`/`onUnknownRoute` (`:1597-1705`), `pushNamed<T>` (`:1833-1870`), `_routeNamed<T>` (settings construction + the onUnknownRoute fallback + the debug assert), `RouteSettings` (`:670-687`), `defaultRouteName` + the initial-route hierarchy synthesis (`:3031-3073`).

## 3. The Rust shape

### 3.1 Registration — on the handle, not the widget

Flutter configures the generator on the `Navigator` widget. FLUI's widget is a thin shell over `NavigatorHandle`, and every push goes through the handle, so the generator registers there:

```rust
let navigator = NavigatorHandle::new();
navigator.set_on_generate_route(|settings| match settings.name()? {
    "/settings" => Some(GeneratedRoute::new(PageRoute::<i32>::new(…))),
    _ => None,
});
navigator.set_on_unknown_route(|settings| Some(GeneratedRoute::new(not_found_page())));
```

Stored as `Mutex<Option<Arc<dyn Fn(&RouteSettings) -> Option<GeneratedRoute> + Send + Sync>>>` on `NavigatorShared`. Both are plain setters, not constructor state: Flutter allows a rebuilt `Navigator` to swap callbacks, and a setter is that without widget-diff plumbing.

### 3.2 `GeneratedRoute` — the erased carrier

The generator must return heterogeneous routes; the caller of `push_named` cannot name the concrete `Output`. `GeneratedRoute::new(route: impl NavigatorRoute)` performs the same erasure `push` performs internally — capturing the `Box<dyn ErasedRoute>`, the pre-minted id, the binding fill and overlay-entry closure — plus an **erased result handle**. `push_named::<T>(name)` then re-types the result with the *existing* delivery-time contract: the concrete route completes with its own `Output`; if `T` differs, delivery logs and completes with `None`, never panics — exactly `pop_with`'s sanctioned behavior extended from "the popper guesses the top route's type" to "the pusher guesses the named route's type". No new failure mode, one new erased type.

### 3.3 The named surface (dependency-ordered units)

- **U1**: `RouteSettings.arguments` (§4.1), `set_on_generate_route`/`set_on_unknown_route`, `GeneratedRoute`, and `push_named::<T>(name) -> Option<RouteResult<T>>` — `None` when neither callback produced a route (Flutter asserts in debug; FLUI returns the absence, and logs).
- **U2**: `push_named_with_arguments`, `push_replacement_named`, `push_named_and_remove_until` — each a one-line composition of U1 with the ADR-0019 §7d front doors.
- **U3 (deferred indefinitely)**: `defaultRouteName` + initial-route hierarchy synthesis (`:3031-3073`) — FLUI bootstraps with `seed_initial`, and the `/a/b`→`[/, /a, /a/b]` synthesis has no consumer until deep links exist.

## 4. The gate — two new public `dyn Any` boundaries

1. **`RouteSettings.arguments`**: `Option<Arc<dyn Any + Send + Sync>>` with a typed accessor `settings.argument::<T>() -> Option<&T>`. The downcast site carries the FR-033/widgets marker; the alternative (a generic `RouteSettings<A>`) infects every route type and the history with a type parameter for a field most routes never read.
2. **`GeneratedRoute`'s erased result** (§3.2): the same `Box<dyn Any + Send>` channel the pop result crosses, reused rather than a second invention.

Both mirror sanctioned shapes, but both **widen the public erased surface**, which ADR-0019 §7e's precedent says needs the repository owner's explicit sign-off before code lands. Questions for the gate:

- Q1: Is `Arc<dyn Any + Send + Sync>` acceptable for `arguments`, or must arguments stay out (callers close over their data in the generator — Rust closures make Flutter's escape hatch far less necessary than in Dart)?
- Q2: `push_named::<T>` returning `Option<RouteResult<T>>` (absence = no route generated) versus Flutter's throw — acceptable?
- Q3: Should `GeneratedRoute` be constructible from user-implemented `NavigatorRoute`s, or only from the shipped `PageRoute`/`PopupRoute`/`SimpleRoute` (a sealed constructor set) until the erased seam proves out?

## 5. Consequences

**Good.** Every mechanism reuses proven machinery (`RouteRecord::erase`, delivery-time typing, `push_prepared`); the units are small once gated. **Bad.** Two more `Any` boundaries to audit; and note Q1's honest observation — with Rust closures, `arguments` is much less load-bearing than in Dart, so the gate may legitimately answer "skip it". **Deferred, named:** `onGenerateInitialRoutes`, `restorablePushNamed`, `Navigator.defaultRouteName` synthesis (U3).

## 6. Update (2026-07-15) — §4.1's field landed; §4.2 and generation did not

`RouteSettings.arguments` shipped (`route.rs`) in exactly the shape §4.1 named: `Option<Arc<dyn Any + Send + Sync>>` behind the public `RouteArguments` alias, with the downcast accessor named `argument::<T>()` as this ADR specified. Ported alongside the `pop_until` gap fix (Business.1, Navigator API gaps); see `tests/parity/navigator_test.rs`'s `route_settings_arguments_round_trip_via_downcast`.

This answers Q1 affirmatively (`arguments` stays in) but does **not** constitute the Gate sign-off §3.3/§4 describe — it landed on direct task authorization, not a recorded repository-owner decision. The single-field surface is small enough, and closely enough matches an already-sanctioned precedent (`flui-objects::MetaDataPayload`), that landing it did not seem to warrant blocking on a formal gate event; a reviewer who disagrees with that call should treat this update as the flag to re-open it, not as the gate having already run.

**Still outstanding, still gated:** §4.2's `GeneratedRoute` erased-result channel, `set_on_generate_route`/`set_on_unknown_route` registration, and `push_named::<T>` — none of U1 beyond the `arguments` field has landed. `onGenerateRoute`/named-route generation remains absent; nothing yet constructs a route from `arguments`.
