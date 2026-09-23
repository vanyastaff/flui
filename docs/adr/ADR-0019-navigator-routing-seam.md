# ADR-0019: Navigator routing seam — an owned handle, a pure-data flush, a `dyn Any` pop result

- **Status:** Accepted. The typed Router (roadmap track A3: routes as a typed tree, the URL as
  the source of truth, push/pop as a thin facade over it) will supersede the Navigator-first
  shape recorded here. The `dyn Any` pop-result decision (§3) is expected to carry over.
- **Date:** 2026-07-09

## Context

Flutter's `Navigator` is an imperative route stack that reaches its `Overlay` through a
`GlobalKey`, calls `setState` from arbitrary callbacks, and delivers a pop result of type `T`
through a `Completer<T?>` stored in a `Route<dynamic>`. FLUI needed the same observable
behavior — lifecycle order, observer order, results that complete on removal as well as on
pop — without Dart's `dynamic`, garbage collector or re-entrant tree access. Reference:
`navigator.dart` (`_flushHistoryUpdates`, `_RouteEntry`, `_RouteLifecycle`), `overlay.dart`,
`routes.dart`.

## Decision

### 1. The history flush is pure data; no new framework seam

All of Flutter's navigator methods only mutate `_history` and call one synchronous flush;
every lifecycle effect happens inside it, and its only tree-visible effect is a deferred
overlay rebuild. FLUI ports it as `RouteHistory`, a pure function over a vector of route
entries that returns the notifications and disposals it decided (see the divergences below),
unit-testable without an element tree. It needs no
build-during-layout style seam: the `'static` `RebuildHandle` (ADR-0018) and the "state
publishes a handle into a shared cell at `init_state`" pattern are enough.

Preserved exactly, because apps observe them:

- lifecycle states as an ordered `#[repr(u8)]` enum; the predicates (`will_be_present`,
  `is_present`, …) are named ranges over declaration order;
- observer notification order: additions drain LIFO, deletions FIFO, all additions first,
  then `did_change_top`; neighbour announcements use a "never announced" sentinel so the
  bottom route gets its initial `did_change_previous(None)`; a dying route receives its final
  announcement before it is disposed, and the overlay is rearranged last;
- `result ?? current_result` on completion, and **a removed route completes its future**
  (`remove_route`, `push_replacement`, `push_and_remove_until`) — otherwise every awaiting
  caller hangs;
- `can_pop`/`maybe_pop` over `RoutePopDisposition`; `maybe_pop` is a plain `fn` (Dart's is
  `async` only for the deprecated `willPop`).

### 2. `Navigator::of` returns an owned handle; no `GlobalKey`

`find_state` yields `&dyn Any` inside a callback while the tree is borrowed, so a mutable
state reference cannot escape and a nested lookup inside the callback would take a second lock.
`Navigator::of`/`maybe_of`/`maybe_of_root` clone an owned `NavigatorHandle` out of the state
inside the callback; every mutation runs after the borrow is released. The route stack lives
behind a private lock in shared state, and navigator and overlay couple through that shared
state, not through the element tree — so Flutter's `GlobalKey<OverlayState>` is not ported.
Neither is `OverlayEntry`'s `GlobalKey`: each entry publishes its own `RebuildHandle`, and a
`ValueKey<OverlayEntryId>` preserves entry state across `rearrange` through keyed
reconciliation.

### 3. The pop result crosses a `dyn Any` boundary

A heterogeneous stack cannot carry each route's `Output`, so it is
`Vec<Box<dyn ErasedRoute>>` and a pop result crosses `Box<dyn Any + Send>`, downcast back to
the route's `Output` on delivery. Flutter has the same runtime failure mode (an unchecked
`pop<T>` on `Route<dynamic>`); Rust would not otherwise need one.

- The erasure is internal. Public front doors are typed: `pop_with<T>`, `remove_route_with<T>`,
  `maybe_pop_with<T>`, plus result-less `pop`, `remove_route`, `maybe_pop`. The type is checked
  at delivery, because the navigator does not know the top route's `Output`.
- **A mismatch logs and completes with `None`; it does not panic.** Flutter throws a cast
  error. A wrong pop type is caller error, not a framework invariant (`docs/PANIC-POLICY.md`).
- `push` returns `RouteResult<T>: Future<Output = Option<T>>`, driven by ADR-0018's
  `AsyncDriver`; push stays usable fire-and-forget.

## Flutter divergences

- **`staging` and `disposing` are omitted** from the lifecycle enum. `staging` serves only
  page-based routing; `disposing` exists because Dart waits a microtask for overlay elements to
  unmount, and FLUI's unmount is synchronous.
- **`PushCompletion::Immediate` settles inside the first flush**; Flutter takes a microtask.
  The observer stream is identical; only the disposal of a route in `Removing` moves one flush
  earlier. `Animating` + `notify_push_completed` is the faithful path transitions use.
- **Routes are named by `RouteId`, not by object**, in observer callbacks and neighbour
  announcements — handing out `&mut dyn ErasedRoute` for one entry while the history holds the
  rest is not expressible.
- **The flush decides, the navigator performs.** `RouteHistory` walks the stack under the
  history lock and returns owned notifications and dying routes in `FlushOutcome`;
  `NavigatorShared::apply` delivers observer callbacks, disposes routes, removes their overlay
  entries and rearranges the overlay with the lock released. Neighbour announcements stay
  under the lock and therefore precede observer callbacks (Flutter: after). An observer may
  read or mutate the stack from any callback (ADR-0021).
- **The overlay entry map lives on the navigator**, keyed by `RouteId`; the route supplies only
  a builder.
- **No self-check in `Navigator::of`.** Flutter's "is `context` the navigator's own element"
  check only fires for a context from `GlobalKey<NavigatorState>.currentContext`, which FLUI
  does not have; during `build` the element's own node is unreachable anyway.
- **Not carried:** `Navigator.build`'s `HeroControllerScope`/`NavigationNotification`/
  pointer-cancelling `Listener`/`FocusTraversalGroup` wrapping, page-based Navigator 2.0,
  restoration, predictive-back user-gesture observers, `replace`/`replaceRouteBelow`.

## Consequences

- Transitions, modal routes and pop scopes build on this (ADR-0020); local history on
  `ModalRoute` (ADR-0025); heroes on the observer API and the overlay (ADR-0021); the overlay's
  public API is ADR-0076.
- A wrong pop result type is a logged `None`, never a crash; the downcast is the one sanctioned
  `dyn Any` boundary in the navigator.

## Alternatives rejected

- **A fully typed pop** (`push` returns a typed handle; `pop()` takes no value). Type-safe, but
  `Navigator::of(ctx).pop(value)` from inside the route's subtree becomes unwritable without
  threading the typed handle down.
- **Porting Flutter's `GlobalKey` couplings.** A `GlobalKey` lookup from inside the ancestor
  walk establishes a lock order between the element tree and the key registry that nothing
  else guarantees.
- **A dedicated overlay/route framework seam.** The flush touches no tree; existing handles
  cover what it needs.
