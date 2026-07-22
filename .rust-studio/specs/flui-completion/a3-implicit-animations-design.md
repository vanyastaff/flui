# A3 — Implicit animations design (durable)

Flagship unit. Flutter's `ImplicitlyAnimatedWidget` family ported to FLUI, deterministic via the A2b `HeadlessBinding`.

## Key architectural decision: NO flui-view surgery

The naive path (controller lives in State, but `AnimatedView::listenable()` is on the View which is recreated each build) breaks because `AnimatedBehavior::on_update` re-subscribes to `core.view().listenable()` — a *new* notifier per rebuild. Instead:

**An implicit widget is a `StatefulView` holding a persistent `AnimationController` in its State. Its `build()` returns an `AnimatedBuilder` (a general `AnimatedView` primitive) whose `listenable` is the controller.** The controller's `ChangeNotifier` has **stable identity** across rebuilds (Arc-backed, shared across `.clone()`), so even though each `build()` mints a fresh `Arc<dyn Listenable>` *object*, add/remove resolve against the same notifier — `AnimatedBehavior`'s re-subscribe-on-update is leak-free. The implicit widget itself does NOT subscribe to its controller (so it does not rebuild every frame); only the inner `AnimatedBuilder` rebuilds per tick. The implicit widget rebuilds only on config change (parent-driven `did_update_view`).

## Pieces (bottom-up)

1. **flui-animation `Vsync`** (`src/vsync.rs`): Arc-backed registry of controllers. `register(controller) -> RegistrationId`, `unregister(id)`, `tick_all(now_secs)` — restart-aware (re-anchor per-run `t=0` on `run_generation()` bump; tick only running controllers). Extracted from A2b's inline `RegisteredController` logic so the binding AND the `VsyncScope` share the type. Plus `AnimationController::set_duration(Duration)` for Flutter's `controller.duration = widget.duration` on retarget.

2. **flui-widgets `AnimatedBuilder`** (`src/transitions/animated_builder.rs`): an `AnimatedView` over `listenable: Arc<dyn Listenable>` + `builder: Arc<dyn Fn() -> BoxedView + Send + Sync>`. `build()` calls the builder. Flutter's `AnimatedBuilder` — a standard, well-known primitive (leapfrog-neutral).

3. **flui-widgets `VsyncScope`** (`src/animated/vsync_scope.rs`): `InheritedView` providing a `Vsync` to the subtree — mirrors `GestureArenaScope` exactly. Implicit widgets read it in `init_state` (`ctx.get::<VsyncScope,_>`) and register their controller. No scope above → controller is not binding-driven (production scheduler drives it); graceful fallback like a private gesture arena.

4. **flui-widgets implicit widgets** (`src/animated/`): `AnimatedOpacity`, `AnimatedPadding`, `AnimatedAlign`, `AnimatedContainer`. Each `StatefulView`; State holds `controller`, `curved: CurvedAnimation`, `tween`, current config, `vsync_reg: Option<RegistrationId>`.
   - `create_state(&self)`: build controller (`AnimationController::new(duration, Arc::new(Scheduler::new()))`), `CurvedAnimation::new(controller, curve)`, initial tween `begin=end=target` (so first frame shows the target, no animation), capture config + child.
   - `init_state(ctx)`: read `VsyncScope` → `vsync.register(controller)` → store id.
   - `build(&self, view, ctx)`: `AnimatedBuilder::new(controller_as_listenable, move || Wrapped::new(tween.transform(curved.value())).child(child))`.
   - `did_update_view(&mut self, old, new)`: if target changed → `tween = Tween(begin = tween.transform(curved.value()) /*current display*/, end = new.target)`; `controller.set_duration(new.duration)`; `controller.forward_from(Some(0.0))`. Update child/config.
   - `dispose(&mut self)`: `vsync.unregister(id)` + `controller.dispose()`.

5. **flui-view** `ViewState::did_update_view(&mut self, old_view, new_view)` — add `new_view` param (Flutter's `didUpdateWidget(old)` + `this.widget`). 2 sites: trait default + 1 test. `StatefulBehavior::on_view_updated` forwards `core.view()` as new.

6. **flui-binding**: drive `Vsync` instead of inline `Vec` (refactor A2b); `vsync() -> Vsync`; `adopt_vsync(Vsync)` so the harness shares the scope's vsync. Harness `lay_out` wraps root in `VsyncScope(vsync)` and `adopt_vsync`s it.

## Discriminating tests
- `animated_opacity_interpolates_to_new_target_over_frames` — change opacity, pump 5 frames, opacity climbs old→new monotonically, intermediate strictly between.
- `animated_opacity_retargets_midflight` — change target mid-animation, begins from current displayed value (not a snap).
- `animated_padding/align/container` analogous (EdgeInsets/Alignment/multi-prop tweens).
- `implicit_first_frame_shows_target_no_animation` — initial build sits at target, no motion.

## Why not a dedicated flui-view behavior
`ElementCore::create_mark_dirty_callback()` (the only `'static` rebuild handle) is reachable from a Behavior but not from a State via `BuildContext`, and `init_state` runs inside first `build_into_views`, not `on_mount` — so a state-subscribes-controller path needs new behavior+element+trait plumbing. The AnimatedBuilder indirection reuses the existing `AnimatedView`/`AnimatedBehavior` machinery with zero flui-view internals touched, at the cost of one extra element per implicit widget (cheap, a proxy).
