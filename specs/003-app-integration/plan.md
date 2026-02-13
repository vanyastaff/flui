# 003 App Integration Sprint Plan

**Created**: 2026-02-13
**Status**: Ready for implementation
**Scope**: Fix critical flui-app architectural issues and wire up missing integration points
**Effort**: 1-2 development sessions (P0 items only)

## Context

The flui-platform MVP (002) delivered a solid cross-platform abstraction layer. The runner
in flui-app (`runner.rs`) successfully opens a window, creates a GPU renderer, mounts a root
widget, and registers platform callbacks. However, the architectural analysis revealed 10 gaps
preventing real interactive applications. This sprint fixes the 5 highest-impact issues.

## Sprint Tasks (5 tasks)

### Dependency Graph

```
Task 1 (PipelineOwner) ───> Task 4 (Hit Testing)
Task 2 (On-demand rendering) - independent
Task 3 (Lifecycle) - independent
Task 5 (Window handle) - independent (but benefits from Task 1 context)

Parallel tracks:
  Track A: Task 1 -> Task 4
  Track B: Task 2 (anytime)
  Track C: Task 3 (anytime)
  Track D: Task 5 (anytime, but after Task 1 is cleaner)
```

---

### Task 1: Unify PipelineOwner (fix architectural bug)

**Priority**: P0 -- blocks hit testing, correct layout, and all downstream rendering
**Complexity**: S (small, surgical changes)
**Constitution check**: Composition over inheritance (IV), strict crate DAG (II)

#### Problem

`AppBinding` creates its own `shared_pipeline_owner: Arc<RwLock<PipelineOwner>>` (binding.rs:103).
`RenderingFlutterBinding` creates a separate `root_pipeline_owner: RwLock<PipelineOwner>>`
(renderer_binding.rs:76). These are two independent instances. `draw_frame()` in AppBinding
flushes the shared one; `RenderingFlutterBinding::draw_frame()` flushes its own. Elements get
the shared pipeline via `render_pipeline_arc()`, but `RenderingFlutterBinding` manages render
views on its own pipeline. Result: render objects inserted by elements are invisible to the
renderer binding's layout/paint pipeline.

Also fixes **GAP 10** (runner.rs:146-151 calls both `Scheduler::instance()` and
`Scheduler::arc_instance()`, causing double-execution of frame callbacks).

#### Plan

**Option chosen**: AppBinding owns the canonical PipelineOwner. RenderingFlutterBinding
delegates to it instead of creating its own.

1. **`crates/flui-app/src/bindings/renderer_binding.rs`**:
   - Change `root_pipeline_owner` from `RwLock<PipelineOwner>` (owned) to
     `Arc<RwLock<PipelineOwner>>` (shared reference).
   - Accept the `Arc<RwLock<PipelineOwner>>` in `new()` via a parameter.
   - Update `RendererBinding::root_pipeline_owner()` to return a reference into the Arc.
   - Update `draw_frame()` to use the shared pipeline (no more separate flush).

2. **`crates/flui-app/src/app/binding.rs`**:
   - Create `shared_pipeline_owner` first, then pass `Arc::clone` to
     `RenderingFlutterBinding::new(pipeline)`.
   - Remove any duplicate pipeline flush from `draw_frame()` -- delegate entirely to
     `RenderingFlutterBinding::draw_frame()` for phases 2-4, keeping only the Scene
     creation in AppBinding.

3. **`crates/flui-app/src/app/runner.rs`**:
   - Remove the duplicate `Scheduler::arc_instance()` calls (lines 149-151).
   - Keep only `Scheduler::instance()` path.

#### Files to Change

| File | Change |
|------|--------|
| `crates/flui-app/src/bindings/renderer_binding.rs` | Accept shared pipeline in constructor |
| `crates/flui-app/src/app/binding.rs` | Pass pipeline to RenderingFlutterBinding, simplify draw_frame |
| `crates/flui-app/src/app/runner.rs` | Remove duplicate scheduler calls |

#### Acceptance Criteria

- [ ] Only ONE `PipelineOwner` instance exists at runtime (the one in `AppBinding::shared_pipeline_owner`)
- [ ] `RenderingFlutterBinding` references the same `PipelineOwner` as `AppBinding`
- [ ] `draw_frame()` flushes layout/paint exactly once per frame
- [ ] `Scheduler::arc_instance()` is not called in runner.rs
- [ ] `cargo test -p flui-app` passes
- [ ] `cargo clippy -p flui-app -- -D warnings` passes

#### Verification

```bash
cargo test -p flui-app
cargo clippy -p flui-app -- -D warnings
# Manual: add tracing to PipelineOwner::new() and verify it's called exactly once
```

---

### Task 2: Implement on-demand rendering (constitution compliance)

**Priority**: P0 -- constitution violation (Performance Constraints: on-demand rendering required)
**Complexity**: S (small, localized change)
**Constitution check**: ControlFlow::Wait, render only when dirty

#### Problem

`on_request_frame` fires every vsync but `render_frame()` always runs the full pipeline
regardless of whether anything changed. The `needs_redraw` flag exists but is never checked
before rendering. This wastes GPU cycles and violates the constitution's performance requirement.

#### Plan

1. **`crates/flui-app/src/app/runner.rs`** (`on_request_frame` callback, ~line 142):
   - Before running the pipeline, check `AppBinding::instance().has_pending_work() ||
     AppBinding::instance().needs_redraw()`.
   - If neither is true, skip the entire frame (no scheduler callbacks, no render_frame).
   - Still call `window.request_redraw()` at end if there IS pending work (to keep the
     frame loop alive while dirty).

2. **`crates/flui-app/src/app/binding.rs`**:
   - Ensure `has_pending_work()` correctly reflects all dirty sources: widget rebuilds
     (`widgets.has_pending_builds()`), layout/paint dirty nodes
     (`shared_pipeline_owner.has_dirty_nodes()`), and animation ticks (scheduler has
     pending callbacks).

#### Files to Change

| File | Change |
|------|--------|
| `crates/flui-app/src/app/runner.rs` | Add dirty check before pipeline execution |
| `crates/flui-app/src/app/binding.rs` | Extend `has_pending_work()` to include scheduler state |

#### Acceptance Criteria

- [ ] When no state changes occur, `render_frame()` is NOT called
- [ ] When `request_redraw()` is called, the next frame renders
- [ ] When `mark_needs_build()` triggers a widget rebuild, the frame renders
- [ ] When `mark_needs_layout()` triggers layout, the frame renders
- [ ] `cargo test -p flui-app` passes

#### Verification

```bash
cargo test -p flui-app
# Manual: add tracing::debug!("Frame skipped - nothing dirty") to the skip path
# Run app, verify frames are skipped when idle
```

---

### Task 3: Integrate lifecycle and graceful shutdown

**Priority**: P0 -- no graceful shutdown, no lifecycle callbacks, duplicate lifecycle types
**Complexity**: M (medium, touches multiple files + platform integration)
**Constitution check**: Strict crate DAG (II), no duplicate types

#### Problem

`AppLifecycle` in flui-app (lifecycle.rs) is an unused enum. `flui-platform` has its own
`LifecycleState` + `PlatformLifecycle` trait. The runner never registers `on_quit`,
`on_close`, or `on_should_close` callbacks. There is no graceful shutdown path.

#### Plan

1. **Delete `crates/flui-app/src/app/lifecycle.rs`** -- replace with re-export of
   `flui_platform::LifecycleState`. The platform's version is the source of truth.
   Update `mod.rs` accordingly.

2. **`crates/flui-app/src/app/binding.rs`**:
   - Add `lifecycle: AtomicU8` field (or use `parking_lot::Mutex<LifecycleState>`) to
     track current lifecycle state.
   - Add `transition_lifecycle(&self, new_state: LifecycleState)` method that updates
     state and notifies listeners.
   - Add `on_lifecycle_change(&self, callback: Box<dyn FnMut(LifecycleState) + Send>)`
     for framework consumers.

3. **`crates/flui-app/src/app/runner.rs`** (in `run_desktop`):
   - Register `platform.on_quit()` callback that transitions lifecycle to Detached and
     performs cleanup (drop renderer, release GPU resources).
   - Register `window.on_close()` callback that calls `platform.quit()`.
   - Register `window.on_should_close()` callback that returns `true` (default behavior;
     apps can override later via AppBinding API).
   - Register `window.on_active_status_change()` callback that transitions between
     Resumed/Inactive states.

#### Files to Change

| File | Change |
|------|--------|
| `crates/flui-app/src/app/lifecycle.rs` | Replace with re-export of platform lifecycle |
| `crates/flui-app/src/app/binding.rs` | Add lifecycle state + transition + listeners |
| `crates/flui-app/src/app/runner.rs` | Register on_quit, on_close, on_should_close, on_active_status_change |
| `crates/flui-app/src/app/mod.rs` | Update module declarations if needed |

#### Acceptance Criteria

- [ ] `AppLifecycle` enum is removed; `flui_platform::LifecycleState` is used instead
- [ ] `on_quit` callback is registered and fires on app exit
- [ ] `on_close` callback is registered on the window
- [ ] `on_should_close` callback is registered (returns true by default)
- [ ] `on_active_status_change` callback transitions lifecycle state
- [ ] No duplicate lifecycle types exist between flui-app and flui-platform
- [ ] `cargo test -p flui-app` passes
- [ ] `cargo clippy -p flui-app -- -D warnings` passes

#### Verification

```bash
cargo test -p flui-app
cargo clippy -p flui-app -- -D warnings
# Manual: run app, close window, verify clean shutdown in tracing output
# Manual: run app, alt-tab away, verify lifecycle transitions logged
```

---

### Task 4: Wire up hit testing through render tree

**Priority**: P0 -- blocks ALL interactive applications (clicks, hovers, gestures)
**Complexity**: M (medium, requires understanding render tree traversal)
**Depends on**: Task 1 (unified PipelineOwner)
**Constitution check**: Flutter as reference (I), composition over inheritance (IV)

#### Problem

`handle_pointer_event` in `AppBinding` (binding.rs:404-407) always returns an empty
`HitTestResult`. The `RenderingFlutterBinding::hit_test_in_view` method exists but
`render_views` is never populated. No `RenderView` is registered during `mount_root`.

#### Plan

1. **`crates/flui-app/src/app/runner.rs`** (in `mount_root`):
   - After mounting the root element, retrieve its `RenderId` and create/register a
     `RenderView` with `RenderingFlutterBinding`.
   - Use `binding.renderer_mut().add_render_view(view_id, render_view)`.
   - The `view_id` should be `0` for the primary view (or derive from window ID).

2. **`crates/flui-app/src/app/binding.rs`** (`handle_input`):
   - Replace the stub closure with actual hit testing:
     ```rust
     self.gestures.handle_pointer_event(&pointer_event, |position| {
         let renderer = self.renderer.read();
         let mut result = HitTestResult::new();
         renderer.hit_test_in_view(&mut result, position.into(), 0);
         result
     });
     ```
   - Import the necessary types from `flui_rendering`.

3. **Verify `RenderView::hit_test`** actually traverses the render tree. If it delegates
   to `RenderObject::hit_test`, the plumbing should work once the render tree is populated
   via the unified PipelineOwner (Task 1).

#### Files to Change

| File | Change |
|------|--------|
| `crates/flui-app/src/app/runner.rs` | Register RenderView during mount_root |
| `crates/flui-app/src/app/binding.rs` | Replace stub hit testing with real render tree traversal |

#### Acceptance Criteria

- [ ] A `RenderView` is registered with `RenderingFlutterBinding` during `mount_root`
- [ ] `handle_pointer_event` performs actual hit testing through the render tree
- [ ] Pointer events reach the correct render objects (verified by tracing)
- [ ] `HitTestResult` contains entries when clicking on rendered content
- [ ] `cargo test -p flui-app` passes
- [ ] `cargo clippy -p flui-app -- -D warnings` passes

#### Verification

```bash
cargo test -p flui-app
cargo clippy -p flui-app -- -D warnings
# Integration test: create a simple widget, simulate pointer event at widget position,
# verify HitTestResult is non-empty
```

---

### Task 5: Store window handle in AppBinding

**Priority**: P1 -- enables future window control, prerequisite for multi-window
**Complexity**: S-M (small-medium, new struct + storage)
**Constitution check**: Composition (IV), no Arc<Mutex> for tree structures but OK for
single window state

#### Problem

`run_desktop` creates a `Box<dyn PlatformWindow>` but it's consumed by closure captures.
After setup, there's no way to access the window (change title, toggle fullscreen, query
size). This blocks any runtime window manipulation.

#### Plan

1. **`crates/flui-app/src/app/binding.rs`**:
   - Add a `WindowState` struct:
     ```rust
     pub(crate) struct WindowState {
         pub window: Box<dyn PlatformWindow>,
     }
     ```
   - Add `active_window: Mutex<Option<WindowState>>` field to `AppBinding`.
   - Add `set_window(&self, window: Box<dyn PlatformWindow>)` and
     `with_window<R>(&self, f: impl FnOnce(&dyn PlatformWindow) -> R) -> Option<R>`.

2. **`crates/flui-app/src/app/runner.rs`** (in `run_desktop`):
   - After `open_window`, store the window in AppBinding before registering callbacks.
   - Callbacks access the window through AppBinding rather than closure captures where
     possible, or continue capturing `Arc` references where needed (for the `Renderer`
     which is not a platform window concern).

3. **Public API**: Expose `AppBinding::with_window()` so framework consumers can query/control
   the window at runtime (e.g., `binding.with_window(|w| w.set_title("New Title"))`).

#### Files to Change

| File | Change |
|------|--------|
| `crates/flui-app/src/app/binding.rs` | Add WindowState, storage, accessor methods |
| `crates/flui-app/src/app/runner.rs` | Store window in AppBinding after creation |

#### Acceptance Criteria

- [ ] `AppBinding` holds a reference to the active `PlatformWindow`
- [ ] `with_window()` API allows runtime window access (title, size, fullscreen)
- [ ] Window can be accessed after initial setup in runner
- [ ] `cargo test -p flui-app` passes
- [ ] `cargo clippy -p flui-app -- -D warnings` passes

#### Verification

```bash
cargo test -p flui-app
cargo clippy -p flui-app -- -D warnings
# Manual: after app starts, call with_window(|w| w.set_title("Runtime Title"))
# Verify title bar changes
```

---

## Risk Assessment

| Risk | Impact | Likelihood | Mitigation |
|------|--------|-----------|------------|
| PipelineOwner unification breaks existing tests | Medium | Medium | Run full test suite after each change. The singleton pattern in RenderingFlutterBinding may conflict -- check `impl_binding_singleton!` macro. |
| Hit testing requires RenderObject changes | Medium | Low | RenderView::hit_test and RenderObject::hit_test already exist in flui-rendering. We're wiring, not implementing. |
| Lifecycle removal breaks downstream code | Low | Low | `AppLifecycle` is unused (grep confirms no references outside lifecycle.rs tests). Safe to replace. |
| Window storage creates ownership issues | Medium | Medium | Use `Mutex<Option<Box<dyn PlatformWindow>>>` for interior mutability. Callbacks that need the window should capture an `Arc` clone, not borrow from AppBinding. |
| On-demand rendering causes missed frames | Low | Low | Always render at least one frame after state change. Use `request_redraw()` as the trigger. |

## What This Sprint Does NOT Cover

These are deferred to a future sprint (see architect's P1/P2 items):

- **EmbedderScheduler integration** (P1 #5) -- replaces direct Scheduler calls with proper frame lifecycle. Do after this sprint's scheduler fix proves stable.
- **Multi-window support** (P2 #9) -- Task 5 is a prerequisite. Full multi-window needs per-window PipelineOwner and RenderView routing.
- **DebugFlags / OverlayManager wiring** (P1 #7, #8) -- built but disconnected. Wire after core pipeline is solid.
- **Theme integration** (P2 #12) -- needs InheritedView pattern in flui-view.
- **Re-enabling disabled crates** (P2 #10, #11, #13) -- animation, reactivity, devtools. Need integration points defined first.

## Recommendation

**Create spec 003-app-integration** (this document serves as the plan). The changes are
surgical and well-scoped to flui-app. No new crate APIs are introduced, no cross-crate
breaking changes. The work is focused enough to not need a full specification document --
this plan is sufficient to guide implementation.

After this sprint completes, evaluate whether a spec is needed for multi-window support
(which DOES cross crate boundaries and introduces new public APIs).

## Verification Gate (end of sprint)

```bash
# All must pass:
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

After the sprint, the framework should support: a single-window app with correct render
pipeline ownership, efficient on-demand rendering, lifecycle management with graceful
shutdown, working pointer hit testing, and runtime window access. This is the minimum
viable application framework.
