# ADR-0021: Hero flight seam — an overlay copy, ancestor-relative geometry, a post-frame measurement

- **Status:** Accepted (2026-07-09)
- **Date:** 2026-07-09
- **Superseded in part by:** ADR-0078 (capability acquisition)
- **Amended:** 2026-11-03 — gesture-driven flights (edge swipe-back) and flights across nested navigators are implemented and stated as §9; the "not carried" entries for them are removed. §1 to §7 are unchanged, and §8's public surface lists `Hero::transition_on_user_gestures`.

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
`placeholder`, `curve`, `reverse_curve`, `transition_on_user_gestures` of §9), `HeroController`, `HeroControllerScope`, `HeroMode`,
`FlightDirection`. Registry, handles, manifests, flights and `HeroTag` stay private.

- `create_rect_tween`: per-hero, else controller-level, else linear `Tween<Rect>`; re-invoked on
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

### 9. Gesture-driven flights and nested navigators

A flight can start from a user gesture (the edge swipe-back) rather than from a committed route
change. The seam of §3 and §7 is unchanged; these rules add the gesture entry to it, and
`Hero::transition_on_user_gestures` joins the public surface of §8.

- **The navigator counts gestures.** `NavigatorHandle::did_start_user_gesture` increments an
  atomic count; only the 0 to 1 edge resolves the top route and its predecessor and calls
  `NavigatorObserver::did_start_user_gesture(route, previous)`, and only the 1 to 0 edge calls
  `did_stop_user_gesture`. The stop edge saturates at zero, so an unmatched stop is a no-op in
  release and a debug assertion in debug builds. Observers run after the history lock is
  released, as in §4 (`crates/flui-widgets/src/navigator/navigator.rs:1243`, `:1255`; the
  counting itself at `:712`, `:732`).
- **The gesture source.** `BackGestureController::new` calls the navigator's
  `did_start_user_gesture` before its first drag update; a route takes part through
  `PageRoute::back_gesture(true)`
  (`crates/flui-widgets/src/navigator/back_gesture.rs:108`,
  `crates/flui-widgets/src/navigator/page_route.rs:339`).
- **A gesture flight is always a pop.** `HeroController::did_start_user_gesture` runs the same
  eligibility and launch path as `did_change_top`, with the dragged route as `from` and the route
  it would reveal as `to`, and `FlightDirection::classify` returns `Pop` for a gesture before it
  reads either route's status
  (`crates/flui-widgets/src/navigator/hero_controller.rs:861`, `:139`). The commit of the swipe
  pops the route and fires `did_change_top` while the gesture is still in progress; that call
  returns without a second flight (`hero_controller.rs:825`).
- **A synchronous start.** For a gesture pop whose destination `maintain_state` and whose page
  subtree already has a finite laid-out size, the measurement and launch run inside the observer
  callback: no offstage flip and no post-frame schedule (`hero_controller.rs:405`). Otherwise the
  flight takes the post-frame path of §3.
- **Heroes opt in.** `Hero::transition_on_user_gestures(bool)` defaults to `false`; a tag flies
  during a gesture only when the hero on both routes opts in, and a hero left out is un-hidden if
  an earlier flight froze it (`crates/flui-widgets/src/navigator/hero.rs:1123`;
  `HeroController` `filter_for_gesture`, `hero_controller.rs:682`).
- **A drag that never moves still ends.** A gesture pop flight records
  `is_user_gesture_transition` (`crates/flui-widgets/src/navigator/hero_flight.rs:82`). On the stop
  edge `HeroController::did_stop_user_gesture` returns while another gesture is in progress, then
  `finish_stalled_gesture_pops` finishes every airborne gesture pop whose proxy never left
  `ReverseSettled`, through the same `finish` a real terminal status uses
  (`hero_controller.rs:872`, `hero_flight.rs:1351`). The sweep attempts every flight under one
  `PanicRecovery` and resumes the first failure after the last one, so a panicking hero callback
  leaves no other flight airborne. Teardown is idempotent (`hero_flight.rs:491`), so a flight
  reached by both its shuttle and the sweep ends once.
- **Nested navigators.** A route's heroes for a flight are `ModalHandle::all_heroes`: its own plus,
  recursively, those a nested `Navigator` publishes for its current `PageRoute`
  (`crates/flui-widgets/src/navigator/modal_route.rs:976`). The nested navigator re-resolves the
  enclosing route's registry on every build, moves its hero source when that registry changed,
  and withdraws it at dispose, so a disposed navigator's heroes are never matched
  (`navigator.rs:2887`, `:3072`). A
  `HeroControllerScope::none` stops the nested navigator from attaching its own controller; it does
  not stop the outer controller from matching its heroes.

Confirmation: `hero_gesture::complete_release_pops_to_the_destination_route_and_the_flight_lands`
and `hero_gesture::an_excluding_gesture_restores_pending_programmatic_placeholders`
(`crates/flui-widgets/tests/hero_gesture.rs:260`, `:176`, rows of `crates/flui-widgets/tests/contracts.rs:1239`)
drive a gesture through `BackGestureController`;
`navigator::hero::registry_tests::nested_hero_resolution_preserves_local_identity_through_cycles`
(`crates/flui-widgets/src/navigator/hero.rs:608`) pins nested registration in `all_heroes`. A row
for a drag that never moves before its stop, finished by the sweep, is to be added to
`tests/hero_gesture.rs`.

## Flutter divergences

Listed above: registry instead of element walk; first-wins duplicate tags; no foreign contexts
for the shuttle builder; state-preserving `placeholder`; auto-default controller; announcements
before observer callbacks; persistent callbacks before the pipeline; `did_change_top` without
Flutter's `isCurrent` assert (a re-entrant push can deliver it for a route no longer top;
introspection returns `None` for it). Reversing an airborne flight between the same heroes
retains its selected rect factory and evaluates it with the original endpoint order at
mirrored progress. This retraces arbitrary custom mappings as well as linear tweens;
a different destination selects a new mapping.

**Not carried:** `MediaQuery` padding compensation in the default shuttle
(`crates/flui-widgets/src/navigator/hero.rs:879`), hero-specific semantics.

## Alternatives rejected

- **Reparenting the hero's element into the overlay.** Not Flutter's mechanism, and it would
  lose the route-side layout the destination measurement depends on.
- **An offset-only walk for hero rects.** Silently wrong under any transforming ancestor.
- **An element walk with a `dyn Any` downcast for discovery.** O(route subtree) per
  transition and a new erased boundary; the registry is O(heroes) and downcast-free.
- **`dyn Any`, a generic `Hero<T>`, or `String` for tags.**
