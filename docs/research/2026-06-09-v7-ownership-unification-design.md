[← Wave 2-3 design](2026-06-09-wave2-3-reactivity-design.md) · [Plan](2026-06-08-beat-flutter-plan.md)

# V-7 Ownership Unification — Design (gates Wave 2 setState)

> **Source:** V-7 scout (2026-06-09) over current main. Wave 2 (production setState) cannot ship until this lands. This resolves the scout's 5 architect unknowns into a sequenced refactor.

## The problem (settled by the scout)

The renderer paints from the **RenderTree** (`PipelineOwner.root_id`), which is fed **only** by `AppBinding.root_element` (Store A — the `RootRenderElement` the runner mounts). Three disconnects:

1. **`WidgetsBinding.element_tree` (Store B) is empty in production.** `attach_root_widget` (which would populate it) has **zero** production callers; the runner uses `mount_root`, which feeds Store A only.
2. **The child subtree lives in `RootRenderElement.child_element`, not the `ElementTree` slab** → no `ElementId`s → `schedule_build_for` has nothing to key on.
3. **Production setState reaches no queue.** `Element::set_state` → `ElementCore::mark_dirty` flips a local `AtomicBool` only; never calls `schedule_build_for`. The per-frame `build_scope` drains Store B's (empty) queue. The wake chain (`on_need_frame`/`on_build_scheduled`) is uninstalled, so a between-vsync setState never repaints.

**Net:** setState today is a no-op in production. Fixing it requires unifying onto a single, slab-resident element tree that `build_scope` drains and the RenderTree is fed from.

## The 5 forks — resolved

1. **Where does setState's dirty element reach the queue?** → Route through the live `ElementTree` + `BuildOwner`. Prerequisite: the element subtree must be **slab-resident** (fork 2). Then setState performs the proven dual-write of `schedule_root_rebuild` (binding.rs:770): mark local atomic **and** `schedule_build_for(id, depth)`.
2. **`RootRenderElement` standalone mount vs `ElementTree` materialization?** → **Materialize in `ElementTree`.** Switch the runner bootstrap from `mount_root` (Store A standalone) to `AppBinding::attach_root_widget` → `WidgetsBinding::attach_root_widget` (binding.rs:665, already tested) which mounts via `element_tree.mount_root_with_pipeline_owner` and schedules the first build. This gives every element a real `ElementId`.
3. **Pipeline-owner injection order?** → `AppBinding::attach_root_widget` must call `self.widgets().set_pipeline_owner(self.render_pipeline_arc())` **before** `widgets.attach_root_widget` (it currently does not — binding.rs:194). The single `shared_pipeline_owner` stays the one RenderTree owner.
4. **Window-wake plumbing?** → Install `WidgetsBinding::set_on_need_frame(|| app.request_redraw())` at bootstrap; `AppBinding::request_redraw` (binding.rs:353) already sets `needs_redraw`, which the on-demand loop polls (runner.rs:153). Wire `BuildOwner::on_build_scheduled` → `handle_build_scheduled` → `on_need_frame`. Now `schedule_build_for` both enqueues *and* wakes.
5. **RenderView double-registration** (runner's hit-test `RenderView` at runner.rs:282 vs `RootRenderElement`'s `RenderViewObject` at root.rs:183) → collapse to one. Adjacent to V-7; fold in to avoid a third divergence.

## Adversarial must-fixes (from the Wave 2-3 harsh-critic) folded in

- **FATAL deadlock:** `parking_lot` RwLock is non-reentrant; `draw_frame` holds `inner.write()` across `build_scope`. A setState fired *during* build self-deadlocks. → setState sets the local atomic lock-free and pushes the schedule onto a **side queue drained at the next phase boundary** (mirror `mid_layout_marks` in the pipeline owner), OR a pre-lock re-entrancy guard. Never take `inner.write()` from inside a build callback.
- **Rigid-downcast silent-loss:** a behavior-agnostic `state_as_any_mut` (sibling to `state_as_any`) so `AnimatedBehavior`/composed behaviors aren't skipped; enforce **mutation ⇒ schedule** (never schedule if the mutation was skipped, à la U33's queue⇒flag).
- **False-green:** an **end-to-end integration test** driving the real frame loop (red-then-green) — `build_scope`-direct unit tests cannot detect a no-op repaint.

## Sequenced units

- **V7-U1 — bootstrap via `attach_root_widget`.** Runner `mount_root` → `AppBinding::attach_root_widget` (set_pipeline_owner first). Subtree becomes slab-resident in `ElementTree`. Verify paint still works (RenderTree fed from the materialized tree).
- **V7-U2 — collapse RenderView double-registration** (fork 5).
- **V7-U3 — delete Store A** (`root_element`, `set_root_element`, `take_root_element`, `rebuild_root` — all zero-external-caller; blast radius = binding.rs + 3 runner call-sites).
- **V7-U4 — setState dual-write + deadlock-safe schedule.** `set_state_for<V,F>` on the binding: local-atomic lock-free + side-queue `schedule_build_for`; behavior-agnostic `state_as_any_mut`; mutation⇒schedule invariant.
- **V7-U5 — wake chain.** Install `on_need_frame`/`on_build_scheduled` → `request_redraw`.
- **V7-U6 — end-to-end test.** Mount a counter app, fire the callback between vsyncs, assert exactly one rebuild + the painted output changes (real loop).

V7-U1..U3 are the unification (no behavior change to the user — paint still works, just through one tree). V7-U4..U6 are Wave 2 proper (setState works). Each unit is a verifiable checkpoint.

## Risk

This rewires the framework's production bootstrap and deletes a root store. Blast radius is **contained** (scout confirmed Store A has no external readers; `attach_root_widget` is tested), but it is the spine — V7-U1 (does paint still work after switching bootstrap) is the gating checkpoint; if it regresses, stop and reassess before U2+.
