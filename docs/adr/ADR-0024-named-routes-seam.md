# ADR-0024 — Named routes: `on_generate_route`, `push_named`, `RouteSettings.arguments`

- **Status:** **Accepted (2026-09-07).** The Gate this ADR waited on has run — see §7. §4.1 shipped in 2026-07-15; §4.2 is approved **as amended by §7**, which supersedes §3.2's erasure shape. Implementation of U1+U2 may proceed; U3 stays deferred.
- **Date:** 2026-07-10
- **Deciders:** repository owner (**Gate: two new `dyn Any` boundaries on the public surface**, §4); chief-architect (the erased-route seam, §3.2); qa-lead (generator and delivery-time-typing tests).
- **Relates to:** ADR-0019 (named-route generation deferred in §6; the delivery-time-checked result contract this reuses, §4/§7e); B1.1's "named-route generation remains deferred" line; unblocks `MaterialApp.routes`-style tables in Catalog.1.

---

## 1. Context

FLUI's `Navigator` pushes concrete typed routes: `handle.push(PageRoute::<i32>::new(…))`. Flutter additionally routes by **name**: `Navigator.pushNamed(context, '/settings', arguments: …)` resolves through the navigator-level `onGenerateRoute: Route<dynamic>? Function(RouteSettings)` (`navigator.dart:1695`), falling back to `onUnknownRoute` (`:1705`). `RouteSettings` carries `name` and `arguments: Object?` (`:670-687`). FLUI's `RouteSettings` exists **minus `arguments`** — deferred with this feature (`route.rs:79-85`).

Everything below the name layer already exists: route erasure (`RouteRecord::erase` → `Box<dyn ErasedRoute>` + a typed `RouteResult`), the delivery-time-checked result channel (`pop_with`'s contract, ADR-0019 §4), and the `push_prepared` front-door shape (ADR-0019 §7d update).

## 2. Reference

`.flutter/packages/flutter/lib/src/widgets/navigator.dart`. **Cited by symbol, not by line** — this
ADR originally cited master `3.33.0-0.0.pre-6280-g88e87cd963f` and every one of those line numbers
had moved by the time the Gate ran; the repository is now pinned at tag `3.44.0`
(`git -C .flutter describe --tags`). Resolve each by name: `RouteFactory` (the
`Route<dynamic>? Function(RouteSettings)` typedef), `Navigator.onGenerateRoute` /
`Navigator.onUnknownRoute` / `Navigator.initialRoute` / `Navigator.onGenerateInitialRoutes` /
`Navigator.defaultRouteName`, `NavigatorState._routeNamed<T>` (settings construction, the
`onUnknownRoute` fallback, and the debug assert), `RouteSettings`, and
`Navigator.defaultGenerateInitialRoutes` (the initial-route hierarchy synthesis). The
`routes: Map<String, WidgetBuilder>` table is **not** on `Navigator` at all — it lives on
`WidgetsApp`, whose `_onGenerateRoute` folds `home`, `routes` and the user's `onGenerateRoute`
into the single hook `Navigator` actually reads (`lib/src/widgets/app.dart`).

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

### 3.2 `GeneratedRoute` — the erased carrier — **SUPERSEDED by §7.2**

The generator must return heterogeneous routes; the caller of `push_named` cannot name the concrete `Output`. `GeneratedRoute::new(route: impl NavigatorRoute)` performs the same erasure `push` performs internally — capturing the `Box<dyn ErasedRoute>`, the pre-minted id, the binding fill and overlay-entry closure — plus an **erased result handle**. `push_named::<T>(name)` then re-types the result with the *existing* delivery-time contract: the concrete route completes with its own `Output`; if `T` differs, delivery logs and completes with `None`, never panics — exactly `pop_with`'s sanctioned behavior extended from "the popper guesses the top route's type" to "the pusher guesses the named route's type". No new failure mode, one new erased type.

> **Superseded (2026-09-07).** The paragraph above is kept as the record of what was proposed, not
> as guidance. Its shape does not survive contact with the crate: erasing a route's own `Output`
> makes the registry uninhabitable by every route class this crate ships, and silently destroys
> every named route's pop result. Both defects are compile-provable; see §7.2 for the evidence and
> the replacement.

### 3.3 The named surface (dependency-ordered units)

> **These are the names this ADR *proposed* in 2026-07-10, not the ones that shipped.** Kept
> as the record of the proposal; **do not copy them** — `set_on_generate_route` /
> `set_on_unknown_route` / `push_named::<T>` / `push_named_with_arguments` do not exist. The
> shipped surface is `route` / `route_keyed` / `on_generate_route` / `on_unknown_route`, the six
> untyped operations returning `Result<RouteId, NamedRouteError>`, `push_named_typed::<T>`,
> `push_keyed`, and arguments carried in `RouteSettings` (`with_arguments` /
> `with_arguments_shared`) or `RouteKey::request` / `request_shared`. See §7.3 and §7.9.

- **U1**: `RouteSettings.arguments` (§4.1), `set_on_generate_route`/`set_on_unknown_route`, `GeneratedRoute`, and `push_named::<T>(request) -> Result<RouteResult<T>, NamedRouteError>` (**amended by §7.3**; originally `Option<RouteResult<T>>`).
- **U2**: `push_named_with_arguments`, `push_replacement_named`, `push_named_and_remove_until` — each a one-line composition of U1 with the ADR-0019 §7d front doors.
- **U3 (deferred, but see §7.6 — this sentence was wrong)**: `defaultRouteName` + initial-route hierarchy synthesis (`Navigator.defaultGenerateInitialRoutes`) — originally justified as having "no consumer until deep links exist". It is still deferred; that justification is false.

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

**Sign-off (2026-07-15):** the repository owner reviewed §4's Q1 (`RouteSettings.arguments` as `Arc<dyn Any + Send + Sync>`) and the shipped `route.rs` shape above, and approved it as-is — ship. This is the formal Gate event the update above noted was still missing for the `arguments` field; it does not extend to §4.2 (`GeneratedRoute`, `set_on_generate_route`/`set_on_unknown_route`, `push_named`), which remain proposed and ungated.

---

## 7. Gate record (2026-09-07) — §4.2 approved **as amended**

The Gate §3.3/§4 waited on has run. The repository owner reviewed §4.2 and approved it **subject to
the amendments below**, which came out of an adversarial plan review conducted before any code was
written. Recorded here the way ADR-0019 §7e's precedent requires: a decision by the repository
owner, stated plainly, not a review implied.

### 7.1 What was approved

`set_on_generate_route` / `set_on_unknown_route` registration, a route table, the named entry
points (U1 + U2), and named-route construction from **any** user-implemented `NavigatorRoute`.
U3 (`defaultRouteName` + initial-route hierarchy synthesis) remains deferred, unchanged.

### 7.2 Amendment — erase the result **handle**, never the route's `Output` (supersedes §3.2)

§3.2 proposed a carrier whose route has `Output = Box<dyn Any + Send>`. Two compile-provable
defects, each independently fatal:

1. **The registry would be uninhabitable.** `Route::current_result(&mut self) -> Option<Self::Output>`
   returns by value, so every shipped route clones its stored fallback and every impl is bounded
   `T: Send + Clone + 'static` — `SimpleRoute`, `ModalRoute`, `PageRoute`, `PopupRoute`,
   `TransitionRoute`. `Box<dyn Any + Send>` is not `Clone`, so
   `error[E0277]: the trait bound 'dyn Any + Send: Clone' is not satisfied`. No route type this
   crate ships could ever have been registered — including the Material page route a route table
   exists to serve.
2. **Every named route would silently lose every pop result.** `pop_with<T>` erases **once**, and
   `RouteRecord::did_complete` downcasts the payload to `R::Output`. When `R::Output` *is* the
   erasure type, that downcast demands a double-boxed value, so an ordinary
   `handle.pop_with(EditResult { .. })` resolves the future with `None` and logs a type name the
   caller cannot act on. Correct use would have required
   `pop_with(Box::new(v) as Box<dyn Any + Send>)` at every pop site — undocumented and
   un-typecheckable. No adapter fixes this: the downcast is rooted in `R::Output`, whatever wraps
   the route.

**Replacement.** Registration stays **typed** — the factory returns a concrete
`R: NavigatorRoute`, so `PageRoute<bool>` keeps `Output = bool`, `current_result` clones a `bool`,
and `pop_with(true)` downcasts to `bool`. The erasure moves to the **result handle**: the factory
feeds the existing `push` / `push_replacement` / `push_and_remove_until` front doors unchanged
(preserving one history state machine and one observer ordering), and `push_named::<T>` downcasts
one boxed `RouteResult` back to `RouteResult<T>`, carrying a `PORT-CHECK-OK-DOWNCAST` marker.

**Consequence for §4's gate question:** there is now **no new public `dyn Any` boundary**.
`AnyResult` stays `pub(crate)` and stays on `tests/navigator_public.rs`'s `INTERNAL` list. The
erasure §4.2 was gating is entirely internal to the named-route path.

### 7.3 Q2 answered — a `Result`, across two entry points

§4's Q2 asked whether `Option<RouteResult<T>>` was acceptable versus Flutter's throw. **Neither.**
The answer is a `Result`, across a **two-entry-point** surface.

The first shape tried here had a single generic `push_named::<T>`, checking the result type before
pushing. That is right about the *signal* and wrong about the *common case*: it makes
`push_named::<()>("/settings")` against a `PageRoute<i32>` generator return `Err` and **not
navigate**, where Flutter's `pushNamed<void>` navigates fine. A caller who only wants to reach a
screen has no reason to know the route's result type, and refusing to navigate is a worse failure
than the silent `None` it was replacing.

So the ordinary operations are infallible in the result type — `push_named`,
`push_replacement_named[_with]`, `pop_and_push_named[_with]` and `push_named_and_remove_until` all
return `Result<RouteId, NamedRouteError>`, whose only error is `Unresolved { name }`. `RouteId` is
a useful return: it feeds `remove_route` and pairs with `current()`.

One typed entry point, `push_named_typed::<T>(req) -> Result<RouteResult<T>, NamedRouteError>`,
carries the pre-mutation guarantee: the `TypeId` captured in `GeneratedRoute` at construction is
compared **before** anything is pushed, so a mismatch is
`Err(NamedRouteError::ResultType { .. })` with the stack unmutated. `ResultType` is therefore
producible only where the caller has explicitly asserted a type.

Typed siblings for replacement and remove-until are not offered: no consumer today, and purely
additive later.

This is better than the reference, not merely different: Flutter re-types through an unchecked
`as Route<T?>?` cast, so a wrong type is undetected there. It is also better than this ADR's own
original answer, which deferred the failure to delivery and reported it as an indistinguishable
`None`. `NamedRouteError` is `#[non_exhaustive]` from birth.

**Replacement tests** (Prime Directive #1(b) — named here, not only cross-referenced, and each
with the mutation it detects):

- `tests/navigator_public.rs::push_named_typed_with_the_wrong_result_type_errors_disposes_the_route_and_changes_nothing`
  — deleting the `TypeId` comparison in `GeneratedRoute::checked` makes the push land before the
  downcast and panic in `TypedPush::push`'s `BUG:` expect, with the stack already mutated.
- `tests/navigator_public.rs::pop_and_push_named_with_an_unresolvable_name_pops_nothing` and
  `…_delivers_its_result_to_nobody` — moving the pop back ahead of the resolve (Flutter's order)
  fails both: the stack reads `[RouteId(1), RouteId(2)]` where it must read `[RouteId(1)]`.
- `tests/navigator_public.rs::a_route_whose_output_the_caller_never_names_is_still_navigable_by_name`
  — type-checking `push_named` against `()` makes it refuse to navigate, which is the regression
  the two-entry-point split exists to prevent.

`crates/flui-widgets/ARCHITECTURE.md` §4 carries the same divergence as a `## Mapping decisions`
entry, with the reasoning behind the split surface.

A panic was rejected: a route name is caller input, not an internal invariant, so
`docs/PANIC-POLICY.md` puts it on the `Result` side.

### 7.4 Q3 answered — any user-implemented `NavigatorRoute`

§4's Q3 asked whether the carrier should be sealed to the shipped route types "until the erased
seam proves out". Under §7.2 that caution no longer applies to anything: registration is typed, so
a user route is erased by exactly the same machinery as a shipped one and weakens nothing. Sealing
would block custom routes from a route table for no safety gain, and unsealing later would itself
be a breaking change.

### 7.5 Also decided in the same slice

`#[non_exhaustive]` lands on `NavigatorCommand` and `NavigatorCommandOutcome`. This slice is what
first makes that enum growable — the enum's own doc explains that a push cannot be a command
because a route "cannot be made `Send`", and a route *name* is `Send`. The attribute is a breaking
change that is free today and is not free later.

### 7.6 Corrections to this ADR's own statements, found during the gate review

Three claims in the sections above are wrong. They are corrected here rather than silently edited,
so a reader who remembers the old text can see what changed.

1. **§3.2's "one-line composition" (§3.3, U2) is false, and eager erasure is the wrong shape.**
   All four history entry points are generic `<R: Route>` and call `RouteRecord::erase_with_id`
   *internally* — `seed_initial_with_id`, `push_with_id`, `push_replacement_with_id`,
   `push_for_remove_until_with_id` (`history.rs`). A pre-erased route cannot enter any of them, so
   §3.2's eager erasure would require four new erased-taking variants in `history.rs`, plus manual
   binding-slot and overlay-entry handling in `push_named` — because boxing to `dyn ErasedRoute`
   drops `binding_slot()` and `content_builder()`, which live on `NavigatorRoute`/`Route` and are
   absent from `ErasedRoute`.

   **The implemented shape keeps the route concrete inside the carrier** and dispatches to the
   existing typed front doors, which already perform the binding fill, the overlay insert and the
   erasure in the correct order. `history.rs` is untouched. The public name `GeneratedRoute` is
   retained for traceability with this ADR; only its internals differ.

2. **A `GeneratedRoute` that is built and never pushed must be disposed.** `dispose` is an
   explicit `Route`/`ErasedRoute` method, not `Drop`, and the machinery assumes a never-installed
   route is disposed. Flutter disposes discarded generated routes explicitly (the failure branch
   of `defaultGenerateInitialRoutes`). §7.3's pre-push type check makes the discard path
   **routine** rather than rare, so `GeneratedRoute` carries a `Drop` impl that disposes an
   unpushed route, and a test asserts it on the `ResultType` error path.

3. **U3's justification — "no consumer until deep links exist" — is false.** Read
   `Navigator.defaultGenerateInitialRoutes`: **any** initial route name other than `/` takes the
   expansion branch, and it seeds `/` **first**. `initialRoute: '/settings'` therefore yields
   `['/', '/settings']` — a two-deep stack whose back button returns home, not a one-deep stack
   that exits the app. The consumer is `MaterialApp(initialRoute:)`, which this ADR's own §6 says
   it unblocks; it is not a deep-link-only feature. U3 remains deferred **by decision**, and the
   gap is recorded in `crates/flui-widgets/src/navigator/mod.rs` naming the two upstream cases it
   owes (`'Initial route can have gaps'`, `'The full initial route has to be matched'`) and the
   error branch that disposes every generated route and seeds `/` alone.

### 7.7 `Rc`, not `Arc` — §3.1's `Send + Sync` is stale by one day

§3.1 stores the generator as `Arc<dyn Fn(..) + Send + Sync>`. This ADR is dated **2026-07-10**;
ADR-0027 (owner-affine UI realms), which makes `NavigatorHandle` `!Send + !Sync` by construction,
is dated **2026-07-11**. The requirement predates the ownership model that settles it.

The registry uses `Rc<dyn Fn(&RouteSettings) -> Option<GeneratedRoute>>`. The closure is reachable
only through the handle, which is owner-affine, so `Send + Sync` can never be exercised — and it
costs real ergonomics, since it constrains the generator's *captures* and the natural generator
captures owner-local state (an `Rc` of app state, a cloned `RouteContentBuilder`, itself `Rc`).
Every peer closure alias on this surface is already `Rc`: `RouteContentBuilder`,
`RoutePageBuilder`, `RouteTransitionsBuilder`.

### 7.8 The generator receives the navigator — **REVERSED, see §7.10**

§3.1's factory signature is `Fn(&RouteSettings) -> Option<GeneratedRoute>` — Flutter's
`RouteFactory` shape, transcribed. In Rust that shape has a cost Dart does not pay: a factory that
wants to navigate captures a `NavigatorHandle`, which closes
`Arc<NavigatorShared>` → registry → `Rc<dyn Fn>` → handle → `Arc<NavigatorShared>`, and the route
stack is never reclaimed after unmount. Flutter is immune only because Dart is garbage-collected.
That is not a contract worth inheriting.

**Decision:** the factory takes a `RouteRequest<'_>` carrying the settings *and* the handle
(`request.navigator()`). Capturing a handle then has no remaining justification — and note the
route's own content builder never needed one either: `RouteContentBuilder` receives a
`&dyn BuildContext` and `NavigatorHandle::maybe_of(ctx)` resolves from it, which is exactly what
`Navigator.of(context)` does in the reference. The cycle was a false necessity on both sides.

A consequence worth stating: a factory that calls back into `push_named` becomes the *supported*
shape rather than a hazard, since the registry's `Rc` is cloned out and its guard dropped before
any factory runs. The leak warning survives only as "if you capture a handle anyway".

### 7.9 `RouteKey<T>` — the name carries its result type

A string that loses its type is a 2015 Dart artifact. `Navigator.pushNamed` cannot do better:
Dart erases generics, so its `_routeNamed` re-types through an unchecked `as Route<T?>?`. Rust can.

**Decision:** beside the string path, a typed key.

```rust
const SETTINGS: RouteKey<i32> = RouteKey::new("/settings");
handle.route_keyed(SETTINGS, |_| Some(PageRoute::<i32>::new(page)));
let result: RouteResult<i32> = handle.push_keyed(SETTINGS)?;
```

`route_keyed<T, R>(key: RouteKey<T>, ..)` is bounded `R: NavigatorRoute<Output = T>`, so a
**registration whose route disagrees with its key is a compile error at the registration site**.
That is the guarantee, and it is worth having: §7.3's trade-off disappears for the mismatch a
caller is actually likely to make.

**Correction (2026-09-07, found in re-review): the guarantee is narrower than this ADR first
claimed.** The text here originally said `ResultType` was *unreachable* on the keyed path and that
the residual hazard was a collision "across the typed and untyped registration paths". Both are
wrong, and the second understates it.

The registry is string-keyed at run time, and the compiler checks each registration against *its
own* key — never one key against another. So two keys sharing a name is a **keyed-only**
collision:

```rust
const A: RouteKey<u32>    = RouteKey::new("/order");
const B: RouteKey<String> = RouteKey::new("/order");
```

Both `route_keyed` calls compile, the second replaces the first in the name-keyed table, and
`push_keyed(A)` yields `ResultType`. The accurate statement is: the `TypeId` check guards **two
registration sites disagreeing about one name**, keyed-vs-keyed included — not merely
typed-vs-untyped. Tests cover both orderings.

**And what the check buys was also stated wrongly.** The original text said it prevents a
"silently wrong result". It cannot: `TypedPush::push` pushes first and then downcasts, and the
downcast is type-safe, so without the check the failure is an `expect("BUG: …")` **panic**, never
a wrong value. What the `TypeId` comparison actually buys is a **pre-mutation, non-panicking**
failure — a stronger and more honest claim than the one it replaced.

Closing the collision properly needs a registry key that carries its type, which a string table
cannot express.

**The shipped policy, decided 2026-09-07 (§7.5 of this record), is not open:** the last
registration wins; a re-registration that changes a name's `Output` emits a **latched,
per-navigator** `tracing::warn!` naming the route and both types, at the registration site; and
`push_keyed` on the stale key returns `NamedRouteError::ResultType`. Pinned by
`a_route_re_registered_with_a_different_output_type_warns_once_per_navigator` and
`two_route_keys_sharing_a_name_collide_even_though_both_registrations_compile`.

Rejecting the conflicting registration outright — returning a `Result` from `route_keyed` —
was considered and **declined**: it is invasive at every call site for what is programmer error
at startup, and it contradicts `ARCHITECTURE.md` §6's contract that an app builder replaces the
table wholesale at mount, which makes a deliberate `Output` change between rebuilds legitimate.
The house rule is repair-and-warn. Revisiting that is a future decision, not an open question
in this one.

The string path stays, and is not deprecated: deep links and server-supplied names are genuinely
not known until run time. It simply stops being the only option.

### 7.10 Reversal of §7.8 (2026-09-08) — the window was paying for a problem it created

§7.8 gave a factory a live `NavigatorHandle` to remove an `Arc` cycle. A four-lens review of the
shipped code found the justification **circular**, and the decision is reversed.

**Why it was circular.** §7.8 itself establishes that a route's content builder never needed a
captured handle: it receives `&dyn BuildContext` and reaches the navigator through
`NavigatorHandle::maybe_of(ctx)`, exactly as `Navigator.of(context)` does in the reference. So a
factory never needed to capture one either — there was no cycle to avoid. `RouteRequest::navigator()`
therefore had exactly one consumer: a factory navigating **during resolution**. Confirmed
mechanically — the method had **zero production call sites**; all four were tests of the window
itself.

**What the window cost.** Everything expensive in this feature existed to serve it: the
capture-then-resolve rewrite (§7.2's successor in `ARCHITECTURE.md` §5), `dismiss_captured`'s
three-case fan-out, `RouteEntry::replacing`, **both** documented observer divergences from the
reference, and three of the correctness defects review found — a caller's result silently
discarded, `didReplace` naming a route nothing completed, and an `Option<RouteId>` overload that
made a factory's own new route the replacement target.

**What replaced it.** The factory receives settings, name and arguments — not a handle. A redirect
is expressed by returning a different route, which is what a factory is for; Flutter's
`RouteFactory` is a builder too.

**Correction, made before this section shipped:** an earlier draft of this paragraph claimed
mutation from inside a factory becomes "unexpressible". That is **false**, and it was measured
false rather than argued: a factory is an `Rc<dyn Fn>` and captures freely, so removing the
accessor removes the *advertised* path and not the *possible* one. Reverting the capture-then-
resolve defences on the strength of that claim reproduced the original round-2 defect exactly —
`[root, victim, arrived]` with the caller's route surviving uncompleted.

So the defences **stay**, and their meaning changes rather than their code: the capture is not
overhead left from a capability, it is the invariant *an operation acts on the route the caller
named*, holding whatever ran in between — including code this crate does not advertise. The two
observer divergences `ARCHITECTURE.md` §5 records are likewise re-framed: they are no longer the
price of a chosen capability, they are what still happens if a factory mutates the stack. The
honest word for re-entrancy here is **survivable**, not *supported*.

A runtime guard (`resolving` flag + a `Reentrant` error) was considered as the way to make the
deletion sound, and rejected: a captured handle can call `pop()` / `remove_route()`, which return
`bool` rather than `Result`, so refusing there means a silent no-op, a panic that
`docs/PANIC-POLICY.md` puts on the `Result` side, or a guard covering only the named operations —
which would not close the window and so would not earn the deletion anyway.

**Two lessons, recorded because they outlive this ADR.** First: a capability whose only
justification is to avoid a hazard that the capability itself creates should be examined before it
is built — ask what uses it *besides* the thing it was introduced to fix; here the answer was
"nothing", discoverable by grep on the day it landed. Second, and the one that nearly shipped a
regression: **withdrawing a capability is not the same as making its effect impossible.** Removing
an accessor removes a supported path; a closure that can capture keeps the path reachable. Defences
built for a capability may be defending an invariant, and deleting them with the capability is how
a fixed defect becomes reachable again with its own tests removed.
