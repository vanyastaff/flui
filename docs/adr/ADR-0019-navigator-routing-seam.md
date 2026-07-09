# ADR-0019: `Navigator` needs **no new framework seam** — ADR-0017/0018 already shipped them — but it does need an `Overlay`, and Rust's lack of `dynamic` forces one unavoidable divergence: **the pop result crosses a `dyn Any` boundary**

*Flutter's `Navigator` is an imperative, mutable route stack that reaches its `Overlay` through a `GlobalKey`, calls `setState` from arbitrary callbacks, and delivers a pop result of type `T` through a `Completer<T?>` stored in a `Route<dynamic>`. Three of those four move cleanly to FLUI. The fourth does not: a heterogeneous `Vec<Box<dyn Route>>` cannot carry `T`, so `pop(result)` must erase and the receiving route must downcast. This ADR reads the reference, establishes that the flush algorithm touches no tree and therefore needs no build-during-layout-style seam, identifies the two lookups that would deadlock if written the Flutter way, and stages the work so the erasure decision is signed off before any public API exists.*

---

- **Status:** Accepted — **U1–U3 landed 2026-07-09** (`Overlay` / `OverlayEntry`; the pure-data route stack; and now a working private `Navigator` view, `NavigatorState` and owned `NavigatorHandle`). U4–U5 unstarted. **No public API**; the `dyn Any` pop-result divergence (§4) is still **unauthorized** and confined to private modules.
- **Date:** 2026-07-09
- **Deciders:** chief-architect; consult view owner (`Navigator::of` handle shape, the two lock hazards in §3), api-design-lead *(role does not exist in this repo — see* Gate 2 *)*, repository owner (the `dyn Any` pop-result divergence: a public API shape we cannot walk back), qa-lead (headless determinism: driving a route transition inside a test frame).
- **Relates to:** reuses [`ADR-0018`](ADR-0018-async-builder-seam.md)'s `RebuildHandle` verbatim, and the "state publishes its handle into a shared cell at `init_state`" pattern from [`ADR-0017`](ADR-0017-build-during-layout-callback-seam.md) U1 (`LayoutConstraintsCell`) / ADR-0018 U4 (`SharedSlot`). Constrained by [`FOUNDATIONS.md`](../FOUNDATIONS.md) and [`PANIC-POLICY.md`](../PANIC-POLICY.md).
- **Blocks:** `Navigator`/routing (tracker B1.1). Transitively blocks `Hero` (tracker B1.1) — see §6.
- **Gate:** ARCH-GATE (this doc) → DEV-GATE per slice → **parity + sign-off gate before public export** (U4), per the [Definition of Done](../../AGENTS.md#definition-of-done-anti-cheating).

---

## Reference

Cross-checked against `.flutter/`, Flutter master **`3.33.0-0.0.pre-6280-g88e87cd963f`** (the same revision used for ADR-0017 U4 and ADR-0018 U6):

| File | Read for |
|------|----------|
| `packages/flutter/lib/src/widgets/navigator.dart` (6490 ln) | `Route`, `_RouteEntry`, `_RouteLifecycle`, `_flushHistoryUpdates`, `NavigatorObserver`, `Navigator.of` |
| `packages/flutter/lib/src/widgets/overlay.dart` (2989 ln) | `Overlay`, `OverlayEntry`, `_OverlayEntryWidget`, `_Theater` / `_RenderTheater` |
| `packages/flutter/lib/src/widgets/routes.dart` (2862 ln) | `OverlayRoute` → `TransitionRoute` → `ModalRoute` → `PopupRoute` |
| `packages/flutter/lib/src/widgets/heroes.dart` (1154 ln) | **Hero dependencies only.** No Hero design in this ADR. |
| `test/widgets/{navigator,overlay,routes}_test.dart` | The behavioral oracles quoted in §7 |

Every statement about Flutter below is read from those files at that revision, with line citations. Statements about FLUI are read from the working tree at commit `ff92c276`. Where I could not verify something, it is labelled **`UNVERIFIED`** rather than asserted.

> **Spelling.** The code spells it `_Theater` / `_RenderTheater` (US). Not `_Theatre`.

---

## 1. What Flutter's Navigator observably guarantees

### 1.1 Everything funnels through one synchronous flush

`push` is three lines (`navigator.dart:5060-5063`): construct a `_RouteEntry` in state `_RouteLifecycle.push`, `_history.add(entry)`, `_flushHistoryUpdates()`, and return `route.popped`. The public methods **only mutate `_history` and call the flush**. All lifecycle work — `install()`, `didPush()`, observer notification, neighbour announcements, overlay insertion — happens inside `_flushHistoryUpdates` (`navigator.dart:4451-4619`).

This is the single most important structural fact, and it is what makes the port tractable: **the flush is a pure function over a `Vec<_RouteEntry>` plus a set of callbacks. It never touches the element tree.** Its only tree-visible effect is the final `overlay?.rearrange(...)` (`:4612`), which is a `setState` on `OverlayState` (`overlay.dart:848-852`) — i.e. a deferred rebuild request, not a mutation.

Contrast with ADR-0017, where the whole difficulty was that build-during-layout genuinely needed to mutate the tree mid-walk. Navigator does not. **No new build/layout seam is required.** (Ladder rung 1: the seam does not need to exist.)

### 1.2 Ordering within one flush

The loop iterates `_history` **top → bottom** (`:4458-4468`), tracking `next`/`previous` neighbours. After the loop, in this exact order (`:4584-4617`):

1. `_flushObserverNotifications()` (`:4585`)
2. `_flushRouteAnnouncement()` (`:4589`) — the neighbour `didChangeNext` / `didChangePrevious`
3. `didChangeTop` to observers, if the top present route changed (`:4591-4597`)
4. engine route-name update, if `reportsRouteUpdateToEngine` (`:4599-4605`)
5. deferred disposal of `toBeDisposed` entries (`:4609-4611`)
6. `overlay?.rearrange(_allRouteOverlayEntries)` (`:4612-4614`)

So: **history is mutated first, observers fire second, neighbour announcements third, overlay rearrange last.** Entries destined for disposal are removed from `_history` inside the loop (`:4572`) but *disposed only after* the announcements, precisely so a dying route still receives its final `didChangeNext`/`didChangePrevious` (the source comments this at `:4571`).

Per-entry, `handlePush` (`:3252-3310`) establishes: `route._navigator = navigator` → `route.install()` → assert `overlayEntries.isNotEmpty` → `route.didPush()` → (if new-first) `route.didChangeNext(null)` → **enqueue** the observer notification. The observer call is *never* inline.

### 1.3 Observer notification order is asymmetric — and this is not an accident

Verified verbatim at `navigator.dart:4621-4636`:

```dart
while (_observedRouteAdditions.isNotEmpty) {
  final _NavigatorObservation observation = _observedRouteAdditions.removeLast();   // LIFO
  _effectiveObservers.forEach(observation.notify);
}
while (_observedRouteDeletions.isNotEmpty) {
  final _NavigatorObservation observation = _observedRouteDeletions.removeFirst();  // FIFO
  _effectiveObservers.forEach(observation.notify);
}
```

**Additions drain LIFO (`removeLast`), deletions drain FIFO (`removeFirst`).** All additions precede all deletions. If no observers are registered, both queues are cleared without notification (`:4623-4626`) — so registering an observer *changes when routes learn nothing*, but never changes route lifecycle. This asymmetry is observable (`test/widgets/navigator_test.dart` → `'initial route trigger observer in the right order'`) and must be ported exactly. It is the kind of detail a "reasonable" reimplementation gets wrong.

The observer methods (`:777-839`): `didPush`, `didPop`, `didRemove`, `didReplace`, `didChangeTop`, `didStartUserGesture`, `didStopUserGesture`.

### 1.4 The route lifecycle state machine

`_RouteLifecycle` (`:3139-3168`), 16 states in index order — and the *index order is load-bearing*, because the predicates are range checks:

`staging`, `add`, `adding`, `push`, `pushReplace`, `pushing`, `replace`, `idle`, `pop`, `complete`, `remove`, `popping`, `removing`, `dispose`, `disposing`, `disposed`.

- `willBePresent` = `add ..= idle` (`:3519-3522`)
- `isPresent` = `add ..= remove` (`:3524-3527`)
- `suitableForAnnouncement` = `push ..= removing` (`:3531-3534`)
- `suitableForTransitionAnimation` = `push ..= remove` (`:3536-3539`)

In Rust this ports as `#[repr(u8)] #[derive(PartialOrd, Ord)] enum RouteLifecycle` with `(Add..=Idle).contains(&state)`. Structure Rust-native, behavior identical.

Two flags in the flush loop control the *silent* add/remove behavior:

- **`canRemoveOrAdd`** (`:4462`) — set once the loop has passed an `idle` route or a completed pop. Meaning: "a settled route covers everything below, so routes beneath may be added or disposed without being seen." This is why `pushReplacement` keeps the old route alive until the new route finishes animating (`:4479`, `:4564-4568`).
- **`poppedRoute` / `seenTopActiveRoute`** (`:4464-4466`) — ensures exactly the topmost active route below a pop receives `didPopNext` (`:4501-4508`, `:4521-4526`).

### 1.5 Pop result delivery

Verified verbatim (`navigator.dart:424-434`, `456-482`):

```dart
T? get currentResult => null;
Future<T?> get popped => _popCompleter.future;
final Completer<T?> _popCompleter = Completer<T?>();

bool didPop(T? result) { didComplete(result); return true; }
void didComplete(T? result) { _popCompleter.complete(result ?? currentResult); }
```

Three consequences:

1. `push()` returns `route.popped` — the future exists **before** any lifecycle runs.
2. The delivered value is `result ?? currentResult`, i.e. the route supplies a fallback when the popper passes nothing.
3. **A route that is *removed* rather than popped still completes its future.** `removeRoute` / `pushReplacement` / `pushAndRemoveUntil` route through `_RouteEntry.complete` → `handleComplete` → `route.didComplete(pendingResult)` (`:3381-3386`). `pushAndRemoveUntil` completes each removed route with `null` (`:5360`). The oracle is `test/widgets/navigator_test.dart` → `'remove a route whose value is awaited'`. **A port that only completes on `pop` will hang every `await` in an app that uses `removeRoute`.**

`didComplete` calls `_popCompleter.complete` unconditionally; a double-complete would throw in Dart. It is guarded by `_RouteEntry.complete`'s state check (`:3430-3439`, no-op once state ≥ `remove`). Rust's one-shot channel gives us this for free, but the *guard* must still be ported or the second completion is silently dropped instead of loudly wrong.

### 1.6 `maybePop` / `canPop`

- `canPop()` (`:5551-5566`): no present routes → `false`; first present route `willHandlePopInternally` → `true`; exactly one present route → `false`; otherwise `true`.
- `maybePop()` (`:5582-5615`) switches on `route.popDisposition` (`:117-136`, `:382-390`): `bubble` → return `false` (unhandled; the app usually closes); `pop` → `pop(result)`, return `true`; `doNotPop` → call `onPopInvokedWithResult(false, result)`, return `true`.
- `popDisposition` defaults to `isFirst ? bubble : pop`.
- `onPopInvokedWithResult(didPop, result)` fires with `true` on a real pop (`:3372`) and with `false` on a blocked pop (`:5612`).

`maybePop` is `async` in Dart because of the deprecated `willPop()` await (`:5591`). Once `willPop` is dropped (it is deprecated), the remaining logic is synchronous. **FLUI must not port `maybePop` as `async fn`** — that would violate the no-async-in-hot-paths rule for no benefit. Port the synchronous `popDisposition` path only.

### 1.7 Nested navigators and lookup

`Navigator.of(context, {rootNavigator = false})` (`:2947-2968`):

```dart
if (context case StatefulElement(:final NavigatorState state)) navigator = state;
navigator = rootNavigator
    ? context.findRootAncestorStateOfType<NavigatorState>() ?? navigator
    : navigator ?? context.findAncestorStateOfType<NavigatorState>();
```

Note the **self-check first**: if `context` *is* the Navigator's own element, that Navigator is used. `rootNavigator: true` walks to the outermost `NavigatorState`, falling back to the local one. `maybeOf` (`:2992-3001`) is the same lookup returning `null` instead of throwing.

### 1.8 Initial route

`initialRoute` is read **once**, in `restoreState` (`:3868-3934`), and only when no serialized history was restored (`:3895`). Changing it later has no effect (`:1689-1692`). `defaultGenerateInitialRoutes` (`:3017-3058`) splits `'/stocks/HOOLI'` into `'/'`, `'/stocks'`, `'/stocks/HOOLI'` and pushes each as a separate entry — a deep link synthesizes a back stack. These enter as `add` → `didAdd` (**not** `didPush`), so they play no push transition, yet observers still receive `didPush` observations (enqueued by `handleAdd`, `:3249`).

### 1.9 Does Navigator require Overlay? Yes.

`NavigatorState.build` unconditionally returns an `Overlay` (`:5984-5990`). `Route.install()` populates `overlayEntries`, and both `handlePush` (`:3272`) and `didAdd` (`:3410`) **assert the list is non-empty** immediately afterwards. Insertion into the live overlay happens only via `overlay.rearrange(_allRouteOverlayEntries)` (`:4612`), where `_allRouteOverlayEntries` (`:4151-4153`) flattens every route's entries across `_history` in order.

**Overlay is mandatory and must be built first.**

### 1.10 GlobalKey

Navigator's *only* intrinsic GlobalKey is `_overlayKey = GlobalKey<OverlayState>()` (`:3746`, `:3875`), used to reach `overlay` so it can call `rearrange` (`:4149`). `Navigator.of` does **not** use it — it walks the element tree. A user-supplied `GlobalKey<NavigatorState>` is a convenience, not a requirement.

`OverlayEntry` holds `GlobalKey<_OverlayEntryWidgetState>` (`overlay.dart:214`) so its subtree state survives `rearrange` reordering and so `markNeedsBuild` can reach `_key.currentState` (`:250`).

**Neither GlobalKey survives the port** — see §3.2 and §3.3. This is the ADR's main structural result.

---

## 2. Overlay: what it actually is, and the smallest thing that works

### 2.1 `OverlayEntry` is not a widget

`class OverlayEntry implements Listenable` (`overlay.dart:109`). It is a plain object holding a `WidgetBuilder` (`:130`), an `opaque` flag, a `maintainState` flag, and a `GlobalKey`. The widget is `_OverlayEntryWidget`, a `StatefulWidget` (`:297`) keyed by that GlobalKey.

`OverlayState.build` (verified verbatim, `overlay.dart:888-918`):

```dart
var onstage = true; var onstageCount = 0;
for (final OverlayEntry entry in _entries.reversed) {
  if (onstage) {
    onstageCount += 1;
    children.add(_OverlayEntryWidget(key: entry._key, overlayState: this, entry: entry));
    if (entry.opaque) { onstage = false; }
  } else if (entry.maintainState) {
    children.add(_OverlayEntryWidget(key: entry._key, …, tickerEnabled: false));
  }
}
return _Theater(skipCount: children.length - onstageCount, …,
                children: children.reversed.toList(growable: false));
```

Read that carefully, because it dictates the whole minimal design:

- Entries **below the topmost `opaque` entry are omitted from the widget tree entirely**, unless `maintainState` is set.
- `maintainState` entries are built but with `tickerEnabled: false` (animations frozen) and are counted into `skipCount`.
- `skipCount` tells `_RenderTheater` how many leading children to **skip in layout, paint, hit-test and semantics** (`:1344-1355`, `:1427-1428`).

So `opaque`, `maintainState`, and `skipCount` are **one mechanism**: an optimization that avoids building/laying-out covered routes, plus an escape hatch for routes that must survive being covered.

**`maintainState` exists only because `opaque`-skipping exists.** Drop the optimization and every entry is built, laid out and painted; every route's state survives trivially.

### 2.2 `_RenderTheater` is *not* `RenderStack`, but it is a stack

`_RenderTheater` (`overlay.dart:1194`) is a `RenderBox` with `ContainerRenderObjectMixin<RenderBox, StackParentData>` that reimplements the stack algorithm rather than extending `RenderStack`; it reuses `RenderStack.layoutPositionedChild` (`:1131`) and `RenderStack.getIntrinsicDimension` (`:1361`). Its parent data is `_TheaterParentData extends StackParentData` (`:1164`), adding `overlayEntry` plus the `OverlayPortal` iterators.

`performLayout` (`:1466-1485`): normally `size = constraints.biggest` (`:1468`), then every child in paint order is laid out with `BoxConstraints.tight(size)` — the source comments this as "Equivalent to BoxConstraints used by RenderStack for StackFit.expand" (`:1478-1484`). Paint order is first-onstage → last, so **the last entry paints on top** (`:894`, `:916`, `:1157-1161`).

**Therefore, with `skipCount == 0` and `alwaysSizeToContent == false`, `_RenderTheater` is behaviorally `RenderStack` with `StackFit::Expand`.** FLUI already has `RenderStack` (`crates/flui-objects/src/layout/stack.rs:221`) with `fit` / `alignment` / `clip_behavior`, `StackParentData` (`crates/flui-rendering/src/parent_data/box_variants.rs:191`), `Positioned`, and last-child-on-top paint order.

**U1 needs no new render object.** (Ladder rung 2: it's already in the codebase.)

### 2.3 What Overlay must do, and what it may skip

Cannot be dropped:

1. An ordered, mutable entry list, **last = topmost**.
2. Per-entry `builder` + a **targeted rebuild** of one entry (`markNeedsBuild`, `overlay.dart:250` → `setState`).
3. `insert` / `remove` that are safe to call during the persistent-callbacks phase — `OverlayEntry.remove` defers its `_markDirty` to a post-frame callback when called mid-frame (`:236-242`).
4. `rearrange` (`:813-846`), reordering to a caller-supplied list, preserving the relative order of unmentioned entries, short-circuiting when `listEquals` (`:833`).
5. Some mechanism preserving the state of covered entries.

May be deferred (with a named cost — §6):

- `opaque` skipping + `maintainState` + `skipCount` — build everything instead.
- `canSizeOverlay` / `alwaysSizeToContent` (only needed under unbounded constraints, `:1487-1525`).
- `OverlayPortal` and its entire deferred-layout machinery (`:1869`, `:2488-2717`) — **proven not required**: routes construct plain `OverlayEntry`s (`routes.dart:2350-2359`), and `_RenderTheater` skips the portal iterators when unused (`:1432-1437`).

### 2.4 The route class hierarchy, and where the floor is

`Route<T>` (`navigator.dart:161`) → `OverlayRoute<T>` (`routes.dart:55`) → `TransitionRoute<T>` (`:111`) → `ModalRoute<T>` (`:1266`) → `PopupRoute<T>` (`:2380`) / `PageRoute<T>` (`pages.dart:23`).

| Layer | Adds |
|-------|------|
| `Route<T>` | lifecycle hooks, `popped`/`_popCompleter`/`currentResult`, `isCurrent`/`isFirst`/`isActive`. `overlayEntries` is `const []`. |
| `OverlayRoute<T>` | the real `overlayEntries`, `createOverlayEntries()`, `install()`, `finishedWhenPopped` (default `true`), `didPop` → `navigator.finalizeRoute(this)`, `dispose()` |
| `TransitionRoute<T>` | `AnimationController` (`vsync: navigator!`), `didPush()` → `TickerFuture`, `secondaryAnimation` + train-hopping, `opaque`, and **`finishedWhenPopped => controller.isDismissed`** (`:178`) — deferring removal until the exit animation ends |
| `ModalRoute<T>` | two entries (barrier + scope), `buildPage`/`buildTransitions`, focus scope, `offstage`, `maintainState`, `ModalRoute.of` |
| `PopupRoute` / `PageRoute` | thin config: `opaque` false/true, `maintainState` true/configurable |

**The floor is `Route` + `OverlayRoute` + one `OverlayEntry`.** That combination can push, show, pop with a result, and preserve the route beneath — with *instant* transitions, because `finishedWhenPopped == true` makes `didPop` finalize synchronously (`routes.dart:87-94`).

One correction to a tempting simplification: the modal **barrier** is often described as cosmetic. Its *color* is (`barrierColor`, `routes.dart:1808`). Its **pointer absorption is not** — without a barrier entry, and without `opaque`-skipping to unbuild them, the routes below remain hit-testable and taps fall through. FLUI already has `AbsorbPointer` (`crates/flui-widgets/src/interaction/absorb_pointer.rs`), so the barrier is cheap; but it must not be labelled optional once `opaque`-skipping is deferred.

---

## 3. What FLUI has, what it lacks, and the two lookups that would deadlock

### 3.1 Present, and directly reusable

| Need | FLUI today |
|------|-----------|
| Stack layout, last-on-top | `RenderStack` (`flui-objects/src/layout/stack.rs:221`), `Stack`/`Positioned`/`IndexedStack` |
| `StackParentData` | `flui-rendering/src/parent_data/box_variants.rs:191` |
| `findAncestorStateOfType` | `BuildContext::find_ancestor_state` (`context/build_context.rs:207`) |
| `findRootAncestorStateOfType` | `BuildContext::find_root_ancestor_state` (`:227`) |
| `setState` from a callback | `RebuildHandle` (ADR-0018, `owner/rebuild_handle.rs:83`) — `Clone + Send + Sync + 'static` |
| Dynamic child list + keyed reconciliation | `RenderElement<V> = Element<V, Variable, …>`, `reconcile_children_by_id` (`tree/id_reconcile.rs:122`) |
| Pointer absorption for a barrier | `AbsorbPointer`, `GestureDetector`, full hit-test in `flui-rendering` |
| Animation (for `TransitionRoute`, U5) | `flui-animation`: `AnimationController`, `Animation<T>`, `Curve`, `Ticker`/`TickerProvider`, `Vsync` |
| A future to `await` a pop result | ADR-0018's `AsyncDriver` + the `SharedSlot` one-shot pattern |

### 3.2 `Navigator::of(ctx)` cannot return `&mut NavigatorState` — and must not look anything up

`find_ancestor_state` has this signature (`context/build_context.rs:207`):

```rust
fn find_ancestor_state(&self, type_id: TypeId, callback: &mut dyn FnMut(&dyn Any)) -> bool;
```

It yields **`&dyn Any` — immutable — inside a callback**, and the tree is borrowed for the callback's whole duration. There are two implementations and they borrow differently:

- `BuildCtx<'b>` (`element_build_context.rs:601`), the **build-time** context: holds `tree: &'b ElementTree`, a plain shared borrow. No lock.
- `ElementBuildContext` (`:36`): holds `tree: Arc<RwLock<ElementTree>>` and `walk_strict_ancestors` opens `self.tree.read()` for the whole walk (`:181`).

Two consequences, both load-bearing:

**(a) `push` must take `&self`.** `ViewState::build` is already `&self` (`view/stateful.rs:129`) and `ViewState: Send + Sync`. So the route stack lives behind a private `Mutex` inside the state. That is consistent with SP-6 ("locks behind private fields") and with `ViewState`'s existing shape. It is not a compromise; Flutter's `NavigatorState` mutates `_history` from `&this` too.

**(b) No nested tree lookup inside the callback.** This is the trap. Flutter's `_flushHistoryUpdates` ends by calling `overlay.rearrange(...)`, where `overlay` resolves through `_overlayKey.currentState` — a **GlobalKey lookup**. Porting that literally means, from inside `find_ancestor_state`'s callback:

- with `ElementBuildContext`: the `tree.read()` guard is held, and `GlobalKey::current_element` → `registry::with_registry` → the production hook installed at `binding.rs:560-576` takes `WidgetsBinding::instance().inner.read()`. That is a **second lock, acquired while holding the first**, establishing a lock order (`ElementTree` → `WidgetsBindingInner`) that nothing else in the codebase guarantees. `parking_lot::RwLock` is **not reentrant**, and `read()` while a writer is queued blocks.
- with `BuildCtx`: the registry hook re-enters `binding.inner.read()`, which is the very lock a `build_scope` running under `inner.write()` would already hold. **`UNVERIFIED`** whether production `build_scope` holds `inner.write()` across the build; the GlobalKey reparent tests install the registry via `test_only_set_global_key_registry` and bypass the binding entirely, so the production nesting appears untested. I did **not** confirm a live deadlock, and this ADR does not claim one — it claims the hazard is real, unproven-safe, and trivially avoidable.

**Decision: `Navigator::of(ctx)` returns an owned, `'static`, `Clone` handle, cloned out of the state inside the callback. Every mutation happens after the callback returns and the borrow is released.**

```
NavigatorHandle {                      // 'static + Clone + Send + Sync
    shared: Arc<NavigatorShared>,      // route stack behind a private Mutex
    overlay_rebuild: RebuildHandle,    // published by OverlayState at init_state
}
```

This is not an invention. It is exactly `RebuildHandle` (ADR-0018 U1: escape the borrow-scoped `BuildContext` by handing out a `'static` capability) composed with the shared-cell publication pattern of ADR-0017 U1 and ADR-0018 U4. Navigator and Overlay couple through an `Arc`, not through the element tree — so **`GlobalKey<OverlayState>` is not ported, and the `GlobalKey` hazard never arises.**

The same reasoning retires the second GlobalKey: `OverlayEntry` need not carry `GlobalKey<_OverlayEntryWidgetState>` to implement `markNeedsBuild`, because the entry's `ViewState` can publish its own `RebuildHandle` into the entry at `init_state` — the `LayoutConstraintsCell` move. What GlobalKey *also* buys Flutter is **state preservation across `rearrange` reordering**; FLUI must instead rely on keyed reconciliation (`reconcile_children_by_id`, which emits `Reorder`).

> **Resolved in U1 (2026-07-09).** This paragraph previously carried an `UNVERIFIED` label on "keyed reorder preserves `ViewState` across a sibling permutation", and made it U1's gate for dropping `GlobalKey`. It holds. `overlay_rearrange_reorders_and_preserves_entry_state` reorders two layers and asserts each keeps its `ElementId` *and* that neither subtree's `create_state` runs a second time. Red-checked by deleting `OverlayEntryView::key`: reconciliation then matches by index and type, and an element is reconciled onto a **different** `OverlayEntry` — caught by the `did_update_view` `debug_assert` that now guards exactly this. A `ValueKey<OverlayEntryId>` is sufficient; **`GlobalKey` is not used by `Overlay`, so the §3.2(b) hazard never arises**, and it remains unresolved-but-unreached.

> Trigger #22 (ADR-0018) already forbids acquiring `rebuild_handle()` inside `build`/layout/paint. Publishing at `init_state` and firing later is the sanctioned pattern, and the existing guard covers Navigator for free.

### 3.3 `Navigator::of` self-check: a divergence we must handle, not inherit

Flutter checks whether `context` **is** the `NavigatorState`'s element before walking ancestors (`:2947`). FLUI's `walk_strict_ancestors` is *strict* — it starts at `parent` (`element_build_context.rs:646`) and can never match self. A `Navigator::of` called from the Navigator's own element would therefore find the *enclosing* navigator, silently targeting the wrong one in a nested setup.

`find_root_ancestor_state` likewise has no `?? navigator` fallback. Both gaps must be closed in `Navigator::of`'s own body (check self, then walk), not by changing `BuildContext`.

> **Corrected in U3 (2026-07-09): neither gap is reachable, and the first cannot be closed at all.** `Navigator::of` cannot check self: during `build` the element's own node is a **hole** (`element_opt` returns `None`), so no `BuildContext` API can reach its own state — and Flutter's self-check only fires for a context obtained from `GlobalKey<NavigatorState>.currentContext`, which FLUI does not have. The second gap is vacuous: the root walk cannot find *fewer* navigators than the nearest walk, so Flutter's `?? navigator` fallback has nothing to catch. `Navigator::of` is therefore a plain `find_state` / `find_root_state` pair, and this paragraph's prescription was wrong.

### 3.4 `flui-app::overlay` is speculative scaffolding, and its name collides

`crates/flui-app/src/overlay/` (568 lines: `entry.rs`, `manager.rs`) exports `OverlayEntry`, `OverlayEntryBuilder`, `OverlayPosition`, `OverlayManager` from `flui-app` (`lib.rs:41`). It is:

- **a different abstraction** — a `HashMap<OverlayId, OverlayEntry>` sorted by an `OverlayPriority` enum, with entries positioned by an `OverlayPosition` enum (`Center`, `Absolute{x,y}`, `TopLeft{…}`, `Fill`, …). Flutter's Overlay is an insertion-ordered stack with `rearrange`; priority ordering and absolute positioning are not in the contract.
- **contentless** — `OverlayEntry` holds `id`, `priority`, `position`, `tag`, a blocks-interaction flag. It holds **no `BoxedView` and no builder closure**. It cannot host a route's subtree.
- **dead** — referenced nowhere outside its own module (the only other `overlay` hit in the workspace is the unrelated `performance_overlay` layer).

It is exactly the category tracker **H7** ("Speculative-scaffolding feature-gating") was opened for. **It must not be the basis of Navigator's Overlay**, and its public `OverlayEntry` will collide by name with the real one. U1 must first delete it or rename it (`FloatingLayerManager`) — that decision is a one-line call for the repository owner, recorded as a U1 precondition, not something to discover mid-implementation.

> **Resolved in U1: deleted.** `crates/flui-app/src/overlay/` and its `pub mod overlay;` are gone, along with the `flui-app/README.md` bullet describing them. Deletion, not renaming, because nothing referenced the module — not one call site, not one test — and it stored no view content, so there was no behavior to preserve. `cargo check --workspace --all-targets` and the full `cargo nextest run --workspace --exclude flui-platform` (5780 tests) pass with no other edit, which *is* the proof that it was dead. The workspace now defines exactly one `OverlayEntry`. Note this module carried a `PORT-CHECK-OK-SP4` waiver on trigger 11, so the speculative-scaffolding gate had been explicitly opted out of rather than passed.

### 3.5 Genuinely missing

| Missing | Consequence |
|---------|-------------|
| `Overlay`, `OverlayEntry` (real), `Route`, `Navigator` | zero implementation; this ADR's subject |
| **`Focus` / `FocusScope` widgets** | H4 landed `FocusManager`/`FocusScopeNode` in `flui-interaction`, but **no widget exposes them**. `ModalRoute`'s focus scope (`routes.dart:1095`, `:1201`) cannot be built. Focus is deferred (§6). |
| **Offstage-with-real-layout** | `RenderOffstage` (`flui-objects/src/interaction/offstage.rs:40`) collapses to zero size and skips hit-test. `ModalRoute.offstage` (`routes.dart:1949`) needs *laid out at real size, not painted* — it exists so `HeroController` can measure final hero geometry. **This is a Hero blocker, not a Navigator blocker.** |
| Restoration | no `RestorationManager` anywhere in the workspace. Deferred; Flutter gates it behind `restorationScopeId`, so absence is invisible. |

---

## 4. The one divergence Rust forces: the pop result crosses `dyn Any`

Flutter's `_history` is `List<_RouteEntry>` where each entry holds a `Route<dynamic>`. `Navigator.pop<T>(context, result)` passes `result` down to `Route<T>.didComplete`, which completes `Completer<T?>`. The `T` is **unchecked at the call site** — a mismatched `pop` throws a cast error at completion. This is a known Flutter footgun.

Rust cannot have `Vec<Box<dyn Route<T>>>` for varying `T`. The stack must be `Vec<Box<dyn ErasedRoute>>`. The popper (a button deep inside route *N*'s subtree) knows its value's type; the Navigator does not. So the value must be erased somewhere. Three shapes:

| | Shape | Cost |
|---|---|---|
| **A** | `pop(result: Box<dyn Any>)`; the erased route downcasts to its `Output`. | Flutter-faithful, including the failure mode. Needs FR-036 (`dyn Any` **already allowlisted**, `port-check.sh:1103`) and an FR-033 downcast registration. A type mismatch is a runtime error, not a compile error. |
| **B** | `push<R: Route>(r) -> RouteResult<R::Output>`; `pop()` takes no value; the result is set through a typed handle captured at push. | Fully type-safe. But `Navigator::of(ctx).pop(value)` — the single most-used call in Flutter — becomes unwritable from inside the route's subtree without threading the typed handle down. Diverges from the core mental model rule #1 protects. |
| **C** | Erased internally (A), with a typed `pop_with::<T>()` front door that fails loudly on mismatch. | A, plus a name. |

**Recommendation: A**, with the mismatch surfaced per [`PANIC-POLICY.md`](../PANIC-POLICY.md) — `tracing::error!` and complete the future with `None` (which is already the return type: `Option<T>`, mirroring Dart's `T?`), rather than panicking. Flutter throws; FLUI logs and yields `None`. That is a **documented behavioral divergence on an error path**, and it is the honest one: `expect("BUG: …")` is reserved for internal invariants, and a wrong `pop` type is caller error, not a framework invariant.

This is a public API shape that **cannot be walked back once consumers exist** — the same category as ADR-0018's keyed identity. It therefore gets its own sign-off gate (§5, Gate 2) *before* any public export, not after.

Two smaller forced divergences, recorded now so they are not "discovered" later:

- **`popped` future.** Flutter returns `Future<T?>`. FLUI returns a one-shot `RouteResult<T>: Future<Output = Option<T>>`, polled by ADR-0018's `AsyncDriver` — reusing the `SharedSlot` shape, adding no dependency. `push` remains usable fire-and-forget.
- **Deferred subtree disposal.** Flutter defers `_RouteEntry.forcedDispose` until every overlay entry's element has unmounted, tracked in `_entryWaitingForSubTreeDisposal` and completed on a microtask (`navigator.dart:3464-3517`). FLUI's `on_unmount` is synchronous, so the set is unnecessary; disposal is immediate. Same observable result, no microtask.

---

## 5. Dependency-ordered implementation sequence

The reference confirms the shape the task proposed, with **one adjustment**: the parity/sign-off gate moves *before* public export, not after. That is not a preference — ADR-0017 U4 and ADR-0018 U6 both established that a public export made before the `.flutter/` cross-check is unrecoverable, and the [Definition of Done](../../AGENTS.md#definition-of-done-anti-cheating) requires the reference check to precede the parity claim. Export is the reward for the gate, not its precondition.

| Unit | Scope | Exit gate |
|------|-------|-----------|
| **U1** ✓ | **`Overlay` + `OverlayEntry`, private, unexported.** Built on the existing `RenderStack` with `StackFit::Expand` — **no new render object**. Entry list, `insert`/`insert_all`/`remove`/`rearrange`, per-entry `RebuildHandle` published at `init_state`. Build **every** entry (no `opaque`/`maintainState`/`skipCount`). Both preconditions discharged: `flui-app::overlay` deleted (§3.4), keyed reorder proven to preserve `ViewState` (§3.2). **Landed 2026-07-09** — see §7a. | ✓ 13 tests, each red-checked; gates green |
| **U2** ✓ | **Route data model + lifecycle + flush, private.** `RouteLifecycle` (16 states, `PartialOrd` range predicates), `RouteEntry`, `ErasedRoute`, `OverlayRoute`, the `flush_history_updates` algorithm (reverse walk, `can_remove_or_add`, `popped_route`/`seen_top_active_route`, deferred disposal), observer queues (**additions LIFO / deletions FIFO**), and the result channel (`RouteResult<T>`, `did_complete`, `result ?? current_result`). **No widget, no Navigator, no animation.** The flush is a pure function over the entry vec: it must be unit-testable with a fake observer and zero element tree. | §7 tests 3–9, 15–17; a re-entrancy guard (`flushing` flag + `debug_assert` the scheduler phase is not build/layout/paint, mirroring `Scheduler::drive_async_tasks`) |
| **U3** ✓ | **Private `Navigator` view/element.** `NavigatorState` owning the route stack behind a private `Mutex`; `NavigatorHandle`; `Navigator::of` / `maybe_of` / `of_root` **including the self-check of §3.3**; `initial_route` + `default_generate_initial_routes`. Still unexported. | §7 tests 1–2, 10–12 |
| **U4** | **Parity re-check against `.flutter/` → Gate 1. Then Gate 2 (below). Only then: public export** of `Navigator`, `Route`, `Overlay`, `OverlayEntry`, `NavigatorObserver` + prelude. Register `dyn ErasedRoute` in the FR-036 allowlist (`port-check.sh` trigger 9) and the result downcast under FR-033. | Full §7 suite through the public prelude; `just ci`; ADR updated with a *Parity findings (U4)* table |
| **U5** | **`TransitionRoute` / `ModalRoute` / `PageRoute`** — animation (`AnimationController` with `NavigatorState` as `TickerProvider`), `secondaryAnimation` + train-hopping, the pointer-absorbing barrier, `finishedWhenPopped => controller.is_dismissed`. **Its own parity gate** (`routes_test.dart` oracles). Likely deserves its own ADR — the train-hopping state machine (`routes.dart:422-496`) is not a small port. | `routes_test.dart` transitions oracles |

**Gate 1 (parity).** Re-read `navigator.dart` / `overlay.dart` / `routes.dart` at the pinned revision. Every §7 oracle passes or is a *documented* divergence. No export before this passes. If `.flutter/` is absent or has moved revision, **stop and report** — do not claim parity.

**Gate 2 (sign-off on the `dyn Any` pop result, §4).** The repo has **no api-design-lead role**; ADR-0018 U6 recorded its equivalent divergence as signed off by the **repository owner** in the task authorization, and said so plainly rather than implying a review that did not happen. The same applies here. **Option A is not yet authorized** — this ADR proposes it. If sign-off is absent when U4 begins, U4 stops at docs and tests: **no public export.**

---

## 6. Deliberately deferred, each with its blocker

| Deferred | Blocker | Why deferring is safe |
|----------|---------|----------------------|
| **`opaque` / `maintainState` / `skipCount`** | Needs a dedicated `RenderTheater` with an offstage-skipping `performLayout`. | Build-everything is *strictly more* state-preserving than Flutter, never less. **Named cost:** every covered route is built, laid out and painted — O(stack depth) wasted layout/paint per frame — and `maintain_state == false` is unobservable, so a route that Flutter would tear down when covered stays alive. **Upgrade path:** add `skip_count` to a `RenderTheater` and port `OverlayState.build`'s loop. Correctness of input isolation must come from the barrier (§2.4), not from unbuilding. |
| **Declarative `Router` / page-based Navigator 2.0** (`pages`, `RouteTransitionDelegate`, `onDidRemovePage`) | Needs the `Page` model + `TransitionDelegate`. | Flutter ships both APIs side by side; the imperative one is not a subset that later breaks. `_RouteLifecycle::staging` (`navigator.dart:3139`) exists *only* for the TransitionDelegate and is **omitted** from U2's enum until pages land — recorded here so its absence reads as a decision, not an oversight. |
| **Named-route generation** (`onGenerateRoute`, `routes` map, `pushNamed`) | None technical. | Purely additive on top of `RouteSettings`, which U2 ports. |
| **Restoration** | No `RestorationManager` exists in the workspace (verified: zero occurrences). | Flutter gates all of it behind `restorationScopeId`; absent, `restoreState` reduces to the `initialRoute` path (`navigator.dart:3895`) — which is what U3 ports. |
| **Focus traversal / `FocusScope` per route** | **No `Focus`/`FocusScope` widget exists.** H4 landed `FocusManager`/`FocusScopeNode` in `flui-interaction` but never exposed a widget. | Faking it (driving `FocusManager` directly from a route) would be exactly the "MVP reported as parity" failure the Definition of Done names. A route without a focus scope is *visibly* keyboard-incomplete, not silently wrong. Needs its own widget-layer slice first. |
| **Predictive back / user gestures** (`didStartUserGesture`, `animateBackWith`) | Platform channel + `AnimationController` back-animation. | Additive; observer methods are no-op defaults. |
| **`LocalHistoryRoute`** | None. | Additive mixin (`routes.dart:747`); affects only `willHandlePopInternally`, which `can_pop` already consults. |
| **`OverlayPortal`** | None. | **Proven not required** (§2.3): no route touches it. |
| **`Hero`** | **Four blockers, all real:** (1) Navigator + Overlay must exist; (2) `HeroController` *is* a `NavigatorObserver` — the observer API (U2) must land first; (3) it needs `ModalRoute.offstage` — laid out at real size, unpainted — which `RenderOffstage` **cannot express** (it zero-sizes, §3.5); (4) it needs GlobalKey reparenting *across overlay entries* plus `createRectTween` flight geometry. | Starting Hero before all four are settled would fix a wrong shape. Hero gets its own ADR, after U5. |

---

## 7. Tests that will prove the behavior

Each is red-checkable: the named change to the implementation must turn it red. Names in `«»` are the Flutter oracles they transcribe.

**Overlay (U1)**

1. `overlay_last_entry_paints_on_top` — insert A then B; a paint probe records B after A. *Red-check:* reverse the child order.
2. `overlay_insert_above_below_and_rearrange` — «`insert top`», «`insert below`», «`insert above`», «`insertAll top`», «`rearrange`», «`rearrange above`», «`rearrange below`». *Red-check:* make `rearrange` append instead of preserving unmentioned relative order.
3. `overlay_rearrange_is_a_noop_when_order_is_unchanged` — the `listEquals` short-circuit (`overlay.dart:833`): no rebuild is scheduled. *Red-check:* delete the guard; the entry's rebuild handle fires.
4. `overlay_entry_rebuilds_alone` — `mark_needs_build` on entry B rebuilds B only, not A. *Red-check:* route it through the Overlay's own handle.
5. `overlay_keyed_reorder_preserves_entry_state` — **the §3.2 precondition.** A stateful child in entry A keeps its state after A moves below B. *If this fails, `GlobalKey` is required and §3.2(b) must be resolved.*

**Route model + flush (U2)**

6. `route_lifecycle_ordering` — a probe route records `install` → `did_push` → observer `did_push`. *Red-check:* notify the observer inline in `handle_push`; the order inverts. «`Route didAdd and dispose in same frame work`»
7. `observer_additions_drain_lifo_deletions_fifo` — the §1.3 asymmetry, with ≥2 additions and ≥2 deletions in one flush. *Red-check:* swap `pop_back`/`pop_front`. «`initial route trigger observer in the right order`», «`Push and pop should trigger the observers`»
8. `observers_notified_before_neighbour_announcements` — observer `did_push` precedes the neighbour's `did_change_next`. *Red-check:* reorder `_flushObserverNotifications` / `_flushRouteAnnouncement`.
9. `pop_delivers_result_to_the_pushing_caller` — `push(...)` resolves to `Some(v)`. «`popUntilWithResult return value to the last popped route`»
10. `pop_without_result_falls_back_to_current_result` — the `result ?? currentResult` clause (`navigator.dart:481`). *Red-check:* drop the fallback; a route overriding `current_result` regresses to `None`.
11. `removed_route_still_completes_its_future` — `remove_route` completes the `push` future. **The §1.5 hang.** *Red-check:* complete only in `did_pop`; the test deadlocks/times out. «`remove a route whose value is awaited`», «`remove route below an other one whose value is awaited`»
12. `push_replacement_reports_did_replace_not_did_remove` — `_reportRemovalToObserver == false` when replaced (`:3435`). *Red-check:* always report removal. «`pushReplacement correctly reports didReplace to the observer`»
13. `did_change_next_and_previous_reach_neighbours` — with the right arguments, and **not** re-sent when unchanged (`lastAnnounced*` caches, `:4655`). *Red-check:* drop the cache; announcements duplicate.
14. `dying_route_still_receives_its_final_announcement` — disposal deferred past `_flushRouteAnnouncement` (`:4571`, `:4609`). *Red-check:* dispose inside the loop.
15. `push_pop_replace_in_sequence` — «`Can push, pop, and replace in sequence`», «`Route management - push, replace, pop sequence`»
16. `no_stale_entries_after_route_disposal` — after a pop, the overlay's entry list is empty, the route is disposed, and its entry's `RebuildHandle` is inert. *Red-check:* skip `overlay_entry.remove()` in `dispose_route_entry`.
17. `flush_is_rejected_during_build_layout_paint` — the re-entrancy guard `debug_assert`s, mirroring `Scheduler::drive_async_tasks`.

**Navigator (U3/U4)**

18. `first_route_builds` — `initial_route` is present and laid out after the first frame, having received `did_add` (not `did_push`), while observers still saw a push observation (§1.8).
19. `push_keeps_the_route_beneath_mounted` — the covered route's `ViewState` is **not** disposed. *Red-check:* build only the top entry. «`Can navigator navigate to and from a stateful widget`»
20. `can_pop_and_maybe_pop` — one route → `can_pop() == false`, `maybe_pop()` returns `false` (bubble); two routes → both `true`; a `doNotPop` route → `maybe_pop()` returns `true` having fired `on_pop_invoked(false, …)`.
21. `nested_navigator_lookup` — `Navigator::of` finds the nearest, `of_root` the outermost. «`Navigator.of rootNavigator finds root Navigator`»
22. `navigator_of_from_the_navigators_own_context` — the §3.3 self-check. *Red-check:* delegate straight to `find_ancestor_state`; the enclosing navigator is returned.
23. `maybe_of_returns_none_when_absent` — no panic. «`Navigator.of fails gracefully when not found in context`»
24. `deep_initial_route_synthesizes_a_back_stack` — `'/a/b'` → three entries (§1.8). *Red-check:* push only the leaf; `can_pop()` regresses to `false`.
25. `pop_with_the_wrong_result_type_logs_and_yields_none` — the §4 divergence, pinned so it can never silently become a panic.

---

## 7a. Implementation findings (U1, 2026-07-09)

`Overlay` / `OverlayEntry` landed in `crates/flui-widgets/src/overlay/`, `pub(crate)`, unexported. 13 tests, each red-checked. The design in §2–§3 survived contact; four things it did not say:

1. **`Stack` was sufficient — no render object added.** As §2.2 predicted: `Stack::new(children).fit(StackFit::Expand)`. Nothing in `flui-objects` or `flui-rendering` was touched.

2. **An overlay rebuild reruns *every* surviving entry's builder; only `mark_needs_build` is targeted.** My first `removed_entry_…` test asserted the untouched entry would not rebuild after a sibling was removed. It rebuilt, and Flutter agrees: `OverlayState.build` constructs a fresh `_OverlayEntryWidget` per entry, each wrapping a fresh `Builder(builder: widget.entry.builder)` (`overlay.dart:424-427`), so a `setState` on the overlay rebuilds all of them. `OverlayEntry.markNeedsBuild` is the only path that rebuilds one layer alone (pinned by `overlay_mark_needs_build_rebuilds_only_that_entry`). The test was wrong, not the code.

3. **A `removed` flag was written, red-checked, and deleted.** It guarded `mark_needs_build` in the window between `remove()` and the frame that unmounts the layer. Deleting it broke no test: the overlay's own rebuild removes the child *before* the drained dirty id is processed, and `RebuildHandle::schedule` already documents a vanished element as a no-op. Rather than ship defensive code no test could reach — the ADR-0018 U4 generation-guard precedent — it is gone. What *does* make a removed entry inert is `remove()` **taking** the overlay back-reference (`Option::take`), so a second `remove()` cannot schedule a second overlay rebuild; that is red-checkable and tested.

4. **Flutter's mid-frame deferral has no analogue.** `OverlayEntry.remove` posts a post-frame callback when it runs during `persistentCallbacks` (`overlay.dart:236-242`), because Dart's `setState` throws during build. `RebuildHandle::schedule` only inserts an id into an inbox drained by the next `build_scope`, so it is already safe from any phase and any thread. Nothing to port. This is the first concrete payoff of ADR-0018's seam beyond async builders.

Two notes for whoever writes U2/U3:

- **`ElementTree::update` dispatch is keyed by `TypeId`**, so `HeadlessBinding::swap_root_view` cannot swap the root to a *different* view type. Unmounting a subtree in a test means toggling a field on one root type. (`stale_overlay_handle_is_harmless` does this via a `Host { show_overlay: bool }`.)
- The module carries `#![allow(dead_code)]` because `Navigator` (U3) is its only intended consumer and does not exist yet. **Delete that attribute in U3**, exactly as ADR-0018 U6 deleted its own, so genuinely-dead code cannot hide behind it.

Deliberately still absent, and not claimed: `opaque` / `maintainState` / `skipCount` (§6), and `rearrange`'s `above:` / `below:` placement of the unmentioned group — `Navigator._flushHistoryUpdates` passes neither (`navigator.dart:4612`). `overlay_deferred_opaque_builds_every_entry` pins the current build-everything behavior so that implementing skipping turns it red.

## 7b. Implementation findings (U2, 2026-07-09)

The route stack landed in `crates/flui-widgets/src/navigator/` (`lifecycle.rs`, `route.rs`, `result.rs`, `observer.rs`, `history.rs`), `pub(crate)`, unexported. No `Navigator`, no widget, no element tree. 26 tests, each red-checked. The §1 reading held; six things it did not say.

1. **`popping` sits *after* `remove` in Flutter's declaration order** (indices 11 and 10). This is not a curiosity. It is why a popping route is not `isPresent`, and it is why `_RouteEntry.complete`'s `>= remove` early-return (`navigator.dart:3431`) already refuses to re-arm a route that popped with an exit transition pending. I initially attributed that protection to the completer's own guard; a red-check proved otherwise. `pop_then_remove_of_an_animating_route_completes_exactly_once` now asserts the ordering explicitly so the surprise is recorded rather than rediscovered.

2. **Two completion guards are unreachable through the history, and are tested at their own layer rather than deleted.** `Completer::complete`'s reject-second-completion and `RouteRecord::did_complete`'s idempotence are both shadowed by the `>= remove` guard above. Unlike U1's `removed` flag — deleted because it protected nothing — these *are* the contract of the types they live on (a one-shot channel; an idempotent erased hook that U3 will call directly). They keep their guards and get direct unit tests, the ADR-0018 U4 `apply_fold` posture. Deleting either turns a test red.

3. **The `pushing` arm of the flush is only reached by a *later* flush.** A route entering as `Push`/`PushReplace` is handled by that arm and parks in `Pushing`; the `Pushing` arm runs only if another flush passes over it while the transition is still in flight. My first test for "an animating push defers disposal of the replaced route" therefore passed even with `can_remove_or_add = true` wrongly added to the `Pushing` arm. Fixed by driving a redundant `flush()` mid-transition. Worth knowing before U5 wires real animations.

4. **`PushCompletion::Immediate` settles inside the first flush; Flutter takes a microtask.** Flutter parks even a zero-duration push in `pushing` and flips it to `idle` on a microtask, forcing a second flush (`:3276-3290`). FLUI has no event loop at this layer, so an immediate push goes straight to `Idle` — reusing the same code path the `replace` branch already takes. The end state and **the entire observer stream** are identical; only the *dispose* of a route sitting in `Removing` moves one flush earlier. Recorded on `PushCompletion::Immediate`. `Animating` + `notify_push_completed` is the faithful path, and is what U5's `TransitionRoute` will use.

5. **Two structural divergences, both forced by "pure data":**
   - The **overlay rearrange is hoisted out of the flush** into the caller. Flutter ends `_flushHistoryUpdates` with `overlay?.rearrange(...)` (`:4612`), *after* disposal. U2 returns a `FlushOutcome { rearrange_overlay, disposed }` and U3 performs it, preserving that order. The `rearrangeOverlay: false` that `pop`/`removeRoute` pass thus has nothing to select yet; it is carried on the outcome.
   - **Routes are named by `RouteId`, not by object**, in observer callbacks and in `did_change_next` / `did_change_previous` / `did_pop_next`. Handing out `&mut dyn ErasedRoute` for one entry while the history holds the rest is not expressible. Ids preserve identity, ordering and arity — everything the oracles assert. **U5 will need more:** `TransitionRoute._updateSecondaryAnimation` needs the *next route's animation* (`routes.dart:422-496`), so it needs a lookup handle. Flagged now, not discovered then.

6. **`staging` stayed omitted, and `disposing` joined it.** Re-checked against the reference: `staging` is written only by `_updatePages`/`RouteTransitionRecord`, read only by `isWaitingForEnteringDecision`, and `_flushHistoryUpdates` `assert(false)`s on it (`:4576`) — so with page-based routing deferred it has no producer and no consumer. `disposing` exists solely because Dart must wait for overlay-entry elements to unmount on a later microtask (`_entryWaitingForSubTreeDisposal`, `:3464-3517`); FLUI's unmount is synchronous, so `dispose` is terminal, and the flush `assert(false)`s on `disposing` too. The enum has 14 variants, not 16. Because the predicates are **named ranges** over declaration order, re-adding either variant later shifts nothing.

Two notes carried forward:

- **`flui-widgets` is outside port-check's FR-036 (`dyn`-boundary registry, trigger 9) and FR-033 (downcast) scopes.** So the `Box<dyn Any + Send>` pop-result boundary landed **unguarded by any gate**. That is a reason to keep it private until U4 Gate 2 rules on it — not a licence. If U4 authorizes the public shape, extending one of those scopes to cover it is the natural follow-up.
- The re-entrancy guard (`assert!` with `BUG:`) is **structurally unreachable through U2's surface**: a `Route` hook receives only `&mut self` and cannot reach the history. It is tested directly via a `#[cfg(test)]` `force_flushing_for_test`, and exists for U5, where a zero-duration transition completes inside a flush. Stated plainly rather than implied to be covered by the widget path.

Deliberately absent, and not claimed: animation, barrier, focus, `Hero`, page-based routing, restoration, named-route generation, `canPop`/`maybePop`, `LocalHistoryRoute`, and `RouteSettings.arguments` (a second erased `dyn Any` field, held back until the first one is ruled on).

## 7c. Implementation findings (U3, 2026-07-09)

`Navigator`, `NavigatorState`, `NavigatorHandle`, `NavigatorRoute` and `SimpleRoute` landed in `crates/flui-widgets/src/navigator/{navigator,overlay_route}.rs`, `pub(crate)`, unexported. 17 widget-level tests on top of U2's 26. The design of §3.2 held exactly; five things it did not say.

1. **`Navigator::of` works, and needs nothing but `find_state`.** `ctx.find_state::<NavigatorState, _>(NavigatorState::handle)` clones an owned, `'static` handle out under the tree borrow and does nothing else there. No `GlobalKey`, no second lock, no `&mut NavigatorState`. `maybe_of_root` is the same over `find_root_state`. `navigator_uses_no_global_key` greps the sources so it stays that way; `nested_navigator_lookup_prefers_nearest_and_root_finds_outermost` pins the two apart.

2. **Correction to §3.3: Flutter's self-check is not merely unimplemented, it is unreachable — and unimplementable.** §3.3 said `Navigator::of` "must close this gap in its own body (check self, then walk)". It cannot. `walk_strict_ancestors` starts at the parent, and during `build` the element's own node is a **hole** (`element_opt` returns `None`), so no `BuildContext` API can reach its own state. Flutter's self-check only matters for a context obtained from `GlobalKey<NavigatorState>.currentContext` — which FLUI does not have. The case is therefore unreachable, and §3.3's second gap (`find_root_ancestor_state` lacking a `?? navigator` fallback) is likewise unreachable: the root walk cannot find fewer navigators than the nearest walk. Both are recorded, not papered over.

3. **The overlay entry lives on the navigator, not the route.** Flutter's `OverlayRoute` owns `List<OverlayEntry>` and the navigator reads `route.overlayEntries` (`:4151`). FLUI's route sits behind `Box<dyn ErasedRoute>` inside `RouteHistory`, and exposing overlay entries there would break U2's pure-data invariant that `route_stack_flush_is_pure_data` enforces. So `NavigatorState` keeps a `RouteId -> OverlayEntry` map and the route supplies only the *builder* (`NavigatorRoute::overlay_builder`). `_allRouteOverlayEntries` becomes a map lookup over `RouteHistory::ids()` — same order, same contents. One consequence: Flutter removes a route's overlay entries *before* `entry.dispose()` (`:3978-3987`), FLUI disposes inside the flush and removes just after. Nothing observes it, because a FLUI route holds no reference to its entry.

4. **`FlushOutcome` grew `disposed: Vec<RouteId>`** (it was a `usize`). `NavigatorShared::apply` transcribes the tail of `_flushHistoryUpdates` (`:4609-4613`): remove each disposed route's overlay entry, *then* rearrange — and only when the flush asked for it, because `pop` and `remove_route` pass `rearrangeOverlay: false` (`:5671`, `:5747`) precisely since `OverlayEntry.remove()` already updated the overlay's own list. Moving the rearrange above the removal loop turns a test red.

5. **A leak only one assertion could see.** Removing a disposed route's entry from the overlay leaves the overlay *looking* correct even if the entry stays in the navigator's own map — the overlay had removed it from its list. My first `navigator_drops_overlay_entries_of_disposed_routes` therefore passed with `entries.get(id)` in place of `entries.remove(id)`, i.e. with an unbounded memory leak. Fixed by adding `tracked_entry_count()` and asserting it tracks the route count.

Also landed: `can_pop` and `maybe_pop`, transcribed from `:5551-5566` and `:5582-5615`. `maybePop` is `async` in Dart **only** because of the deprecated `willPop` await; the remaining logic is a synchronous `switch` on `popDisposition`, so FLUI's is a plain `fn` — porting it as `async fn` would buy nothing and violate the no-async-in-hot-paths rule. `RoutePopDisposition::DoNotPop` is modelled but has no producer until `PopScope` lands.

**The `#![allow(dead_code)]` could not be removed, and the reason is worth stating.** U3 wires `navigator` to `overlay`, but *nothing outside the tests reaches `Navigator`* — so from rustc's reachability view the whole subtree is still dead. It goes when U4 exports. The `#[allow(unused_imports)]` block U2 needed **is** gone: the re-exports it covered were themselves unused and were deleted.

Two smaller notes. The in-crate harness was factored out of `overlay/tests.rs` into `src/test_harness.rs` (`#[cfg(test)]`), shared by both private modules — a third copy was the alternative. And `navigator_private_no_prelude_export` guards U4's gate mechanically by reading `lib.rs`; red-checked with `pub use stack::Stack as Navigator;`.

Still not implemented, and not claimed: `TransitionRoute` / `ModalRoute` / `PageRoute` (no animation, no barrier, no focus scope), `Hero`, page-based routing, restoration, named-route generation, `PopScope`, `LocalHistoryRoute`, and everything Flutter's `NavigatorState.build` wraps the `Overlay` in (`HeroControllerScope`, `NavigationNotification`, the pointer-cancelling `Listener`, `FocusTraversalGroup`).

## 8. Consequences

**Good.** Navigator needs no new framework seam: ADR-0017 and ADR-0018 already paid for the two capabilities it requires (a `'static` rebuild handle, and the publish-a-handle-into-a-shared-cell pattern). The flush algorithm is pure data and ports 1:1. Overlay needs no new render object. Both of Flutter's `GlobalKey` uses dissolve, and with them the lock hazard of §3.2.

**Bad.** The pop result must cross `dyn Any` (§4) — a runtime failure mode where Flutter also has one, but which Rust otherwise would not have needed. `_RouteLifecycle::staging` is omitted until page-based routing lands, so U2's enum is 15 states, not 16, and re-adding it later touches the range predicates.

**Ugly.** §3.2's `UNVERIFIED` GlobalKey-registry nesting remains a latent hazard in code that ships today — `Overlay` (U1) merely declines to walk into it, since it uses no `GlobalKey` at all. Someone should still check whether production `build_scope` holds `WidgetsBindingInner`'s write lock across a build; if it does, `GlobalKey::current_element()` from inside a build already deadlocks, and that is a bug independent of this ADR. (`flui-app::overlay` is resolved: deleted in U1.)

---

## Open questions for the deciders

1. **Gate 2:** is Option A (`dyn Any` pop result, error-logged mismatch, `Option<T>`) authorized? Without it, U4 stops at docs and tests.
2. **`flui-app::overlay`:** delete, or rename to `FloatingLayerManager`? It is dead and unwired either way.
3. **U5 scope:** does `TransitionRoute` + train-hopping warrant its own ADR? (My reading of `routes.dart:422-496` says yes.)
