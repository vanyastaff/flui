# ADR-0021: Hero flight seam — an overlay copy, ancestor-relative geometry, a post-frame measurement

- **Status:** Accepted
- **Date:** 2026-07-09
- **Superseded in part by:** ADR-0078 (capability acquisition)

## Context

Earlier records named Hero's blocker as "`GlobalKey` reparenting across overlay entries".
Reading `heroes.dart` shows Flutter reparents nothing. The hero's own subtree stays in its
route, wrapped in an `Offstage` and still mounted; what flies is a freshly inflated copy in a
new `OverlayEntry` (the default shuttle is `toHero.child`). The only `GlobalKey` in
`heroes.dart` lives inside one hero's own `build`.

Flutter's sequence: `HeroController` (a `NavigatorObserver`) reacts to `didChangeTop`; if both
routes are `PageRoute`s it sets the destination route `offstage` (which pins its animations to
their end values) and defers to a post-frame callback; that callback clears `offstage`,
collects the heroes of each route, measures both with `getTransformTo(routeRenderObject)`,
freezes placeholders and inserts a flight entry that re-measures the destination every tick.

What FLUI actually lacked was geometry (no `applyPaintTransform`, identity
`local_to_global` stubs), a post-frame callback that runs after layout in the same frame, and
a way for an observer to reach its navigator, the routes and the overlay.

## Decision

### 1. Nothing is reparented; the shuttle is a fresh inflation

The source hero's child stays mounted offstage (`RenderOffstage` lays it out at real size and
skips paint, hit-test and semantics, ADR-0020). The shuttle is a new inflation of the hero
child in the flight's overlay entry, with its own state — as in Flutter. Moving the hero's
element into the overlay would be a divergence.

`HeroState::build` emits one constant chain, `SizedBox(size?) → Offstage → TickerMode → child`,
in and out of flight, so reconciliation keeps the child element by position and no `GlobalKey`
is needed. While a flight shows the placeholder, the offstage child's animations are muted.

### 2. Ancestor-relative geometry lives in `flui-rendering`

- `RenderObject::apply_paint_transform(child, child_offset, size, &mut Matrix4)`; the default
  composes the transform the object already reports for paint (`paint_effects(size)`) with a
  translation by the child's committed offset, which covers `RenderTransform`,
  `RenderRotatedBox` and `RenderFittedBox` with no override. Overrides exist where paint
  departs from that path: `RenderFractionalTranslation` (paint-time offset) and `RenderFlow`
  (per-child delegate transform, replayed).
- `PipelineOwner::{transform_to, local_to_global, global_to_local, box_size}`. A FLUI render
  object has no parent link or owner, so these live on the owner, and the old identity stubs on
  `RenderBox` are gone.
- `transform_to` is strict descendant → ancestor. It returns `None` when the ancestor is not
  an ancestor **or any node on the path has not been laid out** — never a matrix computed from
  a substituted zero size. `global_to_local` returns `None` for a singular matrix where Flutter
  returns `Offset.zero`.

`Matrix4::translate`/`scale`/`rotate_z` pre-multiply (Flutter post-multiplies); overrides
compose explicitly as `*transform = *transform * step`.

### 3. The frame contract: post-frame callbacks see this frame's layout

```text
Scheduler::drive_frame(vsync_time, pipeline):
    handle_begin_frame  transient callbacks, microtasks, one async-driver poll (ADR-0018)
    handle_draw_frame   persistent callbacks + task queue
    pipeline()          build, layout, compositing, paint
    end_frame           post-frame callbacks, timing, → Idle
```

Every frame driver — `HeadlessBinding::pump_frame` on its binding-local scheduler and each
platform runner on the singleton — goes through `drive_frame`. A pipeline that returns an
error completes the frame; one that panics is caught, the frame is aborted
(`abort_frame`: phase → `Idle`, no post-frame callbacks, queued callbacks survive) and the
unwind resumes. The pipeline occupies Flutter's persistent slot without being a registered
callback, because a headless binding owns its tree by value; consequently a registered
persistent callback runs *before* the pipeline, where Flutter's runs after. Post-frame parity
is claimed; persistent-phase parity is not.

### 4. The navigator exposes what the controller needs

- `NavigatorObserver::did_attach(NavigatorHandle)` / `did_detach()` (Flutter's
  `observer.navigator`).
- Observer notifications and `Route::dispose` run **after** the history lock is released:
  `RouteHistory` decides and returns owned `notifications` and `dying` routes in
  `FlushOutcome`; `NavigatorShared::apply` performs them. An observer may read or mutate the
  stack from any callback; a re-entrant push runs a fresh flush whose notifications follow the
  outer drain. Neighbour announcements, which need `&mut` on the entries, now precede observer
  callbacks (Flutter: after).
- `pub(crate)` introspection by `RouteId`: `route_peer` (animation, transition group),
  `route_modal` (`ModalHandle`: `set_offstage`, the hero registry), `route_subtree`,
  `is_current`, `overlay()`.
- `RouteSubtree { element_id, render_id }` marks the page subtree. A stateful view owns no
  render object and `find_render_object()` walks ancestors, so the page is wrapped in
  `RenderSubtreeAnchor` (`flui-objects`, harness-tested, not a repaint boundary), which
  publishes its id on `attach` and clears it on `detach`. The element id comes from
  `init_state`. The ids exist before layout; geometry comes from `box_size`, which is `None`
  until layout commits.
- The navigator holds a `PostFrameHandle` and a `PipelineOwner` handle, acquired in its
  lifecycle hooks (ADR-0078) and cleared at dispose, which makes a stale controller inert. If
  either is absent, the controller does not touch the route (it checks before setting
  `offstage`, or the destination would be stranded offstage).

### 5. `offstage` swaps the route's animations

`ModalRoute` owns primary and secondary `ProxyAnimation`s (Flutter's `_animationProxy`, on
`ModalRoute`, not `TransitionRoute`), and those — not the controller — are what `build_page`
and `build_transitions` receive. `offstage` points them at always-complete / always-dismissed,
so the offstage frame lays heroes out where they will land. Without the swap every destination
rect would be the entry position.

### 6. Discovery is a registry; tags are `Arc<dyn ViewKey>`

Each `Hero` registers with the nearest `HeroScope` (an inherited view whose registry lives on
the route's `ModalInner`) in `init_state` and deregisters in `dispose`. The controller reads it
by `RouteId`. This replaces Flutter's element walk plus `widget is Hero` downcast; a hero can
only register with its own route, which makes Flutter's `Navigator.of(hero) == navigator`
check structural. Tags use `ViewKey`'s `key_eq`/`key_hash`; nothing downcasts. A duplicate
tag in one route logs and keeps the **first** (deregistration compares `Arc::ptr_eq`, so the
loser cannot evict the winner); Flutter asserts in debug and keeps the last in release.

`HeroMode` provides an inherited `bool` composed as `ancestor && enabled`, so a nested enabled
scope cannot re-enable a disabled subtree; manifest collection skips a tag whose hero on either
side is disabled.

### 7. The controller and the flight

`HeroController` reacts to `did_change_top` (not push/pop), gates on both routes being in the
page transition group, and takes the direction from the two routes' animation statuses. The
post-frame measurement builds a `HeroFlightManifest` per tag present on both sides, with each
rect in its own route's space via `transform_to(hero, route_subtree)`; a non-finite rect aborts.

`FlightManager` keeps one `HeroFlight` per tag: a `ProxyAnimation` over the driving route's
animation (reversed for a pop) wrapped in `CurvedAnimation` with `Hero::curve` (default
`FastOutSlowIn`) and `reverse_curve` (default `curve.flipped()`, the 180° rotation; none when
diverting). A push reads the destination hero's curve, a pop the source's. The flight entry is
`Stack[Positioned(IgnorePointer(Fade(shuttle)))]` — `RenderTheater` ignores positioned
children, so the inner stack is required. Each tick re-measures the destination and re-aims;
if the destination is gone, the shuttle fades out while the flight keeps flying.

- **Divert.** A second transition for an airborne tag redirects the same flight and overlay
  entry (push↔pop and same-direction branches of `_HeroFlight.divert`). The proxy is repointed
  last, with no flight lock held, because `set_parent` fires `on_tick` synchronously.
- **Retire, don't drop.** A flight ends inside its own animation's status listener, so it is
  moved to `retired` and dropped at end of frame through the `PostFrameHandle`.

### 8. Public surface

`Hero` (`new(tag: impl ViewKey, child)`, `create_rect_tween`, `flight_shuttle_builder`,
`placeholder`, `curve`, `reverse_curve`), `HeroController`, `HeroControllerScope`, `HeroMode`,
`FlightDirection`. Registry, handles, manifests, flights and `HeroTag` stay private.

- `create_rect_tween`: per-hero, else controller-level, else linear `RectTween`; re-invoked on
  re-aim, as Flutter recreates the tween.
- `flight_shuttle_builder` receives the animation, direction and both hero child views, not
  the two foreign `BuildContext`s Flutter passes; that is what those contexts are used for, and
  handing out another route's context would reach across routes.
- `placeholder(Fn(Size) -> View)` renders the custom visual as a sibling of the preserved
  offstage child (`SizedBox → Stack[Offstage → child, placeholder]`), so the child's state
  survives by construction. Flutter's `placeholderBuilder` replaces the child and loses its
  state; FLUI does not port that shape, and does not reuse its name.
- **Automatic attach.** `HeroControllerScope::new(controller, child)` / `none(child)` is an
  inherited `Option<Arc<HeroController>>`. With no scope above it, a navigator creates a default
  controller (Flutter relies on `MaterialApp` to install one; FLUI has no such host). Each
  navigator wraps its overlay in `HeroControllerScope::none`, so nested navigators are isolated
  unless given their own scope. A manually added hero observer
  (`NavigatorObserver::observes_hero_flights`) replaces the default. A controller already
  attached to a mounted navigator refuses a second one (Flutter reports and lets both run).
  The scope is read once in `init_state`.

## Flutter divergences

Listed above: registry instead of element walk; first-wins duplicate tags; no foreign contexts
for the shuttle builder; state-preserving `placeholder`; auto-default controller; announcements
before observer callbacks; persistent callbacks before the pipeline; `did_change_top` without
Flutter's `isCurrent` assert (a re-entrant push can deliver it for a route no longer top;
introspection returns `None` for it). Also: the reverse tween is a begin/end swap, exact only
for linear tweens — an arc tween must switch to a true reverse tween.

**Not carried:** user-gesture flights (`transitionOnUserGestures`,
`didStart/StopUserGesture`; FLUI has no back-swipe), cross-navigator flights, `MediaQuery`
padding compensation in the default shuttle, hero-specific semantics.

## Alternatives rejected

- **Reparenting the hero's element into the overlay.** Not Flutter's mechanism, and it would
  lose the route-side layout the destination measurement depends on.
- **An offset-only walk for hero rects.** Silently wrong under any transforming ancestor.
- **An element walk with a `dyn Any` downcast for discovery.** O(route subtree) per
  transition and a new erased boundary; the registry is O(heroes) and downcast-free.
- **`dyn Any`, a generic `Hero<T>`, or `String` for tags.**
