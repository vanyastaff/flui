# ADR-0020: `TransitionRoute` needs **three back-references FLUI's `Route` does not have** — to a ticker, to the navigator, and to its own overlay entry — and one render object is quietly wrong: `RenderOffstage` does not do what Flutter's does

*ADR-0019 shipped a `Navigator` whose routes appear and vanish instantly. Making them animate looks like "add an `AnimationController`", and it is not. Flutter's `TransitionRoute` owns a controller vsynced to the **navigator**, finalizes itself from an **animation status listener** by calling back into the navigator, and mutates its own **overlay entry's `opaque` flag** every time the status changes. FLUI's `Route` is a leaf: it has no navigator, no overlay entry, and no clock. ADR-0019 deliberately made it that way — the route stack is pure data — so U5 is not a widget task, it is a seam task. This ADR reads the reference, names the five seams, records that `RenderOffstage` diverges from Flutter today (and that ADR-0019 §3.5 mis-stated why), and stages the work so the erased/animated surface is signed off before any of it is public.*

---

- **Status:** Accepted — **U5.0 landed 2026-07-09** (`RenderOffstage` parity fix). U5.1–U5.5 are unstarted; no `TransitionRoute`, no public API.
- **Date:** 2026-07-09
- **Deciders:** chief-architect; consult animation owner (ticker ownership: who is FLUI's `vsync:`), view owner (route → navigator and route → overlay-entry back-references, both of which ADR-0019 deliberately omitted), rendering owner (`RenderOffstage` correction, `maintainSize`), repository owner (any public API — `TransitionRoute`/`ModalRoute`/`PageRoute` shape, and whether `Overlay` becomes public), qa-lead (deterministic transition tests: driving a controller inside a headless frame).
- **Relates to:** implements ADR-0019 §5 **U5**. Depends on the seams ADR-0019 U2 already carved out (`PushCompletion::Animating`, `Route::finished_when_popped`, `RouteHistory::notify_push_completed`) — all of which currently have **no production producer**.
- **Blocks:** route animation; modal dialogs; `PageRoute`. Transitively blocks `Hero` (tracker B1.4), which additionally needs `ModalRoute::offstage`.
- **Gate:** ARCH-GATE (this doc) → DEV-GATE per slice → **parity + sign-off gate before public export** (U5.4), per the [Definition of Done](../../AGENTS.md#definition-of-done-anti-cheating).

---

## Reference

Cross-checked against `.flutter/`, Flutter master **`3.33.0-0.0.pre-6280-g88e87cd963f`** (the revision used by ADR-0017 U4, ADR-0018 U6 and ADR-0019 U4):

| File | Read for |
|------|----------|
| `packages/flutter/lib/src/widgets/routes.dart` | `TransitionRoute` (`:111`), `ModalRoute` (`:1266`), `_ModalScope`, `PopupRoute` (`:2380`) |
| `packages/flutter/lib/src/widgets/navigator.dart` | `NavigatorState`'s mixins (`:3745`), `finalizeRoute` (`:5798`), `handlePush`'s `whenCompleteOrCancel` (`:3274-3290`) |
| `packages/flutter/lib/src/widgets/modal_barrier.dart` | `ModalBarrier`, `AnimatedModalBarrier`, `_AnyTapGestureRecognizer` |
| `packages/flutter/lib/src/widgets/pages.dart` | `PageRoute` (`:23`) |
| `packages/flutter/lib/src/animation/animations.dart` | `TrainHoppingAnimation` (`:503-622`) |
| `packages/flutter/lib/src/rendering/proxy_box.dart` | `RenderOffstage` (`:3834-3952`) |
| `test/widgets/{routes,navigator,modal_barrier}_test.dart` | The behavioral oracles in §5 |

Every statement about Flutter below is read from those files at that revision, with line citations. Statements about FLUI are read from the working tree at commit `3fdffa76`. Where I could not verify something, it is labelled **`UNVERIFIED`**.

---

## 1. The contracts U5 must preserve

### 1.1 The controller is vsynced to the *navigator*, not the route

```dart
class NavigatorState extends State<Navigator> with TickerProviderStateMixin, RestorationMixin {  // :3745
```

```dart
AnimationController createAnimationController() {            // routes.dart:232-242
  return AnimationController(duration: …, reverseDuration: …, vsync: navigator!);
}
```

Consequences a reimplementer gets wrong: the controller ticks only while the **navigator's** ticker is enabled (so an off-screen navigator freezes every route transition at once), and the route — which is not in the element tree — never needs a ticker of its own. `transitionDuration` is abstract; `reverseTransitionDuration` defaults to it (`:140`, `:148`).

The controller is created in `install()`, **not** the constructor (`:326`), and disposed in `dispose()` only if `willDisposeAnimationController` (`:207`, default `true`).

### 1.2 `install()` ordering, and the already-completed special case

```dart
void install() {                                             // routes.dart:323-334
  _controller = createAnimationController();
  _animation = createAnimation()..addStatusListener(_handleStatusChanged);
  super.install();                                           // OverlayRoute: creates overlay entries
  if (_animation!.isCompleted && overlayEntries.isNotEmpty) {
    overlayEntries.first.opaque = opaque;                    // didAdd jumped the controller to 1.0
  }
}
```

The trailing `if` exists because `didAdd` sets `_controller.value = upperBound` (`:352-361`) and no status callback fires for a value already at the end.

### 1.3 `_handleStatusChanged` — the route's whole engine

```dart
void _handleStatusChanged(AnimationStatus status) {           // routes.dart:293-321
  switch (status) {
    case AnimationStatus.completed:
      if (overlayEntries.isNotEmpty) { overlayEntries.first.opaque = opaque; }
      …
    case AnimationStatus.forward:
    case AnimationStatus.reverse:
      if (overlayEntries.isNotEmpty) { overlayEntries.first.opaque = false; }
      …
    case AnimationStatus.dismissed:
      if (!isActive) {                                        // ← the guard
        navigator!.finalizeRoute(this);
        _popFinalized = true;
      }
  }
}
```

Three separate contracts hide here:

1. **The route mutates its own overlay entry's `opaque` flag.** Forced `false` while animating (so routes beneath keep building and show through the transition), set to the route's `opaque` when the entrance completes.
2. **The route finalizes itself by calling back into the navigator**, from an animation callback — not from `didPop`.
3. **The `!isActive` guard.** A subclass (the iOS back-gesture) drives the controller to `dismissed` while the route is still active; finalizing there would be wrong. Naively finalizing on every `dismissed` double-finalizes.

### 1.4 `finishedWhenPopped` defers disposal through the animation

```dart
bool get finishedWhenPopped => _controller!.isDismissed && !_popFinalized;   // routes.dart:177-178
```

vs `OverlayRoute.didPop` (`routes.dart:87-94`), which calls `navigator!.finalizeRoute(this)` **only if** `finishedWhenPopped`.

So the sequence on a pop is: `TransitionRoute.didPop` starts the **reverse** animation, then calls `super.didPop`; at that instant the controller is running, `finishedWhenPopped` is `false`, and nothing finalizes. The route's `popped` future resolves *immediately* (via `didComplete`); its overlay entries and `dispose()` wait for `dismissed`. The `!_popFinalized` term prevents a double-finalize when the controller was **already** dismissed at pop time (the Cupertino dismiss gesture — the source comments this at `:173-176`).

**This is precisely the deferral point ADR-0019 U2 carved out**: `handle_pop` sends the entry straight to `Dispose` iff `finished_when_popped()`. The seam exists; nothing produces `false` yet.

### 1.5 `didPush` returns a `TickerFuture`, and the navigator waits on it

`didPush()` (`:336-350`) returns `_controller.forward()`. `_RouteEntry.handlePush` (`navigator.dart:3274-3290`) parks the entry in `pushing` and attaches `routeFuture.whenCompleteOrCancel(...)`, which flips it to `idle` and re-flushes.

**ADR-0019 U2 already models this**: `PushCompletion::Animating` parks in `Pushing`, and `RouteHistory::notify_push_completed(id)` is the `whenCompleteOrCancel` analogue. Both are implemented. `notify_push_completed` is `#[cfg(test)]` with **no production caller** — U5 is that caller.

Also: `didAdd` jumps to `upperBound` (no animation); `didReplace(old)` **inherits the replaced route's controller value** (`:363-374`), so a replacement does not restart the transition.

### 1.6 `secondaryAnimation`, and train-hopping

`animation` is the raw controller view (`createAnimation()` → `_controller.view`, `:247-251`). **Only `secondaryAnimation` is a `ProxyAnimation`** (`:197-198`, seeded with `kAlwaysDismissedAnimation`). Re-proxying the primary would break the status-listener identity `_handleStatusChanged` depends on.

`_updateSecondaryAnimation(nextRoute)` (`:422-496`), driven from `didChangeNext` (`:404-413`) and `didPopNext` (`:393-402`), points this route's `secondaryAnimation` at the **next route's primary animation**, so a route can animate *out* as another animates *in*. Gated by:

```dart
nextRoute is TransitionRoute && canTransitionTo(nextRoute) && nextRoute.canTransitionFrom(this)   // :429-431
```

Otherwise the proxy is reset to `kAlwaysDismissedAnimation` (`:491`). Both predicates default `true` (`:536`, `:561`); `PageRoute` narrows them to "the other route is also a `PageRoute`" (`pages.dart:57-60`).

**Train-hopping** is what happens when the two animations are at *different* values and cannot be snapped. `TrainHoppingAnimation(currentTrain, nextTrain, onSwitchedTrain:)` (`animations.dart:503-622`) proxies `currentTrain`, and on the first tick where the two cross — in a direction fixed at construction (`maximize` if `current.value > next.value`, else `minimize`, `:523-528`) — it atomically swaps to `nextTrain` and fires `onSwitchedTrain` **exactly once**. If the values are already equal at construction it collapses immediately and never fires. Its `value`/`status` are always the current train's.

The route layer's state machine around it (`:426-496`):

- **Values already equal, or the next train is not animating** → jump straight (`_setSecondaryAnimation(nextTrain, nextRoute.completed)`).
- **Values differ and the next train is animating** → create the hopper, install it as the proxy parent, and register two teardowns: `onSwitchedTrain` (swap the proxy to point *directly* at the target, dropping the hopper) and `jumpOnAnimationEnd` (if the next train **stops before the trains ever cross**, snap to it and tear the hop down).
- **A new `_updateSecondaryAnimation` arrives mid-hop** → the previous hop's `_trainHoppingListenerRemover` is captured first and invoked **last** (`:495`), after the replacement is installed. You cannot dispose the old hopper before its successor exists.
- **The next route goes away** → `_setSecondaryAnimation`'s optional `disposed` future (`nextRoute.completed`) resets the proxy to `kAlwaysDismissedAnimation`, **guarded by `_secondaryAnimation.parent == animation`** (`:503`) so a stale future cannot clobber a newer parent.

That guard and that ordering are the two things a reimplementation drops, and both produce use-after-dispose.

### 1.7 `completed` is not `popped`

`completed` (`:115-122`) resolves in `dispose()` (`:637`). `popped` resolves the moment the pop is accepted. The doc says so explicitly: `completed` fires *after* `popped`, once the route is fully gone. `_updateSecondaryAnimation` consumes `nextRoute.completed` as its cleanup signal.

### 1.8 `ModalRoute`: two overlay entries, barrier below content

```dart
Iterable<OverlayEntry> createOverlayEntries() {               // routes.dart:2349-2359
  return <OverlayEntry>[
    _modalBarrier = OverlayEntry(builder: _buildModalBarrier),
    _modalScope   = OverlayEntry(builder: _buildModalScope,
                                 maintainState: maintainState, canSizeOverlay: opaque),
  ];
}
```

The overlay paints later entries on top, so **the scope sits above the barrier**. Note what carries what: `maintainState` lands on the *scope* entry; the route's `opaque` feeds the scope's `canSizeOverlay`, **not** the entry's `opaque` flag — that one is written by `_handleStatusChanged` on `overlayEntries.first`, i.e. the **barrier**.

> A reimplementer will assume `overlayEntries.first` is the content. It is the barrier.

**The barrier blocks input with an opaque hit-test, not `AbsorbPointer`.** `ModalBarrier.build` (`modal_barrier.dart:207-270`) returns `BlockSemantics(ExcludeSemantics(_ModalBarrierGestureDetector(…)))`, where the detector is a `RawGestureDetector` with `HitTestBehavior.opaque` (`:435-442`) recognising *any* tap button (`_AnyTapGestureRecognizer`, `:370-406`), on tap-**up**. On tap: if `dismissible`, `onDismiss?.call()` else `Navigator.maybePop(context)`; if not dismissible, it plays a system alert sound — **and still absorbs the tap**. `AnimatedModalBarrier` differs only in reading an `Animation<Color?>` per tick (`:352-363`).

`buildModalBarrier` (`routes.dart:2298-2328`) picks the animated barrier only when `barrierColor != null && alpha != 0 && !offstage`; the scrim colour is `animation.drive(ColorTween(transparent → barrierColor).chain(CurveTween(barrierCurve)))`. The wrapper (`:2273-2288`) adds `IgnorePointer(ignoring: !animation.isForwardOrCompleted)` so the barrier stops absorbing once the route starts reversing.

### 1.9 `offstage` — and what it actually is

```dart
set offstage(bool value) {                                   // routes.dart:1949-1963
  …
  _animationProxy!.parent = _offstage ? kAlwaysCompleteAnimation : super.animation;
  _secondaryAnimationProxy!.parent = _offstage ? kAlwaysDismissedAnimation : super.secondaryAnimation;
  changedInternalState();
}
```

Set by `HeroController` for one frame so heroes can be measured **at their final positions** before the visible transition starts. Note it re-proxies `ModalRoute`'s *own* animation proxies (which shadow `TransitionRoute`'s) to always-complete/always-dismissed, so the subtree builds as if the transition were finished.

For that to work, `Offstage` must lay the child out **at real size** while skipping paint. Flutter's `RenderOffstage` (`proxy_box.dart:3894-3944`):

```dart
bool get sizedByParent => offstage;                          // :3896
Size computeDryLayout(c) => offstage ? c.smallest : super…;  // :3905-3910
void performLayout() { if (offstage) { child?.layout(constraints); } else { super.performLayout(); } }
bool hitTest(…) => !offstage && super.hitTest(…);
void paint(…) { if (offstage) return; super.paint(…); }
```

The **child** is laid out under the real constraints. The **Offstage box itself** reports `constraints.smallest`. Paint, hit-test and semantics are skipped.

### 1.10 `maintainState`

Abstract on `ModalRoute` (`:1893`); wired onto the scope entry at `createOverlayEntries` and re-synced on every `changedInternalState` (`:2230`). Observable only because `Overlay` skips building entries below the topmost **opaque** entry: a covered `maintainState: false` route is unmounted and its `State`s disposed; `true` keeps it mounted. `PopupRoute` forces `true` (`:2394`); `PageRoute` leaves it abstract.

### 1.11 `PopupRoute` / `PageRoute`

`PopupRoute` (`:2380-2398`) overrides exactly three things: `opaque => false`, `maintainState => true`, `allowSnapshotting => false`. `PageRoute` (`pages.dart:23-67`): `opaque => true`, `barrierDismissible` (default `false`), `allowSnapshotting`, `fullscreenDialog`, and the `canTransitionTo/From` narrowing to `PageRoute`.

---

## 2. The seams FLUI is missing

ADR-0019's central result was that `Navigator` needed **no new seam**. U5 is the opposite: it needs five, and three of them are back-references ADR-0019 removed **on purpose** to keep the route stack pure data. That tension is the whole design problem, and §3 resolves it.

### Seam 1 — a route has no clock. `NavigatorState` is not a `TickerProvider`.

Flutter: `NavigatorState … with TickerProviderStateMixin`, and `vsync: navigator!`.

FLUI: `NavigatorState` holds `overlay: OverlayHandle` and `entries: Mutex<HashMap<RouteId, OverlayEntry>>` and nothing else. It reaches no frame clock. And the two things named "vsync" in FLUI are **not the same thing**:

- `flui_scheduler::TickerProvider` (`ticker.rs:104`) is the real ticker trait — and its **only production implementor is `Scheduler` itself** (`scheduler.rs:1679`).
- `flui_animation::Vsync` (`vsync.rs`) is **not** a `TickerProvider`. It is a restart-aware *registry* of `AnimationController`s that a binding drives with `tick_all(now_secs)`.

`AnimationController::new(duration, scheduler: Arc<Scheduler>)` (`controller.rs:200`) takes a concrete `Arc<Scheduler>` and builds its own ticker. A widget reaches a `Vsync` through the `VsyncScope` inherited view; **a `Route` is not in the element tree and has no `BuildContext`**, and `BuildContext` exposes no `scheduler()` or `vsync()` regardless.

So U5 must decide *who* provides the clock. See §3, Decision 1.

### Seam 2 — a route cannot call back into the navigator

Flutter's `Route` has `_navigator`, and `TransitionRoute` uses it for `navigator!.finalizeRoute(this)` and `vsync: navigator!`.

FLUI's `RouteRecord<R>` is `{ id, route, completer, installed }`. **There is no navigator back-reference**, by design: ADR-0019 §7b records that routes are named by `RouteId` precisely so the stack stays a pure function over a `Vec`, enforced by `route_stack_flush_is_pure_data`.

Without it, a `TransitionRoute` cannot:

- call `notify_push_completed(id)` when its entrance animation finishes (the `whenCompleteOrCancel` analogue — implemented, `#[cfg(test)]`, no production caller);
- call `finalizeRoute(self)` from the `dismissed` branch of its status listener.

### Seam 3 — a route cannot reach its own overlay entry

Flutter's `TransitionRoute` writes `overlayEntries.first.opaque` on every status change, and `ModalRoute.changedInternalState` calls `_modalBarrier.markNeedsBuild()`.

FLUI put the entry on the navigator, not the route (`NavigatorState.entries: RouteId -> OverlayEntry`), for the pure-data reason above. `OverlayEntry::mark_needs_build` **exists** (`overlay/entry.rs`, `pub(crate)`) and is **unused in production** — ADR-0019 U1 left it waiting for exactly this consumer. A `Route` impl cannot reach it.

Mitigating fact: FLUI does not need `opaque` today (Seam 5), and per-frame *content* rebuilds have a widget-layer answer — `AnimatedView` / `AnimatedBuilder` inside the route's `content_builder` already rebuild on a `Listenable` tick, needing no route → entry link. So Seam 3 is only strictly required for the barrier rebuild and for `opaque`.

### Seam 4 — `RenderOffstage` does not do what Flutter's does, and its comment says it does

This is a **defect**, not a gap. `crates/flui-objects/src/interaction/offstage.rs`:

```rust
if self.offstage {
    // Lay out the child at zero size so its layout state stays
    // valid (Flutter parity — the child is still part of the
    // tree, just collapsed). We then report Size::ZERO to the
    // parent.
    let _ = ctx.layout_child(0, BoxConstraints::tight(Size::ZERO));
    …
    Size::ZERO
}
```

Flutter lays the child out under the **real** constraints and reports `constraints.smallest` for the box itself (`proxy_box.dart:3896-3925`). Two divergences:

1. **The child is laid out at zero size**, so it never reaches its real geometry. Flutter's offstage child is fully laid out — that is the entire point, and it is what `ModalRoute.offstage` exists to exploit.
2. **The box reports `Size::ZERO` rather than `constraints.smallest`.** Under *tight* constraints those differ — `constraints.smallest` is the tight size — so FLUI's `RenderOffstage` **violates its constraints** when offstage under a tight parent.

The comment claiming "Flutter parity" is wrong, and ADR-0019 §3.5 inherited that error: it recorded that FLUI needs a *new* "laid out at real size but unpainted" mode. It does not. **Flutter's `Offstage` already is that mode**; FLUI's implementation of it is simply incorrect. Correcting `RenderOffstage` is a small, self-contained fix that removes a Hero blocker — and it is a **behavioral change to a shipped public widget**, so it needs its own harness evidence.

**Consumers, checked.** `Visibility(maintain_state: true)` wraps its child in `Offstage` (`visibility.rs:8,22`), and `SliverOffstage` / `RenderSliverOffstage` is a separate render object that may carry the same defect (`UNVERIFIED` — not read for this ADR). Correcting `RenderOffstage` changes `Visibility`'s hidden child from "laid out at zero" to "laid out at real size, box reports `constraints.smallest`". Under *loose* constraints `smallest` is zero, so nothing changes; under *tight* constraints the hidden `Visibility` would begin occupying its tight size.

That is not a regression — **it is what Flutter does**, for the same reason (`sizedByParent => offstage`, `computeDryLayout => constraints.smallest`). Flutter's `Visibility` builds on `Offstage` too. So the fix aligns `Visibility` with the reference as a side effect, and any FLUI test asserting a tight-constrained hidden `Visibility` is zero-sized is asserting a bug. U5.0 must find those tests and update them deliberately, not silently.

### Seam 5 — `Overlay` has no `opaque` / `maintainState` / `skipCount`

ADR-0019 U1 deferred these deliberately and pinned the deferral with `overlay_deferred_opaque_builds_every_entry`. Consequences for U5, stated plainly:

- `TransitionRoute`'s `overlayEntries.first.opaque = …` has **nothing to write to**.
- `ModalRoute.maintainState` is **unobservable**: nothing is ever unbuilt, so `false` cannot destroy a covered route. A `maintainState: false` test would be a lie.
- Every route beneath a modal is still built, laid out, painted **and hit-tested**. Input isolation must therefore come from the barrier, exactly as ADR-0019 §2.4 warned — it cannot lean on covered layers being skipped.

### Seam 6 — no `Focus` / `FocusScope` **widget**

`FocusManager`, `FocusNode`, `FocusScopeNode` exist and are public (`flui-interaction`), with `traps_focus` and `set_active_scope` already there. Tracker **H4** marks focus "done" — but scoped to the node/manager layer. **There is no widget that attaches a `FocusScopeNode` to an element and makes it the active scope on mount.** `ModalRoute`'s `FocusScope.withExternalFocusNode` has no analogue.

### Seam 7 — no `BlockSemantics`, no `maintainSize`

`ExcludeSemantics` exists; `BlockSemantics` does not (zero occurrences). So a modal can drop its barrier from the semantics tree but **cannot occlude the semantics of the routes beneath it** — an accessibility gap, not a visual one. `maintainSize` likewise does not exist (and `Visibility::maintain_interactivity` is a documented no-op).

### What already exists, and is enough

| Need | FLUI today |
|---|---|
| `AnimationController` with `forward`/`reverse`/`animate_with`/`animate_back_with`, status + value listeners | `flui-animation`, public |
| `ProxyAnimation` with a **runtime parent swap** | `proxy.rs:150` `set_parent` |
| `kAlwaysDismissedAnimation` / `kAlwaysCompleteAnimation` | `ALWAYS_DISMISSED` / `ALWAYS_COMPLETE` (`constant.rs`) |
| `TrainHoppingAnimation` | **`AnimationSwitch`** (`switch.rs:65`), documented as its analogue — `new(current, next)`, `on_switched`, `current()`, `dispose()` |
| Rebuild-per-tick | `AnimatedView` (`listenable()`), `AnimatedBuilder` |
| Barrier input blocking + tap | `AbsorbPointer`, `GestureDetector::on_tap` (needs a `GestureArenaScope`) |
| `CurvedAnimation`, `ReverseAnimation`, `CompoundAnimation`, `Tween` | `flui-animation`, public |
| The push-deferral seam | `PushCompletion::Animating` + `RouteHistory::notify_push_completed` (`#[cfg(test)]`) |
| The pop-deferral seam | `Route::finished_when_popped` → `handle_pop` sends to `Dispose` iff true |

Two of these deserve emphasis. **`AnimationController` does not return a `TickerFuture` from `forward()`** (it returns `Result<(), AnimationError>`), and there is **no `whenCompleteOrCancel`**. Completion is observed only through a status listener. That is fine — `notify_push_completed` is a status-listener-driven seam by construction — but it means `did_push()` cannot return a future, and `PushCompletion::Animating` is the right shape rather than a workaround.

And **`AnimationSwitch` was audited against `TrainHoppingAnimation` for this ADR, and is faithful.** It fixes a `SwitchMode::{Minimize, Maximize}` at construction from the initial values (`switch.rs:122-124`, matching `animations.dart:523-528`); it collapses to `next` *without* firing the callback when the values are already equal (`switch.rs:104`, matching `:520-522`); its hop predicate is the same crossing test (`switch.rs:237-238` vs `:571-574`); and `value`/`status` delegate to the current train (`switch.rs:329,334` vs `:562,596`). Its `status_listeners_survive_the_hop` test pins the listener re-attachment.

Two things still to confirm in U5.2, and stated as work rather than assumed: that `on_switched` fires **exactly once**, and that `dispose()` detaches from *both* trains (`animations.dart:601-613`) — the route layer relies on that in three separate teardown paths.

---

## 3. Decisions

### Decision 1 — the clock: `NavigatorState` gets a `Vsync`, and the route gets a `RouteTicker`

Flutter's `vsync: navigator!` exists so that (a) one clock governs all of a navigator's transitions and (b) routes stop ticking with the navigator. Both properties are worth keeping.

**Proposed:** `NavigatorState` acquires a `Vsync` in `init_state` — either from an ambient `VsyncScope` (matching how `Scrollable` and `AnimatedSize` already do it) or, absent one, a binding-provided default. It hands routes an owned, `'static` `RouteTicker` capability at `install()` time.

This is the **same shape** as `RebuildHandle` (ADR-0018) and `NavigatorHandle` (ADR-0019 §3.2): a `ViewState` publishes an owned capability that outlives any borrow of it. It does **not** require `NavigatorState` to be a `TickerProvider`, and it does not require `BuildContext` to grow a `scheduler()`.

Rejected: giving each `Route` an `Arc<Scheduler>` and letting it build its own controller. It works, but it decouples route transitions from the navigator's ticker, so an off-screen navigator would keep animating — a real divergence for no gain.

### Decision 2 — a single `RouteBinding`, handed to the route at `install()`

Seams 2 and 3 are the same seam wearing two hats. Rather than give `Route` a `&Navigator` (impossible: `&self` methods, `Mutex`, no `&mut NavigatorState` obtainable — ADR-0019 §3.2), `install()` becomes:

```
fn install(&mut self, binding: &RouteBinding)   // was: fn install(&mut self)
```

where `RouteBinding` is an owned, `'static`, `Clone` capability exposing exactly three things:

- `ticker()` → the `Vsync`/`RouteTicker` of Decision 1;
- `notify_push_completed()` and `finalize()` — the two navigator callbacks, pre-bound to *this* route's `RouteId`, so a route can never finalize another;
- `mark_entry_needs_build()` — pre-bound to this route's overlay entry.

**This does not reintroduce a navigator back-reference into the route stack.** `RouteBinding` holds `Arc`s to the navigator's shared state and to the entry, exactly as `NavigatorHandle` does; `RouteHistory` never sees it, and `route_stack_flush_is_pure_data` keeps holding. The pure-data property ADR-0019 established is preserved because `RouteBinding` lives in `navigator.rs`, not in the stack.

**Re-entrancy is the danger.** `finalize()` and `notify_push_completed()` mutate `RouteHistory` and call `flush()`. Flutter guards this with `_flushingHistory` + `_debugLocked`, and ADR-0019 U2's `flush()` already `assert!`s on re-entry with a `BUG:` message — a guard whose test is direct because "through U2's surface re-entrancy is structurally unreachable." **U5 makes it reachable.** A zero-duration transition completes *inside* the flush that started it, and `_handleStatusChanged` calls `finalizeRoute` synchronously.

Flutter survives this because `finalizeRoute` checks `_flushingHistory` and defers (`navigator.dart:5813-5828`). **U5.1 must port that deferral, and the existing `assert!` must not simply be removed.** This is the single highest-risk item in U5, and it is why U5.1 exists as its own slice.

### Decision 3 — `opaque` / `maintainState` are implemented in U5.3, or **not claimed at all**

`ModalRoute` without `Overlay.opaque` is a `ModalRoute` whose `maintainState` does nothing. Two honest options:

- **(a)** Implement `opaque`/`maintainState`/`skipCount` in `Overlay` (U5.3), which means a real `RenderTheater` with an offstage-skipping `performLayout` — the "no new render object" result of ADR-0019 §2.2 expires here, and `overlay_deferred_opaque_builds_every_entry` goes red **by design**.
- **(b)** Ship `ModalRoute` **without** `opaque`/`maintainState`, delete both from its surface, and say so. Every route stays built; a full-screen modal costs `O(depth)` layout and paint.

**Recommendation: (a), in U5.3.** Option (b) means exporting a `ModalRoute` whose `maintainState` field is a lie, and a `PageRoute` whose `opaque => true` has no effect. Reviving `skipCount` is bounded work — ADR-0019 §2.2 documented the exact upgrade path — and it is what makes `maintainState` testable rather than aspirational.

### Decision 4 — fix `RenderOffstage`; do not add a mode

Seam 4. Correct `perform_layout` to `child.layout(constraints)` + report `constraints.smallest`, matching `proxy_box.dart:3896-3925`. Correct the comment. Add harness tests for both the tight- and loose-constraint cases. Audit `Visibility(maintain_state: true)` for dependence on the old behavior **before** changing it.

### Decision 5 — nothing is public until U5.4

`TransitionRoute`, `ModalRoute`, `PageRoute`, `PopupRoute` and any `Overlay` surface stay `pub(crate)` until the parity + sign-off gate. Precedent: ADR-0017 U4, ADR-0018 U6, ADR-0019 U4 — where the U4 gate caught a real bug that three prior units had missed.

---

## 4. Implementation sequence

The task's suggested order is right in outline; the reference forces two changes. `RenderOffstage` moves **first** (it is a defect in shipped code and blocks nothing else), and the re-entrancy deferral moves into U5.1 (it is not a `TransitionRoute` detail — it is a `RouteHistory` contract that `TransitionRoute` merely exposes).

| Unit | Scope | Exit gate |
|------|-------|-----------|
| **U5.0** ✓ | **`RenderOffstage` correction** (Decision 4). **Landed 2026-07-09** — see §7a. Child laid out under real constraints; box reports `constraints.smallest`; paint/hit-test skipped, and semantics suppression *added* (it was missing). `SliverOffstage` audited: **does not share the defect**. | ✓ 6 harness tests + 1 widget test, each red-checked |
| **U5.1** | **The route-animation seam, no `TransitionRoute`.** `NavigatorState` acquires a `Vsync`; `RouteBinding` (Decision 2) minted at `install()`; `install(&mut self, binding)` signature change; `notify_push_completed` loses `#[cfg(test)]` and gains a production caller path; **`RouteHistory::flush` re-entrancy deferral ported** (`navigator.dart:5813-5828`) so a synchronous finalize during a flush is queued rather than asserting. No animation yet. | The existing `reentrant_flush_panics_with_bug` is *replaced* by a deferral test; `route_stack_flush_is_pure_data` still green |
| **U5.2** | **`TransitionRoute`, private.** Controller lifecycle (`install`/`dispose`, `willDisposeAnimationController`), `_handleStatusChanged` (all four arms incl. the `!isActive` guard and `_popFinalized`), `finished_when_popped`, `didAdd`/`didReplace` value inheritance, `secondaryAnimation` + `canTransitionTo`/`canTransitionFrom`, and train-hopping via `AnimationSwitch` — **after** auditing `AnimationSwitch` against `TrainHoppingAnimation` (§2). `completed` future. | §5 tests 1–7, 11 |
| **U5.3** | **`Overlay` `opaque`/`maintainState`/`skipCount`** (Decision 3) — a real `RenderTheater`; `overlay_deferred_opaque_builds_every_entry` deliberately goes red and is replaced. **Then `ModalRoute`, private:** two entries (barrier below scope), `buildPage` cached once, `buildTransitions`, `offstage`, `setState`/`changedInternalState`/`changedExternalState`, barrier via `GestureDetector`+opaque hit-test. **Focus is deferred and named** (Seam 6): no `FocusScope` widget exists, and faking it is the Definition-of-Done failure mode. | §5 tests 8–10, 12–14 |
| **U5.4** | **`PageRoute` / `PopupRoute` + parity re-check + sign-off, then public export.** Decide whether `Overlay`/`OverlayEntry` become public (they must, if app authors are to write custom routes — or `NavigatorRoute::content_builder` must stay the only door). | Full §5 suite; ADR gains a *Parity findings (U5.4)* table |
| **U5.5** | Tracker flip; `Hero` unblocked and handed to its own ADR. | — |

---

## 5. Tests that will prove the behavior

Each is red-checkable. `«»` names are real Flutter oracles.

**Transition lifecycle (U5.2)**

1. `push_transition_holds_the_previous_route_until_it_settles` — the entry parks in `Pushing`; the route beneath is not disposed. *Red-check:* return `PushCompletion::Immediate` from `did_push`. Pinned already at the stack layer by `animating_push_defers_disposal_of_the_replaced_route_until_it_completes`.
2. `pop_transition_keeps_the_popped_route_until_dismissed` — after `pop`, the route is still in the history, `state == Popping`, its overlay entry still present; its `RouteResult` has **already** resolved. *Red-check:* `finished_when_popped => true`.
3. `finished_when_popped_false_defers_disposal_until_the_controller_dismisses` — drive the controller to `dismissed`; only then does `dispose()` run and the entry leave the overlay. *Red-check:* finalize in `did_pop` instead of the status listener.
4. `already_dismissed_controller_finalizes_synchronously_on_pop` — the `_popFinalized` term (`routes.dart:173-178`). *Red-check:* drop `&& !_popFinalized`; the route double-finalizes.
5. `dismissed_while_still_active_does_not_finalize` — the `!isActive` guard (`:314`). *Red-check:* remove the guard.
6. `did_add_jumps_the_controller_to_the_end_without_animating`; `did_replace_inherits_the_replaced_routes_controller_value` (`:352-374`).
7. `reverse_transition_duration_defaults_to_transition_duration` — «`reverseTransitionDuration defaults to transitionDuration`», «`reverseTransitionDuration can be customized`».

**Secondary animation + train-hopping (U5.2)**

8. `secondary_animation_receives_the_next_routes_primary_animation` — push B over A; A's `secondary_animation.value` tracks B's. *Red-check:* never call `_update_secondary_animation` from `did_change_next`. «`pushReplacement triggers secondaryAnimation`», «`pushAndRemoveUntil triggers secondaryAnimation`»
9. `did_change_next_rewires_the_secondary_animation`; `did_pop_next` likewise.
10. `secondary_animation_resets_to_always_dismissed_when_the_next_route_completes` — «`secondary animation is kDismissed when next route finishes pop`», «`… when next route is removed`». *Red-check:* drop the `parent == animation` guard in `_set_secondary_animation`; a stale cleanup clobbers a newer parent.
11. `train_hopping_swaps_to_the_target_once_the_trains_cross`, and `…_disposes_the_hopper_when_it_does`. «`secondary animation is kDismissed after train hopping finishes and pop`»
12. `train_hopping_interrupted_mid_hop_tears_down_the_previous_hopper_after_installing_the_new_one` — the `:495` ordering. *Red-check:* invoke the previous remover before installing the replacement; use-after-dispose. «`secondary animation is kDismissed when train hopping is interrupted`»
13. `can_transition_to_and_from_gate_the_secondary_animation` — a non-`PageRoute` next route leaves the secondary at always-dismissed. *Red-check:* drop the two predicates from the `if` at `:429-431`.

**Offstage (U5.0)**

14. `offstage_child_is_laid_out_at_real_size` — under loose constraints the child reaches its intrinsic size; the `Offstage` box reports `constraints.smallest`. *Red-check:* the current `tight(Size::ZERO)`.
15. `offstage_box_reports_constraints_smallest_under_tight_constraints` — **the constraint violation**. *Red-check:* return `Size::ZERO`.
16. `offstage_child_is_not_painted_and_not_hit_tested` — unchanged behavior, pinned so the fix does not overshoot.

**Modal barrier (U5.3)**

17. `barrier_absorbs_taps_meant_for_the_route_beneath` — «`prevents interactions with widgets behind it`». *Red-check:* make the barrier's hit-test transparent.
18. `barrier_does_not_block_widgets_in_front_of_it` — «`does not prevent interactions with widgets in front of it`».
19. `dismissible_barrier_pops_the_navigator_on_tap` — «`pops the Navigator when dismissed by primary tap`». *Red-check:* call `pop` instead of `maybe_pop`; a lone route would then vanish rather than bubble.
20. `non_dismissible_barrier_still_absorbs_the_tap` — «`plays system alert sound when user tries to dismiss it`» (the sound is deferred; the absorption is not).
21. `barrier_sits_below_the_content_entry` — `overlayEntries[0]` is the barrier, `[1]` the scope. *Red-check:* swap them; taps on the content reach the barrier.

**`opaque` / `maintainState` (U5.3)**

22. `maintain_state_false_destroys_the_covered_route` — a covered route's `ViewState` is disposed and recreated on reveal. **Untestable before U5.3.**
23. `maintain_state_true_preserves_the_covered_route` — its `ViewState` is not recreated.
24. `an_animating_route_forces_its_entry_opaque_false` — routes beneath keep building mid-transition (`:303-305`). *Red-check:* set `opaque` on `forward`/`reverse`.
25. `overlay_no_longer_builds_every_entry` — the deliberate replacement of `overlay_deferred_opaque_builds_every_entry`.

**Honesty**

26. **No `Hero` test, and no `Hero` claim.** `ModalRoute::offstage` exists after U5.3, which removes *one* of Hero's four blockers (ADR-0019 §6). The other three — Navigator + Overlay (now met), the observer API (met), and GlobalKey reparenting across overlay entries plus `createRectTween` flight geometry (**not** met) — keep it blocked. Hero gets its own ADR.

---

## 6. Deferred, each with its blocker

| Deferred | Blocker | Why deferring is safe |
|---|---|---|
| **`Hero`** | Needs `ModalRoute::offstage` (U5.3), plus GlobalKey reparenting **across overlay entries** and `createRectTween` flight geometry. `HeroController` is a `NavigatorObserver`, which exists. | Starting it before U5.3 fixes a wrong shape. Own ADR. |
| **Per-route focus scope** | **No `Focus`/`FocusScope` widget exists** (Seam 6). H4 shipped only `flui-interaction`'s nodes and manager. | A route without a focus scope is *visibly* keyboard-incomplete, not silently wrong. Faking it — driving `FocusManager` directly from a route — is precisely the "MVP reported as parity" failure the Definition of Done names. Needs its own widget-layer slice. |
| **`BlockSemantics`** | Does not exist. | The barrier can `ExcludeSemantics` itself, but cannot occlude routes beneath. An accessibility gap; name it, don't paper over it. |
| **Predictive back / `popGestureEnabled` / user gestures** | Platform channel; `AnimationController::animate_back_with` exists but `userGestureInProgressNotifier` does not. | Additive. `_handleStatusChanged`'s `!isActive` guard is ported anyway, because it is what makes gesture-driven subclasses *possible* later. |
| **Delegated transitions** (`_buildFlexibleTransitions`, `receivedTransition`, `delegatedTransition`) | None. | Cupertino chained transitions. Pure polish; a first `ModalRoute` calls `buildTransitions` directly. |
| **`BackdropFilter` / `filter`** | No backdrop-filter render object surfaced at the widget layer. | Cosmetic. |
| **`_ModalScopeStatus` aspect granularity** (`InheritedModel` with seven aspects) | FLUI's `InheritedView` has no aspect model. | A plain `InheritedView` with coarse `update_should_notify` is correct, just less efficient. |
| **`PageStorage`, `RestorationScope`, `PrimaryScrollController`, `traversalEdgeBehavior`, `_DismissModalAction`** | Each needs its own subsystem. | None is required to get a modal onscreen. |
| **`allowSnapshotting`, `fullscreenDialog`** | No snapshotting layer. | Config fields with no consumer; omit rather than accept-and-ignore. |
| **Navigator 2.0 `pages` / `Router`, restoration, named-route generation, `PopScope`, `LocalHistoryRoute`** | Unchanged from ADR-0019 §6. | Unchanged. |
| **`pushReplacement` / `pushAndRemoveUntil` public export** | Ported and tested; not exported (ADR-0019 U4). | Two of §5's oracles (`pushReplacement triggers secondaryAnimation`) need them **at the test layer only**; they are `#[cfg(test)]`-reachable. Exporting them is a separate sign-off. |

---

## 7a. Implementation findings (U5.0, 2026-07-09)

`RenderOffstage` now matches `proxy_box.dart:3894-3951`: the child is laid out under the **real** incoming constraints, the box reports `constraints.smallest()`, and paint, hit-test and semantics are all suppressed. Three things §2's Seam 4 did not say:

1. **The existing tests were structurally blind to the defect, and stayed green through the fix.** Every offstage test used `loose` constraints, where `constraints.smallest()` *is* zero — so `Size::ZERO` and the correct value coincide. The discriminating observations are (a) the **child's** size under loose constraints, and (b) the **box's** size under *tight* ones. Both are now asserted. A parity test file had even recorded the defect as `FLUI-DEV-001 — tracked for the next render-object patch` and chosen `loose(200)` to dodge the resulting constraint violation; that note is now corrected rather than deleted.

2. **Semantics suppression was missing entirely**, and §2 did not notice. Flutter's `visitChildrenForSemantics` returns early when offstage (`:3945-3951`). FLUI's `RenderBox` has the exact counterpart — `excludes_semantics_subtree()`, whose contract is "this node's own config is still built; only its descendants are dropped" — and `RenderOffstage` never implemented it. So an offstage subtree was still announced to assistive technology. Now fixed, and pinned by a harness test that asserts the child's labelled node is absent from the semantics tree (with a visible-control assertion, so the test cannot pass vacuously).

3. **`RenderSliverOffstage` does *not* share this defect.** Cross-checked against `proxy_sliver.dart:349-358`: Flutter lays the sliver child out with the real constraints and sets `geometry = SliverGeometry.zero` when offstage; FLUI does exactly that. Its comment is accurate.

   The cross-check did, however, surface **two unrelated gaps in `RenderSliverOffstage`**, neither touched by U5.0: it has **no `paint` override** (Flutter returns early, `proxy_sliver.dart:390-396`; FLUI appears to rely on the zero geometry instead) and **no `excludes_semantics_subtree`** (Flutter skips children for semantics). These are a separate defect class needing their own harness evidence, and are recorded here rather than folded into an unrelated fix.

**Fallout: none behavioral.** No test changed its expected value. `Visibility(maintain_state: true)` under loose constraints is unaffected (`smallest` is zero); under tight constraints a hidden `Visibility` would now correctly occupy its tight size, which is what Flutter does. Three comments asserting the old behavior were corrected, and a widget-level test was added asserting the hidden-but-maintained child reaches its full geometry.

**One blocker removed, and only one.** `Hero` needs `ModalRoute::offstage`, which needs a correct `Offstage`. It also still needs `ModalRoute` itself (U5.3), and GlobalKey reparenting across overlay entries plus `createRectTween` flight geometry. **Hero remains blocked.**

## 7. Consequences

**Good.** The two deferral points U5 needs — `PushCompletion::Animating` and `finished_when_popped` — were designed into ADR-0019 U2 and are already tested; U5 supplies their first production producer. The animation library is unusually complete: `ProxyAnimation::set_parent`, `ALWAYS_DISMISSED`/`ALWAYS_COMPLETE`, and an `AnimationSwitch` that claims to be `TrainHoppingAnimation`. `RouteBinding` reuses the owned-capability pattern three ADRs have now converged on.

**Bad.** ADR-0019's proudest structural result — the route stack is pure data, routes are leaves — is exactly what makes U5 expensive. Three back-references must come back, carefully, without contaminating `RouteHistory`. And ADR-0019 §2.2's "no new render object" expires: `RenderTheater` arrives in U5.3, or `maintainState` is a lie.

**Ugly.** The re-entrancy guard `flush()` asserts on is currently unreachable *by construction*; U5.1 makes it reachable and must replace the assert with Flutter's deferral, not delete it. And `RenderOffstage` had been wrong since it was written, under a comment asserting parity — fixed in U5.0, and a reminder that a comment claiming "Flutter parity" is a claim like any other, which this repo's own Definition of Done says must be verified against `.flutter/`, not asserted. Its sibling `RenderSliverOffstage` still carries two unverified suppression gaps (§7a).

---

## Open questions for the deciders

1. **Decision 1:** does `NavigatorState` take its `Vsync` from an ambient `VsyncScope` (matching `Scrollable`), or from the binding directly? The former makes a navigator inside a paused subtree freeze correctly; the latter always ticks.
2. **Decision 3:** implement `Overlay.opaque`/`maintainState` in U5.3 (recommended), or ship `ModalRoute` without them and delete both from its surface?
3. **U5.4:** must `Overlay` / `OverlayEntry` become public for app authors to write custom routes, or is `NavigatorRoute::content_builder` a sufficient door?
4. **U5.0:** is anything depending on `RenderOffstage`'s current zero-size behavior? (`Visibility(maintain_state: true)` is the obvious candidate.)
