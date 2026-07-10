# ADR-0021: `Hero` does **not** reparent anything — it builds a second copy in the overlay. The real blocker is that FLUI cannot compute a render object's rect in an ancestor's coordinate space

*Every prior document in this repo — ADR-0019 §6, tracker B1.4 — records `Hero`'s fourth blocker as "GlobalKey reparenting **across overlay entries**". Reading `heroes.dart` shows that is not what Flutter does. The hero's subtree never leaves its route: it is wrapped in an `Offstage` and stays mounted. The thing that flies is a **freshly inflated widget** in a new `OverlayEntry` — the default shuttle is literally `toHero.child` (`heroes.dart:1089`), a second element with its own state. The only `GlobalKey` in `heroes.dart` (`:363`) never crosses an overlay entry; it sits inside one hero's own `build`.*

*So the reparenting blocker is imaginary, and the geometry blocker is worse than recorded. Flutter measures both heroes with `box.getTransformTo(routeRenderObject)` (`heroes.dart:506`) and re-measures the destination every tick with `localToGlobal(Offset.zero, ancestor: …)` (`:673`). FLUI has neither: `RenderBox::local_to_global` is an identity stub (`render_box.rs:192-199`), and no render object implements `applyPaintTransform`. That is the seam Hero is actually waiting on, and it lives in `flui-rendering`, not in `flui-view`.*

---

- **Status:** Proposed — **design only.** No code, no public export. `Hero` remains blocked (tracker B1.4).
- **Date:** 2026-07-09
- **Deciders:** chief-architect; consult rendering owner (**S4**, the ancestor-relative paint transform — the one seam that cannot be worked around), view owner (**S6**, post-frame callbacks from a view; **S7**, hero discovery without an element walk), animation owner (flight animation composition, `ProxyAnimation` re-parenting mid-flight), repository owner (public API: `Hero`, `HeroController`, tag type), qa-lead (deterministic flight tests through a real `Vsync`).
- **Relates to:** consumes ADR-0019 (Navigator, Overlay, observers) and ADR-0020 (`PageRoute`, `ModalRoute.offstage`, `RenderOffstage`, `RenderTheater`). **Corrects** ADR-0019 §6 blocker (4) and tracker B1.4 precondition (4).
- **Blocks:** tracker B1.4.
- **Gate:** ARCH-GATE (this doc). Each slice below carries its own DEV-GATE; the public surface has a parity + sign-off gate at U6, per the [Definition of Done](../../AGENTS.md#definition-of-done-anti-cheating).

---

## Reference

Read at master `3.33.0-0.0.pre-6280-g88e87cd963f`.

| File | What was read, and why |
|---|---|
| `packages/flutter/lib/src/widgets/heroes.dart` (1154 ln) | **In full.** `Hero`, `_HeroState`, `_HeroFlightManifest`, `_HeroFlight`, `HeroController`, `HeroMode`. |
| `packages/flutter/test/widgets/heroes_test.dart` (3937 ln) | Test titles in full; `'Stateful hero child state survives flight'` (`:1674`), `'Heroes animate should hide original hero'` (`:674`), `'Heroes are not interactive'` (`:880`), `'One route, two heroes, same tag, throws'` (`:1004`), `'Hero push transition interrupted by a pop'` (`:1063`), `'Destination hero disappears mid-flight'` (`:1233`), `'Destination hero scrolls mid-flight'` (`:1348`), `'Aborted flight'` (`:1543`) read in full or in the parts that fix behavior. |
| `packages/flutter/lib/src/widgets/routes.dart` | `ModalRoute._subtreeKey` (`:2268`), `subtreeContext` (`:1966`), `offstage` setter (`:1949-1962`). |
| `packages/flutter/lib/src/widgets/navigator.dart` | `NavigatorObserver` (`:777+`), `observer.navigator`, `userGestureInProgressNotifier`. |
| `packages/flutter/lib/src/widgets/pages.dart` | `PageRoute` — the flight's route type gate (`_maybeStartHeroTransition` returns unless **both** routes are `PageRoute`). |

FLUI sources read: `crates/flui-view/src/key/{global_key.rs,registry.rs}`, `crates/flui-view/tests/{global_key.rs,global_key_reparent.rs}`, `crates/flui-view/src/context/build_context.rs`, `crates/flui-view/src/binding.rs`, `crates/flui-rendering/src/traits/render_box.rs`, `crates/flui-rendering/src/{storage/tree.rs,pipeline/owner/accessors.rs,testing/inspect.rs,view/render_view.rs}`, `crates/flui-objects/src/{layout/theater.rs,interaction/offstage.rs}`, `crates/flui-widgets/src/navigator/*`, `crates/flui-widgets/src/overlay/*`, `crates/flui-scheduler/src/scheduler.rs`, `crates/flui-foundation/src/key.rs`, `crates/flui-geometry/src/matrix4.rs`.

---

## 1. What Flutter actually does, in the order it does it

Verified against the reference; every step cites a line.

1. `HeroController extends NavigatorObserver` and reacts to **`didChangeTop`** (`heroes.dart:854`), plus `didStartUserGesture` / `didStopUserGesture`.
2. `_maybeStartHeroTransition` (`:911`) returns immediately unless **both** routes are `PageRoute` (`:915-919`) and the direction is decidable from the two routes' animation statuses (`:924-932`).
3. Unless this is a user-gesture pop of a `maintainState` route with a valid size (`:955-960`), it sets **`toRoute.offstage = toRoute.animation!.value == 0.0`** (`:967`) and defers to a **post-frame callback** (`:968`). The comment says why: *"Putting a route offstage changes its animation value to 1.0. Once this frame completes, we'll know where the heroes in the `to` route are going to end up."*
4. In the post-frame callback, `_startHeroTransition` (`:979`) sets `to.offstage = false` (`:987`), grabs `navigator.overlay` and `navigatorRenderObject.size`, then collects `Hero._allHeroesFor(subtreeContext, …)` for **each route's own subtree** (`:1013-1020`) — an element walk (`_allHeroesFor`, `:279`) that skips `HeroMode(enabled: false)` subtrees (`:335`) and asserts on duplicate tags within one route.
5. For each tag present in **both** maps it builds a `_HeroFlightManifest`, whose `fromHeroLocation` / `toHeroLocation` are `box.getTransformTo(routeRenderObject)` applied to `Offset.zero & box.size` (`:499-508`, `:514`, `:520`). A manifest is **invalid** if either rect is non-finite (`:529`), and an invalid manifest aborts rather than flies.
6. `_HeroFlight.start` (`:697`) points a `ProxyAnimation` at the driving route's animation (reversed for a pop), builds `heroRectTween`, calls `fromHero.startFlight(shouldIncludedChildInPlaceholder: push)` and `toHero.startFlight()`, and **inserts a new `OverlayEntry`** (`:735`).
7. `_HeroState.startFlight` (`:381`) measures its own `RenderBox.size` and `setState`s a `_placeholderSize`. Its `build` (`:411-437`) then returns a `SizedBox(width, height)` wrapping `Offstage(offstage: true, child: TickerMode(enabled: false, child: KeyedSubtree(key: _key, child: widget.child)))` — **the child stays mounted, laid out at its real size, unpainted** — *unless* `_shouldIncludeChild` is false, in which case the child is dropped entirely and a bare `SizedBox` is returned (`:423-424`).
8. The flight's overlay entry builds `AnimatedBuilder(animation: _proxyAnimation, child: shuttle, builder: … Positioned(… IgnorePointer(child: FadeTransition(opacity: _heroOpacity, child: child))))` (`:572-598`). `shuttle` is built once, lazily (`:573`).
9. Every tick, `onTick` (`:665`) re-reads the destination hero's origin via `toHeroBox.localToGlobal(Offset.zero, ancestor: toRoute.subtreeContext.findRenderObject())` and **rebuilds `heroRectTween` if it moved** (`:679-684`). If the destination is gone or unpaintable, the flight keeps flying while fading out (`:685-691`) — that is `'Destination hero disappears mid-flight'` and `'Destination hero scrolls mid-flight'`.
10. When the proxy animation stops animating (`:601-603`), the entry is removed and disposed, `fromHero.endFlight(keepPlaceholder: status.isCompleted)` and `toHero.endFlight(keepPlaceholder: status.isDismissed)` run, and the flight is dropped from `_flights`.

### 1.1 The load-bearing correction

**Nothing is reparented.** The two `GlobalKey` occurrences in `heroes.dart` are the *declaration* (`:363`) and the *use* (`:434`), both inside `_HeroState.build`. The key exists so that the hero's child element survives `build` returning a **different wrapper shape** (bare `SizedBox` ↔ `SizedBox → Offstage → TickerMode → KeyedSubtree`, or a caller-supplied `placeholderBuilder` tree). It never crosses an `OverlayEntry`.

The flying widget is a **second inflation**. The default shuttle is:

```dart
Widget _defaultHeroFlightShuttleBuilder(…) {
  final toHero = toHeroContext.widget as Hero;
  …
  return toHero.child;                       // heroes.dart:1089 (MediaQuery-less path)
}
```

`toHero.child` is a `Widget` — an immutable description. Inflating it under the flight's `OverlayEntry` creates a **new element with new state**. `'Stateful hero child state survives flight'` (`heroes_test.dart:1674`) does not contradict this: it asserts `find.text('456')` is `findsOneWidget` at every moment, which holds because the *from* hero's real child stays mounted but **offstage** (and `find` skips offstage by default), the *to* hero's placeholder has no child (`_shouldIncludeChild == false`), and the shuttle supplies the one visible copy. The `_shouldIncludeChild` comment states the actual guarantee: *"allowing the original element tree to be preserved"* (`heroes.dart:375-377`).

> **Therefore:** the instruction "do not fake Hero by rebuilding a second copy of the child" describes a Hero that Flutter does not have. Rebuilding a second copy **is** the reference behavior. What must not be faked is the *offstage preservation of the original subtree* and the *re-measured rect tween*. This ADR is loyal to the former reading, per the Prime Directive's "verify against `.flutter/`, not against comments" clause — including comments in our own ADRs.

---

## 2. The nine questions, answered

Each answer is tagged **VERIFIED** (I read the code or ran it), **DERIVED** (follows from verified facts, no test yet), or **UNKNOWN** (needs an experiment before U1 starts).

### Q1 — Does existing `GlobalKey` reparenting work across different `OverlayEntry` subtrees, or only among siblings / within one parent?

**VERIFIED (and the question is moot).** Two facts:

1. **Hero does not need it.** §1.1. No `GlobalKey` in `heroes.dart` crosses an overlay entry.
2. **The mechanism nevertheless exists and is cross-parent.** `flui-view` has `GlobalKey<T>` (`key/global_key.rs:50`) with a process-wide registry installed by `WidgetsBinding` in production (`binding.rs:576`) and by `test_only_set_global_key_registry` in tests (`lib.rs:113`). Reparenting is covered by:
   - `tests/global_key.rs::global_key_state_migrates_to_new_parent_slot`
   - `tests/global_key.rs::global_key_state_preserved_across_100_reparents`
   - `tests/global_key_reparent.rs::covers_sc003_reparent_emits_single_reparent_event` (inactive-queue reactivation, `from_parent: None`)
   - `tests/global_key_reparent.rs::covers_sc003_active_to_active_reparent_emits_from_parent_and_preserves_state` (**cross-parent, same-frame, active element**, `from_parent: Some(old)`)

   Overlay entries are ordinary sibling elements under one `Theater` in **one** `ElementTree`, so "across overlay entries" is exactly the cross-parent case those tests already cover.

**The missing test, named:** no test moves a `GlobalKey` between two *`OverlayEntry` subtrees of a mounted `Overlay`*. If a future feature (not Hero) needs it, the test to write is `crates/flui-widgets/src/overlay/tests.rs::global_key_subtree_migrates_between_overlay_entries`. Until it exists, the cross-overlay claim is **DERIVED**, not verified, and this ADR does not rely on it.

**Correction filed:** ADR-0019 §6 blocker (4) and tracker B1.4 precondition (4) both state Hero needs "GlobalKey reparenting across overlay entries". They are wrong. Both were written without cross-checking `heroes.dart` — ADR-0019 §2's own reference table says it read heroes.dart for *"Hero dependencies only"*.

### Q2 — Can a Hero subtree be lifted into the overlay without losing state / render links?

**VERIFIED: it is never lifted, so the question does not arise.** The subtree stays where it is, under `Offstage(offstage: true)`. ADR-0020 U5.0 already fixed `RenderOffstage` to lay its child out at real size while skipping paint, hit-test and semantics (`offstage.rs`, `excludes_semantics_subtree` at `:242`), which is precisely the mode `_HeroState.build` needs.

The *shuttle* is a fresh inflation with no render-link to the original. **DERIVED:** in FLUI this means the shuttle's subtree gets its own `ViewState`; a hero child holding non-idempotent state (a video position, a scroll offset) will visibly reset in the shuttle. Flutter has the same property and documents it obliquely. Not a divergence.

### Q3 — Does FLUI expose enough geometry to compute source/destination rects in global overlay coordinates?

**VERIFIED: no.** This is the real blocker.

What exists:
- `BuildContext::find_render_object() -> Option<RenderId>` (`build_context.rs:245`).
- `PipelineOwner::render_tree()` is public (`pipeline/owner/accessors.rs:198`); `RenderTree::parent(id)` (`storage/tree.rs:571`); `RenderNode::offset()` — the committed paint offset.
- `Matrix4::transform_rect` (`flui-geometry/src/matrix4.rs:456`).

What is missing:
- **No per-render-object `applyPaintTransform`.** The only `apply_paint_transform` in the tree is on `RenderView` (`view/render_view.rs:411`) — the root. A parent that paints its child through a matrix (`RenderTransform`, `RenderRotatedBox`, `RenderFittedBox`, `RenderFractionalTranslation`) contributes nothing to any accumulation today. *(U1 correction, §7a: three of those four already expose `paint_transform`, which the new default consults; the real overrides are `RenderFractionalTranslation` and `RenderFlow`.)*
- **`RenderBox::local_to_global` / `global_to_local` are identity stubs** (`traits/render_box.rs:192-199`): they return the point unchanged.
- **No `getTransformTo(ancestor)`** in any form.
- `box_geometry(owner, id)` — the only size reader — lives in `flui_rendering::testing`, which is `#[cfg(any(test, feature = "testing"))]` (`lib.rs:103-104`). Production code cannot call it.

A naive walk accumulating `RenderNode::offset()` from hero to route root would be **silently wrong** under any transforming ancestor, and a hero inside a `Transform` is exactly the case `MaterialRectArcTween` users hit. So this cannot be worked around at the widget layer. See **S4**.

### Q4 — Can `ModalHandle::set_offstage` be driven by `HeroController` for final geometry measurement?

**Not today; the behavior is right, the reachability is not.** ADR-0020 U5.3 implemented `ModalRoute.offstage` faithfully (offstage page laid out at real geometry, barrier suppressed) and U5.4 marked `ModalHandle` and its setters **`#[cfg(test)]`**, recording that "`Hero` (B1.4) is the seam's consumer". Three gaps:

1. `ModalHandle` is unreachable from a `NavigatorObserver`: the observer gets a `RouteId`, not a route object (`observer.rs:36-40`, which says so).
2. Flutter's `offstage` setter *also* swaps the route's animations to `kAlwaysComplete`/`kAlwaysDismissed` (`routes.dart:1959-1962`) — the mechanism that makes the offstage frame measure the hero at its **final** position. ADR-0020 §7d explicitly **did not implement** that swap, calling it Hero's problem. It is now Hero's problem. Without it, an offstage `to` route is laid out with `animation == 0`, and every measured `toHeroLocation` is the *entry* position, not the final one. **This is a correctness gap, not a plumbing gap.**
3. There is no post-frame callback reachable from a view or an observer (**S6**).

### Q5 — Where does `HeroController` live?

**Decision (see §4, D5): a `NavigatorObserver`, registered on the navigator, holding an attached `NavigatorHandle`.** Matching Flutter, where `HeroController extends NavigatorObserver` and reads `this.navigator`.

No `HeroControllerScope` in the first cut. Flutter needs it because a nested `Navigator` must be able to *deny* the ambient controller (one controller may serve only one navigator at a time; `HeroControllerScope.none` exists for that). FLUI has no nested-navigator story yet (`maybe_of_root` exists but nothing consumes it), and nested navigators are deferred (§8). An app-level default observer is likewise deferred until `flui-app` grows a `MaterialApp` analogue.

**Seam required (S1):** `NavigatorObserver` needs the navigator. Flutter's `NavigatorObserver.navigator` is set on registration. FLUI's `add_observer` takes an `Arc<dyn NavigatorObserver>` and never speaks to it again.

### Q6 — How are hero tags represented?

**Decision (D6): `Arc<dyn ViewKey>`.** `flui_foundation::ViewKey` (`key.rs:364`) already provides `key_eq`, `key_hash`, `clone_key` and `debug_fmt` — everything an `Object`-keyed map needs, with no downcast at the Hero layer.

Rejected:
- **`dyn Any` + downcast.** Would need a `PORT-CHECK-OK-DOWNCAST` marker and a widening of the FR-033/widgets guard, for no gain over `ViewKey`.
- **`Hero<T: Eq + Hash>` generic.** The controller's flight map is heterogeneous across tags; a generic hero cannot be stored in one map without erasure anyway.
- **`String`.** Loses `ValueKey<i32>`-style tags that Flutter supports, and invites accidental collisions.

**Guard consequence:** `Arc<dyn ViewKey>` in `flui-widgets` is a new erased boundary and needs an **FR-036 registry entry** in `scripts/port-check.sh` trigger #9 plus a row in `docs/PORT.md`. **No FR-033 change:** nothing downcasts. The `Hero` view's own `key()` is unrelated and unaffected.

### Q7 — What does the flight shuttle own?

**VERIFIED against the reference:** a **cloned view**, built once and cached (`heroes.dart:573`, `shuttle ??= …`), discarded and rebuilt only when a flight is diverted to a new route pair (`:793`, `shuttle = null` (`:793`) `… overlayEntry!.markNeedsBuild()`).

It does **not** own the original child, a reparented element, or the placeholder. The placeholder belongs to the *from* hero (`_placeholderSize` + `Offstage`); the shuttle belongs to the flight's own `OverlayEntry`.

In FLUI terms the shuttle is a `BoxedView` produced by a `HeroFlightShuttleBuilder` and re-inflated by the flight entry's builder. Because `BoxedView` is not `Clone`, the "build once and cache" trick has no direct analogue — the builder will re-run per frame, exactly as `ModalScope`'s page builder does (ADR-0020 §7e). Same cost, same non-divergence: element reconciliation preserves the shuttle's state across ticks.

### Q8 — What minimal public API should exist?

Deferred to the **U6 sign-off gate**; the *proposal*, to be argued then:

| Symbol | Proposed | Note |
|---|---|---|
| `Hero` | public | `tag`, `child`, and nothing else in U1. |
| `HeroController` | public | Constructed by the app, handed to `NavigatorHandle::add_observer`. |
| `HeroTag` (= `Arc<dyn ViewKey>` alias) | public | Q6. |
| `create_rect_tween` | public, **U4** | `Fn(Rect, Rect) -> Box<dyn Animatable<Rect>>`. Flutter puts it on both `Hero` and `HeroController`; keep both. |
| `flight_shuttle_builder` | public, **U4** | Q7's signature, minus the two `BuildContext`s Flutter passes (FLUI cannot hand out a foreign element's context safely — see §7 UNKNOWN-3). |
| `placeholder_builder` | public, **U5** | Forces the `GlobalKey` in `_HeroState` (§4 D2). |
| `HeroControllerScope` | **not exported** | Nested navigators deferred. |
| `HeroMode` | **not exported** in U1 | Cheap to add later; needs the registry to honour it (S7). |

Everything else — `HeroFlight`, `HeroFlightManifest`, the registry, the geometry seam's raw `Matrix4` walk — stays private.

### Q9 — Which Flutter behaviors are deferred?

See §8. Summary: nested navigators, user-gesture flights (`transitionOnUserGestures`, `didStartUserGesture`, `didStopUserGesture`, `userGestureInProgressNotifier`), `TickerMode` suppression, `HeroMode`, duplicate-tag assertion semantics, hero semantics, and `MediaQuery` padding compensation in the default shuttle.

---

## 3. The seams FLUI is missing

Ordered by how hard they are to fake. **S4 is the one that cannot be.**

### S1 — An observer cannot reach its navigator

Flutter: `NavigatorObserver.navigator` (set on registration). FLUI: `NavigatorObserver` receives `RouteId`s and nothing else, and `observer.rs:36-40` admits the gap in prose (*"`HeroController`, the one observer whose implementation needs the route object…"*).

**Proposed:**
```rust
pub trait NavigatorObserver: Send + Sync {
    /// Called once when the observer is registered, and again with `None` when
    /// the navigator unmounts. Flutter's `NavigatorObserver.navigator` setter.
    fn did_attach(&self, navigator: Option<NavigatorHandle>) {}
    // … existing did_push / did_pop / did_change_top …
}
```
`NavigatorHandle` is already the owned, `'static`, cloneable capability this repo hands out everywhere. No lock is taken under a tree borrow.

### S2 — A `RouteId` says nothing about the route

`HeroController` needs, for a given `RouteId`: is it a `PageRoute`; its primary animation; its `maintain_state`; the ability to set `offstage`; and the identity of its page subtree.

**Proposed, on `NavigatorHandle`:**
```rust
pub fn route_kind(&self, id: RouteId) -> Option<RouteKind>;        // Page | Popup | Plain
pub(crate) fn route_animation(&self, id: RouteId) -> Option<RouteAnimation>;
pub(crate) fn route_maintain_state(&self, id: RouteId) -> Option<bool>;
pub(crate) fn set_route_offstage(&self, id: RouteId, offstage: bool);
pub(crate) fn route_subtree(&self, id: RouteId) -> Option<RouteSubtree>;  // S3
```
`route_animation` can read the existing private `TransitionRegistry` (`binding.rs`), which already maps `RouteId -> TransitionPeer { animation, group, … }`. `TransitionGroup::Page` is already exactly "is a `PageRoute`" — the flight's route gate is a `group == Page` check, not a type test. That is a pleasant consequence of ADR-0020 §7e.

### S3 — A route has no addressable page subtree  *(this section's `init_state` claim is WRONG — see §7d)*

Flutter: `ModalRoute._subtreeKey` (`routes.dart:2268`) → `subtreeContext` (`:1966`), used for hero discovery **and** as the coordinate space of both hero rects.

FLUI does **not** need a `GlobalKey` for this. `ModalScope` (ADR-0020 U5.3) is already a stateful view owned by the route's `ModalInner`. It publishes its own `ElementId` and `RenderId` at `init_state` — the same owned-capability move as `RebuildHandle`, `OverlayHandle`, `RouteBindingSlot` — and clears them at `dispose`:

```rust
pub(crate) struct RouteSubtree { element: ElementId, render: RenderId }
```
Acquiring the ids in `init_state`, never in `build`/layout/paint, keeps port-check trigger #22 satisfied.

### S4 — **No ancestor-relative paint geometry.** The blocker.

Flutter needs exactly two operations:
- `RenderBox.getTransformTo(ancestorRenderObject)` → `Matrix4` (`heroes.dart:505`)
- `RenderBox.localToGlobal(Offset.zero, ancestor: …)` (`heroes.dart:669`) — a special case of the first.

Both are built on `RenderObject.applyPaintTransform(child, transform)`, which every transforming render object overrides. **FLUI has no such hook.** (`RenderView::apply_paint_transform` at `view/render_view.rs:411` is the root's, and unrelated.)

**Proposed, in `flui-rendering`:**
```rust
// on the RenderObject trait, defaulting to a pure translation by the child's
// committed paint offset — correct for every non-transforming render object.
fn apply_paint_transform(&self, child: usize, transform: &mut Matrix4);

// on PipelineOwner, walking `RenderTree::parent` from `descendant` up to
// `ancestor`, composing each step's `apply_paint_transform`.
pub fn transform_to(&self, descendant: RenderId, ancestor: RenderId) -> Option<Matrix4>;

// production readers, promoted out of the `testing` feature gate
pub fn box_size(&self, id: RenderId) -> Option<Size>;
```
Overrides required at minimum on `RenderTransform`, `RenderRotatedBox`, `RenderFittedBox`, `RenderFractionalTranslation`. Every other box render object inherits the default.

This is a `flui-rendering` change with its own harness obligations (every touched render object already has a `harness_*` test; each gains a transform assertion). **It should land as its own slice, before any Hero code, and it is useful independently** — `local_to_global` stops being a lie, and `RenderBox`'s two identity stubs get real implementations.

Scope note: `getTransformTo` between two nodes with a common ancestor (rather than a strict ancestor/descendant pair) is a superset Flutter supports. Hero only ever asks descendant→ancestor. Implement the narrow one; document the restriction.

### S5 — `HeroController` cannot reach the overlay

`NavigatorHandle::overlay_handle()` exists but is `#[cfg(test)]`. The flight needs `OverlayHandle::insert`, and `OverlayEntry::{new, mark_needs_build, remove}` — all present, all currently under the `overlay` module's `#![allow(dead_code)]`, which ADR-0020 §7d/U5.4 records as *"waiting for a consumer"*. Hero is that consumer.

**Proposed:** `pub(crate) fn overlay_handle(&self)`, reached through `S1`'s attached `NavigatorHandle`. `Overlay` / `OverlayEntry` stay unexported.

### S6 — No post-frame callback from a view or an observer

`Scheduler::add_post_frame_callback` exists (`scheduler.rs:945`). `BuildContext` exposes `rebuild_handle()` and `async_driver()` and nothing else (`build_context.rs:94`, `:107`). An observer has no context at all.

**Proposed:** a `PostFrameHandle` capability, published exactly like `RebuildHandle` (acquired in `init_state` by whatever view hosts the controller, or handed to the observer at `did_attach`). Trigger #22's rule generalises: acquire in a lifecycle hook, fire later.

**UNKNOWN-1 — ANSWERED `NO`, see §7b.** `pump_frame` never drains the post-frame queue (and never opens a scheduler frame); the production runner drains it **before** build/layout. Both paths are wrong, in different ways. U2 is blocked on a scheduler/binding fix (proposed as **U1.5** in §7b).

### S7 — Hero discovery: element walk vs. registry

Flutter walks the route subtree with `context.visitChildElements`, downcasting each `element.widget` to `Hero` (`heroes.dart:279`, visitor at `:318-340`).

FLUI *could* mirror this: `BuildContext::visit_child_elements(&mut dyn FnMut(ElementId))` exists (`build_context.rs:261`). But recovering "is this element a `Hero`, and what is its tag" from an `ElementId` requires a downcast of the view — a new `dyn Any` boundary needing FR-033 sign-off, for a walk that is `O(route subtree)` every transition.

**Proposed instead:** a `HeroRegistry` published by the route's `ModalScope` and resolved by each `Hero` at `init_state` (the ambient-scope pattern `VsyncScope` already uses). A `Hero` registers `tag -> HeroHandle` and deregisters at `dispose`. `HeroHandle` carries `start_flight`, `end_flight`, `render_id`, mirroring `_HeroState`'s methods.

Gains: no downcast, no FR-033 change, discovery is `O(heroes in route)`.

**DERIVED behavioral equivalences to prove, not assume:**
- **Duplicate tags.** Flutter asserts inside the walk. A registry detects the collision at *registration*, i.e. one frame earlier and at a different call site. The `'One route, two heroes, same tag, throws'` oracle must be re-expressed against [`PANIC-POLICY`](../PANIC-POLICY.md) — an app-author mistake, so `tracing::error!` + ignore the second, **not** a panic. Divergence, must be documented in the ADR's §7 when U2 lands.
- **`HeroMode(enabled: false)`.** Flutter's walk *skips the subtree*. A registry must make the hero not register (or deregister) when an ambient `HeroMode` is disabled — an inherited dependency, so `did_change_dependencies`. Deferred with `HeroMode` itself (§8).
- **Nested navigators.** Flutter's walk asks `Navigator.of(hero) == navigator`, and admits heroes of a nested navigator's top-most `PageRoute` (`heroes.dart:322-332`). A registry scoped to the *nearest* route naturally excludes them. Since nested navigators are deferred, this is a **narrowing**, not a divergence — but it must be stated, because "Hero works" would otherwise imply it.

### S8 — `Positioned` inside an overlay entry

`RenderTheater` (ADR-0020 U5.3) deliberately ignores positioned children: *"FLUI's `Overlay` builds one non-positioned `OverlayEntryView` per entry and nothing else, so every child here is non-positioned"* (`theater.rs` module docs). Flutter's flight entry is a `Positioned` (`heroes.dart:588`), inside an `IgnorePointer` (`:593`).

**Proposed: no theater change.** The flight entry builds `Stack::new(vec![Positioned(...)]).fit(StackFit::Expand)`. The theater lays the stack out with `tight(size)`; `RenderStack` already implements the full positioned split. One extra render object versus Flutter, identical geometry.

**UNVERIFIED.** This is a paper argument. Before U3 it must be checked that a `Positioned` at the root of an `OverlayEntry` builder does not instead attach its `StackParentData` to the theater's direct child and get silently dropped. The test to write: `overlay::tests::positioned_inside_an_entry_is_laid_out_by_an_inner_stack_not_the_theater`.

---

## 4. Decisions

**D1 — Hero does not reparent. The shuttle is a fresh inflation.**
Loyal to `heroes.dart:1089`. The offstage preservation of the *from* hero's subtree is the contract that matters, and ADR-0020 U5.0's `RenderOffstage` already provides it. Any implementation that instead moves the hero's element into the overlay is a **divergence** and must be rejected in review.

**D2 — No `GlobalKey` in U1–U4; introduce it only with `placeholder_builder` (U5).**
`_HeroState._key` exists to survive arbitrary wrapper-shape changes. Without `placeholder_builder`, FLUI's `HeroState::build` emits one fixed chain — `SizedBox → Offstage(flag) → child` — whose reconciliation preserves the child without any key. The moment a caller-supplied placeholder can restructure the tree, the `GlobalKey` becomes load-bearing.
**Red-check that will prove it:** with `placeholder_builder` supported and the `GlobalKey` removed, `hero_child_state_survives_a_placeholder_shape_change` must fail.

**D3 — The geometry seam (S4) lands first, alone, in `flui-rendering`.**
It is the only blocker with no widget-layer workaround, it is independently valuable, and it carries render-harness obligations that must not be smuggled into a widget PR.

**D4 — `offstage` must swap the route's animations, or the measurement is wrong.**
ADR-0020 §7d deferred `routes.dart:1958-1962`. Deferring it further would make every `toHeroLocation` the *entry* rect. `ModalRoute::set_offstage` must point its `_animationProxy` at `ALWAYS_COMPLETE` and its secondary at `ALWAYS_DISMISSED` while offstage. FLUI has both constants and `ProxyAnimation::set_parent`. **But `TransitionRoute`'s primary animation is the controller, unproxied** (`transition_route.rs`: *"only `secondaryAnimation` is a `ProxyAnimation`"*, matching `routes.dart:197`). So U3 must add a **primary proxy** on `ModalRoute` — not on `TransitionRoute` — exactly as Flutter puts `_animationProxy` on `ModalRoute` and not on `TransitionRoute`. Re-read `routes.dart:1740-1770` before implementing.

**D5 — `HeroController` is a `NavigatorObserver` with an attached `NavigatorHandle` (S1).** No scope, no app-level default, in this ADR.

**D6 — Tags are `Arc<dyn ViewKey>` (Q6).** FR-036 registry entry; no FR-033 change.

**D7 — Discovery is a registry, not an element walk (S7).** Behavioral narrowings enumerated and documented rather than hidden.

**D8 — Duplicate tags log and drop, they do not panic.** Per `PANIC-POLICY`: caller error, not a framework invariant. Divergence from Flutter's `assert`, recorded.

---

## 5. Implementation sequence

Each slice is independently mergeable. Nothing is public before U6.

| Slice | Content | Gate |
|---|---|---|
| **U1** ✅ | **S4, alone.** Landed 2026-07-10 — see §7a for two corrections to this row. `RenderObject::apply_paint_transform` (default = `paint_transform` ∘ translate-by-committed-offset) + overrides on `RenderFractionalTranslation` and `RenderFlow` (**not** the four named here) + `PipelineOwner::transform_to(descendant, ancestor)` + production `box_size`. `local_to_global`/`global_to_local` moved **off** the `RenderBox` trait onto `PipelineOwner`. No Hero, no widgets. | `cargo test -p flui-rendering`; `cargo test -p flui-objects --test render_object_harness`; new `harness_*_paint_transform` assertions on `RenderTransform`, `RenderRotatedBox`, `RenderFittedBox`, `RenderFractionalTranslation`; `just clippy`; `port-check`. |
| **U1.5** ✅ | **Landed 2026-07-10 (§7c).** `Scheduler::{drive_frame, end_frame, abort_frame}`; the async-driver poll moved into `handle_begin_frame` (ADR-0018 corrected). Both frame drivers share one ordering. |
| **U2** | **S1 + S2 + S3 + S5 + S6.** Observer↔navigator attachment; route introspection by id; `RouteSubtree` publication from `ModalScope::init_state`; `overlay_handle` reachable; `PostFrameHandle`. **Unblocked by U1.5.** `PostFrameHandle` must resolve to the *binding's* scheduler, not a global (§7c). Still no Hero. | `cargo test -p flui-widgets navigator`; the existing 98 navigator tests must not regress; new tests per §6. |
| **U3** | **D4 + S7 + S8.** `ModalRoute` primary animation proxy; `set_offstage` drives it. Private `HeroRegistry` + `Hero` view + `HeroHandle` (`start_flight`/`end_flight`, placeholder-`Offstage` build). Verify S8's `Positioned` claim. No controller, no flight yet: a `Hero` is a pass-through that can be told to show a placeholder. | `cargo test -p flui-widgets hero`; `cargo test -p flui-widgets navigator`; port-check FR-036 entry for `Arc<dyn ViewKey>`. |
| **U4** | Private `HeroController` + `HeroFlight` + `HeroFlightManifest`. Rect tween, `ProxyAnimation` driving, overlay entry insert/remove, `on_tick` re-measure, `create_rect_tween` and `flight_shuttle_builder` hooks (private). Push and pop only; no divert, no gestures. | `cargo test -p flui-widgets hero`; end-to-end through a real `Vsync` as `tests/routes.rs` does. |
| **U5** | Flight divert (push interrupted by pop, and the three-way cases at `heroes.dart:739-813`), abort/fade-out (`onTick`'s `_heroOpacity` path), `placeholder_builder` + the `GlobalKey` D2 requires. | `cargo test -p flui-widgets hero`; the divert oracles in §6. |
| **U6** | **Parity + API sign-off gate.** Cross-check every claim against `.flutter/`; decide the public surface (Q8); export; public tests through the prelude; update `docs/PORT.md`, tracker B1.4. | Full battery: `just ci`; `cargo test -p flui-widgets --test hero_public`; `RUSTDOCFLAGS="-D warnings" cargo doc`; `port-check -v`. |

---

## 6. Tests that will prove the behavior, and the mutation that must make each fail

Every row names the **red-check**: a change to the implementation that must turn the test red. A test with no red-check is not evidence.

### U1 — geometry

| Test | Red-check |
|---|---|
| `transform_to_accumulates_offsets_through_a_plain_chain` | Return `Matrix4::IDENTITY` from `transform_to`. |
| `transform_to_respects_a_render_transform_ancestor` | Delete `RenderTransform::apply_paint_transform` (fall back to the offset default). |
| `transform_to_respects_a_rotated_box_ancestor` | Same, for `RenderRotatedBox`. |
| `transform_to_returns_none_when_the_ancestor_is_not_an_ancestor` | Return `Some(IDENTITY)` on the walk falling off the root. |
| `local_to_global_is_no_longer_an_identity_stub` | Restore the stub body. |

### U2 — seams

| Test | Red-check |
|---|---|
| `observer_receives_the_navigator_on_attach_and_none_on_unmount` | Never call `did_attach`. |
| `route_animation_is_readable_by_id_for_a_page_route` | Return `None` from `route_animation`. |
| `route_kind_distinguishes_page_from_popup_from_plain` | Return `Page` unconditionally. |
| `modal_scope_publishes_its_subtree_ids_at_init_state_and_clears_them_at_dispose` | Skip the `dispose` clear; a stale `RenderId` must be caught. |
| `post_frame_callback_runs_after_layout_in_the_same_pumped_frame` | **This is UNKNOWN-1's experiment.** If it cannot be made to pass, U3's design changes. |

### U3 — hero + placeholder

| Test | Red-check |
|---|---|
| `hero_registers_its_tag_with_the_enclosing_route_and_deregisters_on_dispose` | Skip deregistration; a popped route's hero must not be discoverable. |
| `start_flight_makes_the_hero_show_a_placeholder_of_the_measured_size` | Return `Size::ZERO` from the measurement. |
| `an_offstage_hero_child_stays_mounted_and_laid_out_at_real_size` | Wrap the child in `SizedBox::shrink()` instead of `Offstage`. — *this is the D1 guard* |
| `end_flight_restores_the_child` / `end_flight_with_keep_placeholder_does_not` | Ignore `keep_placeholder`. |
| `duplicate_tags_in_one_route_log_and_drop_the_second` | Register both; the second must not silently win. |
| `positioned_inside_an_overlay_entry_is_laid_out_by_an_inner_stack` | **S8's verification.** Remove the inner `Stack`; if it still passes, the theater is honouring `Positioned` and the ADR is wrong. |
| `setting_a_route_offstage_drives_its_primary_animation_to_completed` | Skip the proxy swap — **D4's guard**. Without it `toHeroLocation` is measured at `animation == 0`. |

### U4 — flight

| Test | Flutter oracle | Red-check |
|---|---|---|
| `a_push_between_two_page_routes_with_a_matching_tag_starts_one_flight` | `'Heroes animate'` (`:396`) | Gate on `TransitionGroup::Page` inverted. |
| `no_flight_when_only_one_route_is_a_page_route` | `_maybeStartHeroTransition` `:917` | Remove the group check; a popup must not fly. |
| `the_from_hero_is_hidden_for_the_whole_flight` | `'Heroes animate should hide original hero'` (`:674`) | Never call `start_flight` on the from hero. |
| `the_flight_entry_is_removed_and_the_heroes_restored_when_the_animation_settles` | `_performAnimationUpdate` `:601` | Leak the entry. |
| `the_shuttle_is_not_hit_testable` | `'Heroes are not interactive'` (`:880`) | Drop the `IgnorePointer`. |
| `the_rect_tween_is_re_created_when_the_destination_hero_moves_mid_flight` | `'Destination hero scrolls mid-flight'` (`:1348`) | Make `on_tick` a no-op. |
| `a_flight_whose_destination_disappears_keeps_flying_and_fades_out` | `'Destination hero disappears mid-flight'` (`:1233`) | Abort the flight instead. |
| `an_invalid_manifest_aborts_rather_than_flies` | `isValid` (`:529`) | Drop the finiteness check; a `NaN` rect must not reach the tween. |
| `hero_flight_end_to_end_through_a_real_vsync` | — | The `tests/routes.rs` posture: no hand-driven controller. |

### U5 — divert

| Test | Flutter oracle |
|---|---|
| `a_push_flight_interrupted_by_a_pop_reverses_the_same_rect_path` | `'Pop interrupts push, reverses flight'` (`:2012`), and the comment at `heroes.dart:751-755` explaining why `ReverseTween` is used rather than a fresh tween with swapped ends |
| `overlapping_flights_for_the_same_tag_divert_rather_than_stack` | `'Overlapping starting and ending a hero transition works ok'` (`:969`) |
| `an_aborted_flight_ends_both_heroes` | `'Aborted flight'` (`:1543`) |
| `hero_child_state_survives_a_placeholder_shape_change` | D2's `GlobalKey` guard |

### U6 — public

`hero_public.rs`: `Hero` + `HeroController` reachable from `flui_widgets::prelude`; `HeroFlight`, `HeroRegistry`, `HeroHandle`, `Overlay`, `OverlayEntry` **not** exported (via `navigator::export_guard::assert_not_exported`, which matches whole identifiers — ADR-0020 §7e).

---

## 7. Verified, derived, unknown

**Verified against `.flutter/` (line-cited above):** the flight lifecycle (§1, ten steps); that no reparenting occurs (§1.1); that the shuttle is a second inflation of `toHero.child`; that `_placeholderSize` comes from the hero's own `RenderBox.size`; that both rects are measured in the *route's* coordinate space, not the overlay's; that the flight gate is `PageRoute` on both sides; that `offstage` is set before a post-frame callback and cleared inside it; that `onTick` re-creates the rect tween when the destination moves.

**Verified against FLUI (file:line above):** `GlobalKey` + registry exist and are production-wired; cross-parent reparenting is tested; `RenderBox::local_to_global` is an identity stub; no per-object `apply_paint_transform`; `box_geometry` is behind the `testing` feature; `NavigatorObserver` has `did_change_top` but no navigator; `ModalHandle::set_offstage` is `#[cfg(test)]`; `Scheduler::add_post_frame_callback` exists but is unreachable from a view; `ProxyAnimation`, `ReverseAnimation`, `CurvedAnimation`, `Interval`, `RectTween`, `ReverseTween`, `AnimatedBuilder`, `FadeTransition`, `IgnorePointer`, `Offstage`, `Stack`, `Positioned`, `MediaQuery` all exist; `Matrix4::transform_rect` exists; `RenderTheater` ignores positioned children; `ViewKey` carries `key_eq`/`key_hash`/`clone_key`.

**Derived, not yet proven:**
- **D-1.** Cross-overlay-entry `GlobalKey` migration works, because overlay entries are ordinary siblings in one `ElementTree` (Q1). Not needed by Hero; test named if it ever is.
- **D-2.** The shuttle's `ViewState` will not be shared with the original child, so a stateful hero child resets inside the flight (Q2). Flutter behaves the same way; worth an explicit test at U4.
- **D-3.** `Positioned` inside an overlay entry is handled by an inner `Stack` (S8). **A paper argument.** Must be checked before U3.

**Unknown — each must be answered by an experiment, not an assumption:**
- ~~**UNKNOWN-1.**~~ **Answered `NO` (§7b).** `pump_frame` never drains post-frame callbacks at all; the runner drains them *before* build/layout/paint. Nothing registers the pipeline as a persistent frame callback, where Flutter registers exactly that (`rendering/binding.dart:61`, `:557-558`). **Blocks U2** until the proposed **U1.5** binding seam lands.
- **UNKNOWN-2.** Does `ModalRoute` swapping its primary animation to `ALWAYS_COMPLETE` while offstage disturb `TransitionRoute`'s status listener, which writes `overlayEntries.first.opaque` and raises `finalize()` on `dismissed` (ADR-0020 §7d)? The proxy must sit **above** the controller, not replace it. *Blocks U3.*
- **UNKNOWN-3.** Flutter's `flightShuttleBuilder` receives both heroes' `BuildContext`s (`heroes.dart:1081-1087`). FLUI cannot hand out a foreign element's `&dyn BuildContext` outside its own build. What replaces it — the two `HeroHandle`s? the two tags plus their sizes? — decides the public signature at U4/U6. *Blocks the Q8 sign-off.*
- **UNKNOWN-4.** Whether `RenderStack`'s positioned pass composes correctly under `RenderTheater`'s `tight` constraints for a `RelativeRect`-style four-edge `Positioned`. Cheap to check; do it in U3 alongside D-3.

---

## 7a. Implementation findings (U1, 2026-07-10)

**Status: landed.** `RenderObject::apply_paint_transform`,
`PipelineOwner::{transform_to, local_to_global, global_to_local, box_size}`. No
Hero, no widget changes.

### Correction to §3 S4: the overrides are not the four this ADR named

S4 said the overrides were required "at minimum on `RenderTransform`,
`RenderRotatedBox`, `RenderFittedBox`, `RenderFractionalTranslation`". Reading the
code shows **three of those need none, and a fifth object does.**

FLUI's paint pipeline already composes two things per level
(`pipeline/owner/paint.rs:291-367`): it wraps children in the layer of
`RenderObject::paint_transform(size)` — **a hook this repository already had** —
and then paints each child at its committed offset. Child-local → parent-local is
therefore `paint_transform · translate(child_offset)`, and that is what
`apply_paint_transform`'s default body does. `RenderTransform`,
`RenderRotatedBox` and `RenderFittedBox` all report a `paint_transform`, so the
default already carries them; adding overrides would have duplicated the truth and
invited drift.

An override is needed exactly where **paint deviates from that default path**:

| Object | Why the default is wrong |
|---|---|
| `RenderFractionalTranslation` | Lays the child out at `Offset::ZERO` and shifts it at paint time with `PaintCx::paint_child_at` — an `offset_override`. The committed offset says nothing. |
| `RenderFlow` | Paints each child under a per-child transform scope the delegate chooses. **This ADR never mentioned `RenderFlow`.** Flutter overrides `applyPaintTransform` for it too (`flow.dart:456-462`). |

`RenderFlow` was the near-miss: shipping `transform_to` without it would have
produced a *silently wrong* rect under a flow — precisely the failure mode §3 S4
raises against a naive offset-only accumulation. FLUI caches no per-child flow
matrix (by design, see `flow.rs` module docs), so the override replays
`paint_children`, exactly as `hit_test` already does.

The red-checks: deleting either override turns `harness_transform_to_respects_*`
red; deleting `RenderRotatedBox::paint_transform` turns the rotated-box test red,
which is what proves the default consults it.

### Correction to §3 S4 / U1 item 5: `local_to_global` cannot live on the trait

U1's instruction allowed for this, and it is the case. `RenderBox::local_to_global`
and `global_to_local` were `&self` identity stubs. They could not have been
anything else: a FLUI render object has **no parent link and no owner** — the tree
lives in `RenderTree`, geometry in `RenderState`. Flutter's live on `RenderBox`
only because a Dart render object holds `parent` and `owner` (`box.dart:3113`,
implemented via `getTransformTo`).

Both stubs are **deleted**. The real methods are on `PipelineOwner`, which owns the
tree. Nothing called the stubs, so nothing broke; `flow.rs`'s module docs, which
recorded the gap, are updated.

For the same reason `apply_paint_transform` takes two parameters Flutter's does
not — the child's committed paint offset and this node's laid-out size. A Dart
render object reads both off itself; a FLUI one cannot.

### `transform_to` is narrow, and `None` means "cannot answer"

Strict descendant → ancestor only, as §5 U1 scoped it. `Some(IDENTITY)` for a node
and itself; `None` when `ancestor` is not an ancestor, **or when any node on the
path has not been laid out** (see the defect below). Because the walk never
inverts, it cannot fail on a singular matrix — only `global_to_local` can, and it
returns `None` where Flutter returns `Offset.zero` (`box.dart:3079-3081`) rather
than inventing an answer.

**A red-check found a redundant guard.** The first draft returned `None` from two
places: running out of parents, *and* a child-index lookup that could not find the
child. Removing the first left every test green — the second was silently covering
for it. A `None` from a corrupt parent/child link would have masqueraded as "not an
ancestor". The index lookup is now an `expect("BUG: …")` (the tree's own
invariant, per [`PANIC-POLICY`](../PANIC-POLICY.md)), so exactly one check answers
the question and deleting it goes red.

### A defect that shipped in the first U1 commit: `unwrap_or(Size::ZERO)`

`RenderNode::apply_paint_transform` resolved the parent's size with
`geometry().unwrap_or(Size::ZERO)`, copying the shape of the neighbouring
`RenderNode::paint_transform` (where a pre-layout node is never reached, because
paint runs after layout). `transform_to` has no such guarantee, and the
substitution made it **plausibly wrong before the first layout**: a `FittedBox`, a
rotation about its centre, a `RenderFractionalTranslation` and a `RenderFlow` are
all size-dependent, so `Size::ZERO` relocates the pivot and returns a *well-formed
matrix for a layout that never happened*. Meanwhile the doc promised `None` only
for a malformed ancestor relation.

Flutter does not substitute: it asserts `hasSize` at the call sites
(`box.dart:3016`, `heroes.dart:380`).

**Fixed** by threading the absence through: `RenderNode::laid_out_size()` returns
`Option<Size>` (for slivers, `absolute_paint_size` needs *both* geometry and
constraints, and silently zeroes when either is missing — the presence check moved
out of it), `RenderNode::apply_paint_transform` returns `Option<()>`, and
`transform_to` propagates. A public API returning `None` beats a `BUG:` panic here:
"has this subtree been laid out yet" is a legitimate question a caller may not know
the answer to, not a framework invariant.

So `transform_to` now returns `None` for **two** reasons, both meaning *the
question cannot be answered*: not an ancestor, or not laid out. `Some(IDENTITY)`
for a node and itself stays — no step runs, so no size is needed.

Pinned by `transform_to_before_layout_returns_none`, whose fixture scales about its
own centre. Red-check: restoring `unwrap_or(Size::ZERO)` turns it red, as does
swallowing the `Option` in `transform_to`. A third red-check corrected the test's
own doc: a *size-independent* parent would also have caught the `None`, because the
old code returned `Some` either way — the size-dependent fixture earns its place by
letting the test show *what* the old code returned, not by being necessary for
detection.

### One footgun, documented rather than fixed

`Matrix4::translate` **pre**-multiplies (`*self = translation * *self`), where
Flutter's post-multiplies. So does `scale`, and `rotate_z`. Composition here is
written explicitly as `*transform = *transform * step`. Changing those methods is
out of U1's scope — they have other callers — but any future
`apply_paint_transform` override that reaches for `Matrix4::translate` will be
silently wrong, and the trait's doc comment says so.

### Not done, not claimed

`getTransformTo` for two nodes sharing a *common ancestor* (Flutter's general
form, which needs an inverse). Perspective transforms: `global_to_local` takes a
plain inverse, exact for the affine 2-D matrices every render object here
produces, but **not** Flutter's un-projection onto the local z = 0 plane
(`box.dart:3062-3095`). No perspective transform exists in this repository; one
arriving must revisit that method. Sliver render objects get the default
`apply_paint_transform` with no override point, since `RenderSliver` does not
forward the hook.

---

## 7b. UNKNOWN-1 answered: **no**, on both paths. U2 is blocked (2026-07-10)

**Status: U2 not started.** Per its own first rule, the frame-order question was
closed experimentally before any seam was designed. The answer is negative, so
nothing else in U2 was built. No `PostFrameHandle`, no observer attachment, no
route introspection. This section records the actual frame order and proposes the
minimal seam that would unblock U2.

### What Flutter does

`RendererBinding` registers the render pipeline as a **persistent frame callback**
(`rendering/binding.dart:61`, `:557-558` — `_handlePersistentFrameCallback` calls
`drawFrame()`). `SchedulerBinding.handleDrawFrame` then runs, in one frame and in
this order (`scheduler/binding.dart:1338-1358`):

1. `SchedulerPhase.persistentCallbacks` → **the whole pipeline: build, layout, paint.**
2. `SchedulerPhase.postFrameCallbacks` → the post-frame queue, drained once.

So a post-frame callback registered during frame *N* runs at the end of frame *N*,
after layout has committed. That is precisely what `heroes.dart:968` depends on:
`toRoute.offstage = …; WidgetsBinding.instance.addPostFrameCallback(…)` measures
the destination hero *in the frame it just forced offstage*.

### What FLUI does

**Nothing registers the pipeline as a persistent callback.** Grep for
`add_persistent_callback` across `flui-app`, `flui-binding` and `flui-view`
returns nothing. The pipeline is driven by the *bindings*, beside the scheduler
rather than inside it. The two paths then diverge from Flutter in two different
ways:

| Path | Order | Consequence |
|---|---|---|
| `AppBinding` / runner (`app/runner.rs:279+283`, `:559+562`, `:755+764` — all three sites) | `handle_begin_frame` → **`handle_draw_frame()`** → `binding.render_frame()` | The post-frame queue is drained (`scheduler.rs:685-706`) **before** build/layout/paint. A callback sees the *previous* frame's geometry. **One frame stale.** |
| `HeadlessBinding::pump_frame` (`flui-binding/src/lib.rs:424-490`) | clock → gestures → `vsync.tick_all` → `drive_async_tasks` → `build_scope` → `run_frame_with_layout_builders` → `service_child_requests` | It never calls `handle_draw_frame` or `handle_begin_frame` at all. The post-frame queue is **never drained**, and no scheduler frame is ever opened, so even calling the drain would be a no-op — `handle_draw_frame` guards on `current_frame` being `Some` (`scheduler.rs:687`). |

### The experiment

Reading three call sites is not an experiment, so a throwaway probe was run against
the real `HeadlessBinding` frame path: register one post-frame callback, pump two
frames, then call `Scheduler::execute_frame()` directly.

```text
PROBE after 1 pump_frame:  fired=0
PROBE after 2 pump_frame:  fired=0
PROBE after execute_frame: fired=1
```

The callback is well-formed and the queue works — `pump_frame` simply does not
drive it. (The probe was deleted; it is reproduced here as evidence, not shipped.)

### Consequence for the design

`post_frame_callback_runs_after_layout_in_the_same_pumped_frame` **cannot be made
to pass** without a binding change. Per U2's own instruction, no `PostFrameHandle`
was written, no seam was designed around the assumption, and nothing was papered
over with a second `pump_frame`. **U2 is blocked on a scheduler/binding fix that
this ADR did not anticipate**, and which is not a widgets change at all.

`§3 S6`'s claim — *"`Scheduler::add_post_frame_callback` exists … `BuildContext`
exposes `rebuild_handle()` and `async_driver()` and nothing else"* — was right
about the missing capability and wrong to imply the callback machinery underneath
it works. It does not.

### Proposed minimal common seam (**U1.5**, needs sign-off)

Flutter's arrangement — the pipeline *is* a persistent callback — cannot be copied
directly: `flui-scheduler` sits below `flui-view` and cannot name a tree, and
`HeadlessBinding` owns its `ElementTree` by value, so no `Fn` closure can drive it.
The minimal change that gives both paths Flutter's *observable* ordering:

1. Split `Scheduler::handle_draw_frame` (`scheduler.rs:649`). It keeps the
   persistent-callback and task-queue phases; its tail — take the frame timing,
   record jank, drain the post-frame queue, notify completion, return to `Idle`
   (`scheduler.rs:685-725`) — moves into a new `Scheduler::end_frame()`.
2. Every frame driver becomes: `handle_begin_frame` → `handle_draw_frame`
   (persistent) → **its own pipeline step** → `end_frame()` (post-frame).
3. `HeadlessBinding::pump_frame` opens and closes a scheduler frame, which today
   it never does. This is what makes the headless path drive the *same* seam as
   production rather than a parallel one.
4. `Scheduler::execute_frame()` becomes `begin + draw + end` so its current
   post-frame behavior is preserved for existing callers.

Divergence to state plainly if this lands: a FLUI *persistent* callback runs
**before** the pipeline, where Flutter's pipeline is itself the first persistent
callback. Post-frame callbacks are unaffected — they see committed geometry in
both — and post-frame is the only phase Hero needs. Nothing else in the framework
registers a persistent callback today, so nothing observes the difference.

**Cost:** touches `flui-scheduler`, `flui-binding`, `flui-app` (three runner call
sites), and the `handle_draw_frame` tests. It is the kind of cross-crate change §5
U1 deliberately isolated for the geometry seam, and it deserves the same treatment:
**its own slice, before U2, with its own gate.**

### Honest limits of this finding

- It is **not** established that moving the drain is *sufficient* for Hero — only
  that the current order makes the U2 seam impossible to build correctly. Whether
  a route forced offstage in frame *N* has committed geometry by the post-frame
  phase of frame *N* is a further claim, testable only once the drain moves.
- The three runner call sites are the *desktop*, *web*, and one further path; each
  was read, none was executed. A `production_and_headless_paths_drive_the_same_post_frame_seam`
  test does not exist and cannot exist until they share a step.
- `Scheduler::handle_draw_frame`'s post-frame drain is not dead code — it fires for
  callbacks registered outside the tree path. The bug is its *position*, not its
  existence.

---

## 7c. U1.5 landed: the frame contract (2026-07-10)

**Status: landed.** `Scheduler::{drive_frame, end_frame, abort_frame}`; the
async-driver poll moved into `handle_begin_frame`. No Hero, no Navigator, no
widget seams. **U2 is unblocked.**

### The contract, now enforced in one place

```text
Scheduler::drive_frame(vsync_time, pipeline):
    handle_begin_frame  → transient callbacks
                        → MidFrameMicrotasks: flush microtasks
                        → exactly ONE poll of *this* Scheduler's AsyncDriver
    handle_draw_frame   → PersistentCallbacks: persistent callbacks + task queue
    pipeline()          → build + layout + compositing + paint   (persistent slot)
    end_frame           → PostFrameCallbacks: drain, timing, jank, notify
                        → Idle
```

Every driver goes through `drive_frame`: `HeadlessBinding::pump_frame` on its
binding-local scheduler, and the desktop / android / wasm runners on the
`Scheduler::instance()` singleton. There is no second sequence to maintain.

The pipeline **semantically occupies the persistent slot without being registered
as an `Fn` callback**. That is what makes this possible at all: a `HeadlessBinding`
owns its `ElementTree` by value, so no closure could ever have driven it the way
Flutter's `drawFrame` does. §7b's Option C was rejected for exactly that reason.

### Why the naive split (§7b's proposal) would have panicked on frame 1

§7b proposed splitting `handle_draw_frame` and running the pipeline while the phase
was `PersistentCallbacks`. An adversarial review of that plan, before any code, found
it fatal: `AppBinding::draw_frame` called `Scheduler::instance().drive_async_tasks()`,
which opens with

```rust
debug_assert_ne!(self.phase(), SchedulerPhase::PersistentCallbacks,
    "BUG: the async driver must not poll during build/layout/paint");
```

The pipeline is not the persistent slot in FLUI — it *contains* FLUI's transient
(`vsync.tick_all`) and mid-frame (`drive_async_tasks`) work. The desktop runner would
have panicked on its first debug frame, and violated ADR-0018's slot contract in
release.

**Resolution: the driver poll moved into the scheduler** (`handle_begin_frame`'s
mid-frame slot), and the bindings stopped calling it. ADR-0018's stated contract —
*"the driver step is owned by the binding"* — was a mis-statement of its real
invariant (*one mid-frame poll per frame, on the right instance*), and is corrected
there. Holding the poll in the scheduler enforces both halves structurally.

`RC5` reproduces the panic: putting `drive_async_tasks()` back into `draw_frame` makes
`production_post_frame_callback_observes_this_frames_committed_layout` fail with that
exact `BUG:` message.

### Error and panic semantics, explicit

* A pipeline that **returns an error value** is a completed frame. `end_frame` runs,
  post-frame callbacks fire, exactly once, phase → `Idle`.
  (`a_pipeline_returning_an_error_still_completes_the_frame`)
* A pipeline that **panics** is an abandoned frame. `drive_frame` catches the panic,
  calls **`abort_frame()`** — phase → `Idle`, **no** post-frame callbacks — and then
  resumes the unwind unchanged. The queued callbacks survive to the next completed
  frame. This mirrors Flutter, where a throwing persistent callback skips the
  post-frame loop and `finally { _schedulerPhase = idle; }` still resets
  (`scheduler/binding.dart:1341-1374`).
  (`a_panicking_pipeline_aborts_the_frame_and_runs_no_post_frame_callbacks`,
  `a_frame_after_a_panicking_frame_starts_cleanly`)

  **Review caught a regression here.** The first cut left the frame open and expected
  the *caller* to call `abort_frame`. No caller did. Before U1.5 the phase was already
  `Idle` when the pipeline ran, so a panicking `render_frame` was harmless; after the
  split it would leave the frame at `PersistentCallbacks` and the **next**
  `handle_begin_frame` would attempt the illegal
  `PersistentCallbacks -> TransientCallbacks` transition. `drive_frame` now recovers.

**No `Drop` guard.** Two reasons, both load-bearing. A guard would have to force
`PersistentCallbacks -> Idle`, which `can_transition_to` forbids, so
`set_scheduler_phase`'s `debug_assert!` would fire *while already panicking* — a
double panic, i.e. `abort`. And running any user callback during unwind is a hazard
for no benefit. The recovery instead runs **between** `catch_unwind` and
`resume_unwind`: the payload is already captured, so nothing executes during
unwinding. `abort_frame` bypasses the phase validator by design — the one sanctioned
exit from a half-open frame — and notifies completion waiters, so `end_of_frame()`
cannot hang on an aborted frame. Under `panic = "abort"` nothing is caught, which is
moot.

`Idle -> PostFrameCallbacks` was **not** added to the phase machine.

### The divergence that remains, pinned not claimed

In Flutter the pipeline **is** the first persistent callback
(`rendering/binding.dart:61`, `:557-558`), so a persistent callback registered later
runs *after* it. In FLUI the pipeline is a closure, so every registered persistent
callback runs *before* it. Nothing in the framework registers one — only
`flui-scheduler`'s own tests do — so nothing observes the difference today.
`persistent_callbacks_run_before_the_pipeline_a_divergence_from_flutter` asserts it,
so it cannot be forgotten.

**Post-frame parity is claimed. Persistent-phase parity is not.**

### What is proven, and by what

| Claim | Evidence |
|---|---|
| Headless post-frame sees this frame's committed layout | `flui-binding`: `post_frame_callback_runs_after_layout_in_the_same_pumped_frame` — the callback reads `PipelineOwner::box_size` and is never invoked by the test |
| Production post-frame sees this frame's committed layout | `flui-app`: `production_post_frame_callback_observes_this_frames_committed_layout` — drives the real `AppBinding::draw_frame` through `drive_frame` |
| Headless and production use different `Scheduler` instances, on purpose | `pump_frame_drives_the_binding_local_scheduler_not_the_singleton`; `each_scheduler_instance_polls_only_its_own_async_driver` |
| No runner site hand-rolls the sequence | `flui-app`: `every_runner_frame_site_uses_the_shared_drive_frame_helper` (source scan) |
| Callback runs exactly once; a callback registered from one defers | `post_frame_callback_runs_exactly_once_across_two_frames`; `a_post_frame_callback_registered_from_a_post_frame_callback_defers_to_the_next_frame` |

### Honest limits

* The **android** runner site was **not compiled** — it needs the NDK. The wasm site
  type-checks (`cargo check -p flui-app --target wasm32-unknown-unknown`); the desktop
  site compiles and is exercised. The source-scan guard proves only that no site
  hand-rolls begin/draw/end; it is a regression guard, not a proof of the android
  body's runtime behavior.
* The wasm site's early `return` (renderer not yet ready) used to run *after*
  `handle_draw_frame` and would now have leaked a half-open frame. Inside the closure
  it merely exits the closure and `end_frame` still runs. That is a fix, and it is
  only type-checked, not executed.
* **A first version of the "callback has not run during layout" test was a
  tautology**, and review caught it. It sampled the flag from a *persistent*
  callback, which precedes the pipeline in **both** the fixed and the broken
  ordering — so it passed under the injected bug. It now samples from inside
  `perform_layout`, the only vantage point that can tell the two apart, and it
  red-checks.
* **`EmbedderScheduler` (`flui-app/src/embedder/embedder_scheduler.rs`) leaks a
  scheduler phase**: its `begin_frame()` opens a frame and its `end_frame()` calls
  `Scheduler::end_of_frame()`, which only registers a completion future and never
  closes the phase, despite a doc comment claiming it "executes post-frame
  callbacks". **Pre-existing and untouched by U1.5** — the glue never called
  `handle_draw_frame` either — and it is dead code with no caller. Its `end_frame`
  now name-collides with `Scheduler::end_frame`, which is a trap for whoever wires
  the embedder. Not fixed here; out of U1.5's scope.
* **U2 is unblocked but not proven.** That a route forced offstage in frame *N* has
  committed geometry by that frame's post-frame phase is a further claim, and belongs
  to U2's `RouteSubtree`/`PostFrameHandle` tests. U1.5 proves only that *some*
  committed geometry is visible to a post-frame callback in both paths.
* `HeadlessBinding` and production drive **different `Scheduler` instances**. Any U2
  `PostFrameHandle` must resolve to the *binding's* scheduler, not a global, or the
  two paths silently wire different queues. This is now a tested property, not folklore.

---

## 7d. U2 acceptance gate passed; §3 S3 is wrong (2026-07-10)

### The gate: **yes**, with a caveat

U2's first rule was to prove, executably, that a route forced offstage in frame *N*
has committed geometry visible from that frame's post-frame callback. It does.
Three tests in `flui-widgets/src/navigator/offstage_measurement_tests.rs`, all
driven through the real `HeadlessBinding::pump_frame` — no callback is ever invoked
by hand:

| Claim | Test | Red-check |
|---|---|---|
| A **newly pushed** route, forced offstage between frames, is built, laid out, and its geometry committed before the post-frame callback | `a_route_forced_offstage_has_committed_geometry_in_the_same_frames_post_frame_callback` | reorder `drive_frame` to `end_frame()` before `pipeline()` → red |
| That geometry is **real**, not a zero-sized placeholder | `the_offstage_routes_committed_geometry_is_real_not_zero` | same |
| Setting `offstage` on an **already-mounted** route rebuilds it before the callback | `setting_offstage_on_a_mounted_route_rebuilds_it_before_the_post_frame_callback` | delete `mark_entry_needs_build()` from `changed_internal_state` → red |

The route under test is *newly pushed* on purpose: a route laid out by an earlier
frame has committed geometry whatever this frame's ordering, so such a test would
pass for the wrong reason.

**The third test exists because a red-check exposed a hole.** Deleting
`mark_entry_needs_build()` left the first test green — the `push` alone was
building the route, so that test said nothing about the offstage *dirty*. The
`HeroController` pop path has no push to lean on.

**Caveat, and it is not small.** This proves the geometry is *committed*. It does
**not** prove it is the *final* hero position. Flutter's `offstage` setter also
points the route's animation proxy at `kAlwaysCompleteAnimation`
(`routes.dart:1958-1962`), which is what puts the heroes where they will land.
ADR-0020 §7d deferred that swap and **D4** assigns it to U3. Until D4 lands, an
offstage route is laid out at `animation == 0` — the *entry* position. U2's seams
are unblocked; Hero's measurement is not correct yet, and U3 owes it.

### Correction to §3 S3: `ModalScope::init_state` cannot publish a `RenderId`

S3 said: *"`ModalScope` … publishes its own `ElementId` and `RenderId` at
`init_state` — the same owned-capability move as `RebuildHandle`."* Both halves of
the `RenderId` claim are wrong:

1. **`ModalScope` is a stateful view. It owns no render object at all.** Only a
   `RenderView`'s element carries a `RenderId`
   (`flui-view/src/element/behavior.rs`: `RenderBehavior` is the sole override of
   `ElementBase::render_id`).
2. **`BuildContext::find_render_object()` walks strict *ancestors*, not
   descendants** (`context/element_build_context.rs:459-471`, `:824-825`). It is
   Flutter's `findAncestorRenderObjectOfType`, **not** `context.findRenderObject()`.
   Calling it from `ModalScope` returns the enclosing `RenderTheater`/`RenderStack`
   — the overlay's coordinate space, not the route's.

Flutter has no such problem: `_subtreeKey` is a `GlobalKey` on a **`RepaintBoundary`**
(`routes.dart:1229`), a render-object widget, so `subtreeContext.findRenderObject()`
returns that boundary's own render object. The route's subtree root is a render
object *by construction*.

**The first lifecycle hook where a `RenderId` is guaranteed** is
`RenderObject::attach(RepaintHandle)` — `RepaintHandle::id()` returns it
(`flui-rendering/src/pipeline/handle.rs:216`, `:231`), and `detach()` is its exact
mirror. That is mount/unmount-driven, needs no `GlobalKey`, no element walk, and no
acquisition during build/layout/paint (port-check trigger #22).

So `RouteSubtree` publication requires a **render object** at the root of the
route's page subtree, which publishes its own id on `attach` and clears it on
`detach`. No fake id, no zero placeholder. **Where that render object lives is an
open decision** (see the open questions), because `flui-widgets` defines none today
and `flui-objects`' harness catalog guard requires every exported `RenderBox` to
carry `harness_*` tests.

### Landed in this pass

* `Scheduler::abort_frame`'s doc comment corrected: `drive_frame` **does** catch a
  panicking pipeline (`catch_unwind` → `abort_frame` → `resume_unwind`); the reset
  runs between catch and resume, not during unwinding. Text only; no behavior change,
  and the scheduler suite is unchanged.
* The three acceptance tests above, plus the test-only accessors they need
  (`Harness::{scheduler, pipeline_owner}`, `PageRoute::modal_handle`).

**No U2 seam is implemented.** `PostFrameHandle`, observer attachment, route
introspection, `RouteSubtree` and the overlay seam are all unbuilt.

---

## 7e. U2 seams 2–5 landed (2026-07-10)

Four seams, each pinned to the `heroes.dart` line that will consume it. No `Hero`
widget, no `HeroController`, no flight, no public Hero API.

| Seam | Shape | Flutter |
|---|---|---|
| 2. Observer attachment | `NavigatorObserver::did_attach(NavigatorHandle)` / `did_detach()`, driven from `NavigatorState`'s `init_state` / `activate` / `deactivate` / `dispose` | `NavigatorObserver.navigator`, an `Expando` written at `navigator.dart:3836`, `:4060`, `:4108`, `:4121` |
| 3. Route introspection | `NavigatorHandle::{route_peer, route_subtree, is_current}`, `pub(crate)` | `route.animation`, `route is PageRoute`, `route.subtreeContext`, `route.isCurrent` |
| 4. Route subtree | `RenderSubtreeAnchor` around `buildPage`'s output; `RouteSubtree { element_id, render_id }` published into a navigator-owned registry | `ModalRoute._subtreeKey` on the `RepaintBoundary` at `routes.dart:1229-1231` |
| 5. Overlay access | `NavigatorHandle::overlay()`, `pub(crate)`; `Overlay`/`OverlayEntry` stay unexported | `navigator.overlay` (`heroes.dart:990`) |

### Two facts this pass established, both load-bearing

**1. Observers are notified while the history mutex is held.** `RouteHistory::flush`
calls `did_push` / `did_pop` / `did_remove` / `did_replace` / `did_change_top` from
inside `NavigatorShared::mutate`, which holds `history.lock()` for the whole walk.
`parking_lot::Mutex` is not reentrant, so an observer that reads or mutates the
*stack* through its `NavigatorHandle` from one of those callbacks **hangs**. Every
other capability on the handle — `overlay()`, `route_subtree()`, `route_peer()`,
`post_frame_handle` — is safe there, and that is exactly the set
`HeroController.didPush` uses (`heroes.dart:964-973`): flip a route offstage, then
schedule a post-frame callback and measure from it, outside the flush. Documented on
the trait. `attach_observers` / `detach_observers` snapshot the observer list and
notify with **no** lock held, so `did_attach` may use the handle freely.

Deferring notification out of the lock entirely (so the constraint disappears) would
mean moving the observer list and the route disposal onto `NavigatorShared` and
returning both in `FlushOutcome`. That is a strictly better shape — it would also
move `route.dispose()` out from under the lock — and it is **not** done here.
Recorded as a follow-up, not as a limitation of U2's seams.

**2. `subtreeContext` and its render object are two nodes in FLUI, not one.**
Flutter's `_subtreeKey` sits on a `RepaintBoundary`, so `subtreeContext` and
`subtreeContext.findRenderObject()` name the same node. FLUI's
`BuildContext::find_render_object()` walks strict **ancestors** (it is
`findAncestorRenderObjectOfType`), so a context can never yield the `RenderId` below
it. `RouteSubtree` therefore carries two ids from two hooks: `element_id` from
`ViewState::init_state`, `render_id` from `RenderBox::attach`. They bracket exactly
the page subtree, so nothing observable depends on the offset — recorded, not claimed
away. `RenderSubtreeAnchor` is also *not* a repaint boundary: identity without the
compositing side effect.

### The two-stage resolution rule

`RouteSubtree` resolves from `attach`, which runs during **build**. Layout has not
run then. `SubtreeAnchor::get()` is therefore **not** layout-readiness; geometry
comes from `PipelineOwner::box_size`, which is `None` until the first layout commits
(§7c / U1). `route_subtree_ids_are_published_before_layout_commits` reads the seam
from inside the page's own `build` and asserts exactly that: ids `Some`, size `None`;
the post-frame callback of the same frame then sees committed geometry.

### Open question 2 (§7d) is answered

`RenderSubtreeAnchor` lives in **`flui-objects`**, public and in the harness catalog
(`RENDER_OBJECT_TYPES`, `harness_subtree_anchor_*`). It is a render-layer fact, not a
widget-layer trick, and a private exception in `flui-widgets` would have put a render
object outside the catalog guard.

### Still not done, and not claimed

* No `HeroControllerScope`, so **nested navigators are out of scope**: these seams
  answer only about the navigator that owns them. B1.4 stays open.
* `ModalHandle` (the `offstage` setter) is still `#[cfg(test)]`, and `offstage` still
  does not swap the animation proxy to `kAlwaysComplete` (`routes.dart:1958-1962`).
  D4 assigns both to U3; until then a `HeroController` could flip a route offstage
  but would read its *in-flight* animation value, not `1.0`.
* `route_peer` / `route_subtree` / `is_current` / `overlay` have no production caller
  and carry `#[allow(dead_code)]` naming U3 as the first one.

---

## 7f. Observers are notified outside the history mutex (2026-07-10)

§7e shipped seam 2 with a documented restriction: an observer's `did_push` must not
read the stack through the `NavigatorHandle` it was handed, because
`RouteHistory::flush` ran the callbacks while `NavigatorShared::mutate` held
`history.lock()`, and `parking_lot::Mutex` is not reentrant. That was the wrong call.
`NavigatorObserver` is public, the handle is the *point* of seam 2, and
`HeroController` is the first consumer — a doc-only restriction on a hang is not a
contract anyone can build on.

### The shape

`RouteHistory` now **decides** and the navigator **performs**. The flush walks the
stack and returns owned data; `NavigatorShared::apply` acts on it with the mutex
released:

| | Under `history.lock()` | After it is released |
|---|---|---|
| walk, `handle_push`/`handle_pop`, entry removal | ✅ | |
| `_flushRouteAnnouncement` (`did_change_next`/`did_change_previous`) | ✅ | |
| observer notifications, `did_change_top` | | ✅ |
| `Route::dispose` | | ✅ |
| overlay entry removal, `rearrange` | | ✅ |

`FlushOutcome` grew `notifications: Vec<Notification>` (additions LIFO, then
deletions FIFO, then `TopChanged` — per pass) and `dying: Vec<RouteEntry>`, so it
owns the routes it killed. It is no longer `Clone`/`PartialEq`. `last_outcome`
**absorbs** rather than overwrites: an un-taken outcome owns dying routes, and
dropping it would silently skip a `Route::dispose`.

`RouteHistory` no longer holds observers at all — they moved to `NavigatorShared`,
and `route_stack_flush_is_pure_data` now forbids the pure files from even naming
`NavigatorObserver`. A stack that cannot call an observer cannot deadlock on one.

### What this buys, and what it costs

**Buys.** `current()`, `route_ids()`, `can_pop()` — and `push()` / `pop()` — are all
safe from any observer callback. Notifications are delivered against a *settled*
stack, never a half-walked one. `Route::dispose` (an animation controller, a vsync
unregistration, a route below releasing its secondary animation) no longer runs
under the mutex.

**Costs, both recorded.**

1. **`_flushRouteAnnouncement` now precedes the observer callbacks** rather than
   sitting between them and `did_change_top` (`navigator.dart:4584-4596`). It takes
   `&mut` on the entries, so it cannot leave the borrow. `did_change_next` /
   `did_change_previous` are route-internal — they drive secondary animations, which
   no observer surface exposes — so an observer sees a strictly more settled stack,
   never a different one.
2. **Mutating the stack from a callback is defined here, where Flutter asserts.**
   `_flushHistoryUpdates` guards with `_debugLocked` (`:4452`); FLUI's callback runs
   after the flush has settled and released the mutex, so a re-entrant `push` simply
   runs a fresh flush whose notifications are delivered after the outer drain.
   `an_observer_may_push_from_did_push_without_deadlocking` pins that it terminates.
   Defined, not encouraged.

### Evidence

A deadlock hangs rather than fails, so the two regression tests run their body on a
worker thread and assert on the clock (`must_finish`, 10 s). Restoring the U2 shape —
`apply(outcome)` inside the `history.lock()` scope — times both out. Nine other
mutations cover queue order (additions LIFO, deletions FIFO, additions-before-
deletions), `absorb`'s two `extend`s, `drain`'s consume-not-copy, notify-before-
overlay-teardown, and the purity edge.

One test was found to pin the harness rather than the code:
`flush_disposes_removed_routes_after_notifications` drives the test-side `settle`,
so reordering production's `apply` left it green.
`observers_are_notified_before_a_dying_routes_overlay_entry_is_torn_down` now pins
`apply` itself, through the real navigator.

---

## 7g. U3: the offstage animation proxies and the measurement skeleton (2026-07-10)

### Task A — `ModalRoute.offstage` swaps the animations, not just the visibility

ADR-0020 §7d deferred this as tidy-up. It is not. `RenderOffstage` keeps a route laid
out while hiding it — but a route half-way through its entrance transition *lays out*
half-way through its entrance transition, offstage or not. Flutter's setter therefore
does two more things (`routes.dart:1958-1961`):

```dart
_animationProxy!.parent = _offstage ? kAlwaysCompleteAnimation : super.animation;
_secondaryAnimationProxy!.parent = _offstage ? kAlwaysDismissedAnimation : super.secondaryAnimation;
```

`ModalRoute` now owns two `ProxyAnimation`s, seeded in `install()` exactly as Flutter
seeds them (`:1685-1686`), and **they — not the controller — are what `buildPage` and
`buildTransitions` receive**. Without them every hero flight would have started from
the wrong rect, and no test in the tree would have noticed.

**A correction to what this pass first claimed.** `mark_entry_needs_build` was
documented as "the only thing that rebuilds the scope with the swapped animations".
That is false, and a red-check proved it: deleting the call left every offstage test
green. `ProxyAnimation::set_parent` *notifies* its listeners, the `ModalScope`'s relay
is one of them, and the scope rebuilds itself. What `mark_entry_needs_build` actually
rebuilds is the overlay **entry** — `Stack[barrier, Offstage[scope]]` — so the flipped
flag reaches the `Offstage` wrapper and the barrier. Two mechanisms, two tests:
delete the swap and the route paints correctly while measuring wrong; delete
`mark_entry_needs_build` and it measures correctly while still painting.

### Task B — `HeroController`, and the hook a port gets wrong

Flutter's `HeroController` overrides **`didChangeTop`** (`heroes.dart:854`) — never
`didPush`/`didPop`. This pass started with `didPush`/`didPop` and was wrong: those
fire for routes that never become the top one, and do not fire when a route becomes
top by having its cover popped. `assert(topRoute.isCurrent)` (`:855`) says as much.
The flight direction likewise comes from the two routes' animation **statuses**
(`:924-932`), not from which navigator call happened.

The controller composes every seam this ADR built, and adds nothing of its own:

| Step | Seam | Landed |
|---|---|---|
| `didChangeTop` outside the history lock | `Notification::TopChanged` | §7f |
| `toRoute.offstage = …` (`:967`) | `ModalHandle` via the navigator's modal registry | U3 |
| offstage ⇒ `animation.value == 1.0` | the `ModalRoute` proxies | U3 |
| `addPostFrameCallback` (`:968`) | `PostFrameHandle` | U2 |
| callback runs after layout commits | `Scheduler::drive_frame` | U1.5 |
| `to.subtreeContext` (`:1014`) | `RouteSubtree` | U2 |
| `…findRenderObject()!.size` (`:952`) | `PipelineOwner::box_size` | U1 |
| `getTransformTo(…)` (`:1029`) | `PipelineOwner::transform_to` | U1 |

### The one capability that was missing: `BuildContext::pipeline_owner()`

`find_render_object()` hands out a `RenderId`, and a `RenderId` alone answers nothing —
geometry lives in the `PipelineOwner`. Flutter has no equivalent because a Dart
`RenderObject` *is* the handle. Nothing in FLUI exposed the owner to a
`BuildContext`, so `HeroController` could not resolve what `RouteSubtree` handed it.

Rather than reach around it, this pass added the capability and says so. It is **not**
a frame capability: it schedules nothing, so trigger #22 does not guard it, and a
read during `build` simply answers `None` for every un-laid-out node (U1). Its purpose
is the opposite direction — code *outside* the tree holding an owned handle so it can
measure from a post-frame callback. `NavigatorState::init_state` captures it, and the
`PostFrameHandle`, into `NavigatorShared`; `dispose` clears both, which is what makes
a stale `HeroController` inert.

It is cloned from the element's own `ElementCore`, not looked up in the tree:
`build_scope` has the element *extracted* from its node, and `ElementNode::element`
panics in that window.

### Acquire the capability, then mutate the route

Review caught the ordering. `maybe_start` flipped the destination offstage and *then*
reached for `post_frame_handle()`, returning on `None`. Flutter can write it that way
because `addPostFrameCallback` cannot fail (`heroes.dart:967-968`); FLUI's capability
is an `Option` — absent on an unmounted navigator, and absent under any binding that
never calls `install_build_capabilities`. The only code that calls `set_offstage(false)`
is the measurement that failure would have scheduled, so the destination was stranded
offstage forever: invisible, unhittable, animation pinned at `1.0`.

The guard now precedes the mutation. `without_a_post_frame_capability_the_destination_is_left_onstage`
mounts a navigator through a binding that installs no post-frame handle — a real,
reachable configuration, not a mock — and pins that a fully eligible `PageRoute` →
`PageRoute` top change schedules nothing and touches nothing.

### Still not implemented, and not claimed

* **No `Hero` widget, no public API.** `HeroController`, `ModalHandle`,
  `FlightDirection`, `Measurement` and `RouteSubtree` are `pub(crate)`, and
  `public_no_internal_route_stack_exports` now fails if any is exported.
* **No flight.** No `_allHeroesFor`, no `_HeroFlight`, no overlay entry, no
  `RectTween`, no `flightShuttleBuilder`. `Measurement` is *recorded*, never used.
* **No `userGestureInProgress`.** FLUI has no back-swipe, so `didStartUserGesture` /
  `didStopUserGesture` (`heroes.dart:871-889`) and the `hasValidSize` fast path
  (`:952-960`) — which only ever runs for a gesture-driven pop — are absent, not done.
* **No nested navigators.** No `HeroControllerScope`; a controller answers only about
  the navigator that attached it. B1.4 stays open.
* The four introspection methods and `ModalHandle` carry `#[allow(dead_code)]` naming
  U4's `Hero` widget as the first production consumer.

---

## 8. Deferred, each with its blocker

| Deferred | Why |
|---|---|
| **Nested navigators** (`heroes.dart:322-332`, `HeroControllerScope`) | FLUI has `maybe_of_root` but no nested-navigator semantics, and the registry design (S7) scopes heroes to the nearest route. A **narrowing**, stated in §3 S7, not a silent gap. |
| **User-gesture flights** — `transitionOnUserGestures`, `didStartUserGesture`, `didStopUserGesture`, `userGestureInProgressNotifier`, `didStartUserGesture` (`:872`) / `didStopUserGesture` (`:882`), and the delayed `_performAnimationUpdate` (`:620-650`) | FLUI has no back-swipe / predictive back (ADR-0020 §7e defers it). Every gesture path in `HeroController` is unreachable without it. |
| **`TickerMode`** (`heroes.dart:433`) | Does not exist in FLUI (`visibility.rs:29` already records the gap). An offstage hero's animations keep running. Cost, not correctness. |
| **`HeroMode`** (`heroes.dart:1129`) | Needs the registry to honour an inherited disable (S7). Cheap once the registry exists; not U1–U5. |
| **Hero semantics** | Flutter's shuttle and placeholders have no special semantics handling worth porting yet; FLUI's `RenderTheater` does not skip semantics for offstage children at all (ADR-0020 §7d). Fixing that is `RenderTheater`'s problem, not Hero's. |
| **Duplicate-tag `assert`** | Replaced by log-and-drop (D8). Divergence, recorded. |
| **`MediaQuery` padding compensation in the default shuttle** (`heroes.dart:1092-1116`) | Needs `MediaQueryData.padding` interpolation; adds nothing to the flight contract. The default shuttle returns `toHero.child`. |
| **Placeholder sizing subtleties** — `_shouldIncludeChild` for the *pop* direction, `keepPlaceholder` across diverts | Lands with U5, where the divert cases make them observable. Implementing them earlier would ship untested code. |

---

## 9. Consequences

**Good.** The expensive-sounding blocker (cross-overlay reparenting) does not exist, and the mechanism it named is already implemented and tested. ADR-0020 U5.0 and U5.3 delivered `RenderOffstage`'s real-geometry mode and `ModalRoute.offstage` — exactly the two things `_HeroState` and `HeroController` need — before anyone knew Hero would need them in that shape. `TransitionGroup::Page` (ADR-0020 §7e) turns out to *be* Flutter's `toRoute is PageRoute` gate.

**Bad.** The real blocker (S4) is in `flui-rendering`, the densest crate, and it means admitting that `RenderBox::local_to_global` has been an identity stub in shipped code. Hero cannot be attempted until that is fixed, and fixing it touches four render objects and their harness tests.

**Ugly.** ADR-0019 §6 and tracker B1.4 have carried an incorrect blocker since 2026-07-09, and ADR-0020 §7d deferred `offstage`'s animation swap to "Hero's problem" without noticing that the swap is what makes the measurement *correct* rather than merely *tidy*. Both are corrected here. The lesson is the one the Definition of Done already states: a blocker recorded without a `.flutter/` cross-check is a hypothesis, and hypotheses ossify into "known facts" across sessions.

---

## Open questions for the deciders

1. **S4's scope.** Implement `transform_to` for the strict descendant→ancestor case only (all Hero needs), or the full `getTransformTo` with a common-ancestor search? The narrow one is a walk; the general one needs an inverse.
2. **S3's anchor (new, §7d).** `RouteSubtree` needs a render object at the root of the route's page subtree to publish its `RenderId` on `attach`. Does it live in `flui-objects` as a public, harness-tested render object, or privately in `flui-widgets` (which defines none today and would sit outside the harness catalog guard)?
3. **UNKNOWN-3.** What replaces `flightShuttleBuilder`'s two `BuildContext`s? This is the only public signature Hero cannot copy from Flutter.
3. **D8.** Is log-and-drop the right answer for duplicate tags, or is a duplicate tag a *framework* invariant (Flutter asserts, and a wrong flight is very visible) that earns an `expect("BUG: …")`?
4. **S7.** Registry over element walk trades a downcast for three enumerated narrowings. Is the nested-navigator narrowing acceptable to close B1.4, or must "Hero works" mean it works under a nested navigator?
5. **D4 / UNKNOWN-2.** Does the primary animation proxy belong on `ModalRoute` (as in Flutter) or can `TransitionRoute` host it without disturbing the status listener that ADR-0020 U5.2 built?
