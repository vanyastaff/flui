# ADR-0020: TransitionRoute / ModalRoute seam — a queued route binding, a real `RenderTheater`, a correct `Offstage`

- **Status:** Accepted
- **Date:** 2026-07-09
- **Superseded in part by:** ADR-0064 (push completion: `did_push` returns a `TickerFuture`
  the navigator awaits)

## Context

ADR-0019 shipped a navigator whose routes appear and vanish instantly, with a route stack that
is pure data: routes are leaves with no navigator, no overlay entry and no clock. Flutter's
`TransitionRoute` needs all three: its controller is vsynced to the navigator, it finalizes
itself from an animation-status listener by calling `navigator.finalizeRoute(this)`, and it
writes its own overlay entry's `opaque` flag on every status change. `ModalRoute` adds a
barrier, `maintainState` and `offstage`, which only mean something if the overlay skips
covered entries. Reference: `routes.dart` (`TransitionRoute`, `ModalRoute`, `PopupRoute`),
`pages.dart`, `overlay.dart` (`OverlayState.build`, `_RenderTheater`), `animations.dart`
(`TrainHoppingAnimation`), `proxy_box.dart` (`RenderOffstage`).

## Decision

### 1. The clock: the navigator owns a `Vsync`

Flutter's `vsync: navigator!` gives one clock per navigator, so an off-screen navigator
freezes all its transitions. FLUI's `AnimationController` builds its own ticker, and
`flui_animation::Vsync` is a registry a binding drives, not a `TickerProvider`. So
`NavigatorState::init_state` resolves an ambient `VsyncScope` and route controllers register
with it; absent one, each controller keeps its own wall-clock ticker (as `AnimatedSize` does).
`VsyncRegistration` has no `Drop`, so a route's `dispose` unregisters explicitly.

### 2. Routes reach the navigator through a queued `RouteBinding`

A route cannot call the navigator directly: the navigator holds its non-reentrant history lock
for the whole flush, and a route finalizing from a status listener inside that flush would
deadlock. Instead:

- `RouteBinding` is an owned capability pre-bound to one `RouteId`: it enqueues a
  `RouteCommand::Finalize` on a queue with its own lock and calls a `wake` closure, and it
  reaches the route's overlay entry (to write `opaque`, `maintain_state` and mark it for
  rebuild). A route can never finalize another. The entrance transition's completion arrives
  on the same queue as `RouteCommand::PushCompleted`, raised by a continuation on the
  `TickerFuture` that `did_push` returns (ADR-0064), not by the route.
- `wake` `try_lock`s the history: between frames the commands apply and flush at once; during a
  flush on this thread, that flush drains the queue before returning. The queue is Flutter's
  `_flushingHistory` deferral expressed as ownership instead of a flag.
- `flush` is a bounded pass loop (apply pending, walk, repeat; `MAX_FLUSH_PASSES`), like the
  layout↔build fixpoint of ADR-0017. The `BUG:` assert on a genuinely recursive flush stays.
- `RouteHistory` never sees the binding, so it stays pure data.

The public door is `NavigatorRoute::binding_slot() -> Option<&RouteBindingSlot>`, defaulted to
`None`. `RouteBindingSlot` is opaque (`new`, `is_bound`); `push` and `seed_initial` fill it
before `install()`. Non-animating routes such as `SimpleRoute` never see one.

### 3. `TransitionRoute` ports `_handleStatusChanged` and its guards

All four status arms: `completed` writes the route's `opaque` to its entry, `forward`/`reverse`
clear it (routes beneath keep building through the transition), `dismissed` finalizes only if
the route is no longer active (a gesture-driven subclass may reach `dismissed` while active).
`finished_when_popped` is `controller dismissed && !pop_finalized`, so a pop starts the reverse
animation, resolves the route's result immediately, and defers disposal until `dismissed`.
`did_add` jumps the controller to the end.

The secondary animation follows the next route's primary animation through a `ProxyAnimation`,
gated by `can_transition_to`/`can_transition_from`, with train-hopping via `AnimationSwitch`
(audited against `TrainHoppingAnimation`: fixed hop direction, collapse without callback when
equal, `on_switched` exactly once, `dispose` detaches from both trains). The proxy is released
by the next route's `completed` signal, guarded by "the proxy still names that route", so a
stale signal cannot clobber a newer parent. `completed` is not `popped`: it resolves at
dispose.

"Is the next train animating" is read from its status (`forward`/`reverse`), not from
`AnimationController::is_animating`, which follows the ticker and can stay true after the
controller has settled.

### 4. The overlay skips covered entries: `RenderTheater`

`Overlay` honours `opaque`, `maintain_state` and a skip count, porting `OverlayState.build`'s
onstage loop as a pure `onstage_plan()`: walk entries top-down until an opaque one, keep only
`maintain_state` entries below it (as a skipped prefix), drop the rest from the tree.
`RenderTheater` (`flui-objects`) is a `StackFit::Expand` stack whose first `skip_count`
children are not laid out, painted or hit-tested; with `skip_count == 0` it is exactly
`Stack(fit: Expand)`. Without this, `maintainState` and `PageRoute.opaque` would be unobservable.

### 5. `RenderOffstage` is corrected, not extended

`RenderOffstage` lays its child out under the real constraints, reports
`constraints.smallest()` for itself, and skips paint, hit-test and semantics — Flutter's
`RenderOffstage`. It previously laid the child out at zero size and reported `Size::ZERO`,
which violated tight constraints and could not measure an offstage page. `ModalRoute.offstage`
(used by heroes, ADR-0021) needs exactly Flutter's behavior, so no new mode was added. Under
tight constraints a hidden `Visibility(maintain_state: true)` now occupies its tight size, as in
Flutter.

### 6. `ModalRoute`: one overlay entry holding barrier and page

Flutter's `ModalRoute` creates two entries, `[_modalBarrier, _modalScope]`. FLUI keys one entry
per `RouteId` (ADR-0019), so `ModalRoute` builds `Stack[barrier, page]` into one entry. The
opacity, `maintain_state` and rebuild that Flutter writes to the barrier or scope entry land on
that one entry; paint and hit-test order are unchanged. Costs: a barrier-only rebuild rebuilds
the page too, and a covered `maintain_state` route keeps its stateless barrier mounted.

The barrier is an `AbsorbPointer` that absorbs within its bounds whether or not it has a
colour, with a `GestureDetector` that calls `maybe_pop` when dismissible. `offstage` hides the
scope, drops the barrier and swaps the route's animation proxies to always-complete /
always-dismissed so heroes measure final geometry. The page is wrapped in
`FocusScope::with_external_node` (ADR-0026).

`ModalScope` is an `AnimatedView` over a relay notifier fed by both animations.

### 7. Public surface: `PageRoute` and `PopupRoute`

`PageRoute<T>` (Flutter's `PageRouteBuilder`; opaque, same-family transitions) and
`PopupRoute<T>` (non-opaque, `maintain_state = true`) are public and closure-configured, with
`RoutePageBuilder`, `RouteTransitionsBuilder`, `RouteAnimation` and `RouteBindingSlot`.
`TransitionRoute`, `ModalRoute`, `RouteBinding` and `ModalHandle` stay private: Rust has no
subclassing, and exporting them as extensible bases means designing a route trait, a decision
of its own. `can_transition_to/from` is a symmetric family test: a `TransitionGroup` on each
route, so a popup over a page drives no secondary animation on it.

## Flutter divergences

- A finalize raised during a flush applies on a second pass of the same `flush`, not
  mid-walk; the end state and observer stream are identical before `flush` returns.
- The page builder re-runs on every transition tick (Flutter caches `_page`); reconciliation
  keeps the page's state, so this is cost, not behavior.
- A covered `maintain_state` entry keeps ticking (no `tickerEnabled: false`) and is still in
  the semantics tree (no per-child semantics visitor).
- `canSizeOverlay`/`alwaysSizeToContent` are not ported; under unbounded constraints
  `RenderTheater` falls back to `constraints.smallest()` instead of throwing.
- `TransitionRoute.opaque` defaults to `false`.
- **Not carried:** `didReplace` controller-value inheritance, `BlockSemantics` and barrier
  semantics, `barrierLabel`, `AnimatedModalBarrier`/`barrierCurve`,
  `IgnorePointer(ignoring: !isForwardOrCompleted)`, `filter`/`BackdropFilter`,
  `fullscreenDialog`, `allowSnapshotting`, delegated transitions, `_ModalScopeStatus` aspects.

## Alternatives rejected

- **A direct navigator callback from the route** (`install(&mut self, binding)` calling into
  the navigator). Deadlocks on the history lock, and threading the binding through the public
  `Route::install` would have made it public API.
- **Each route owning an `Arc<Scheduler>`.** Decouples transitions from the navigator's clock;
  an off-screen navigator would keep animating.
- **Shipping `ModalRoute` without `opaque`/`maintain_state`.** Exports fields that do nothing
  and costs O(stack depth) layout and paint for every full-screen route.
- **A new "laid out but unpainted" offstage mode.** Flutter's `Offstage` already is that mode;
  FLUI's implementation was wrong.
