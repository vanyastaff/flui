# Design: Fix PipelineOwner Root Widget Attachment Lifecycle

**Change ID:** `fix-pipeline-attach-lifecycle`

## Architecture Context

This change operates within FLUI's three-tree architecture at the boundary between the View Tree (immutable) and Element Tree (mutable), coordinated by the `PipelineOwner`.

### System Overview

```
┌─────────────────────────────────────────────────────────────┐
│                     Application Layer                        │
│              (flui_app::WgpuEmbedder)                       │
└───────────────────────┬─────────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────────┐
│                  Binding Layer (AppBinding)                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │   Gesture    │  │  Scheduler   │  │   Renderer   │      │
│  │   Binding    │  │   Binding    │  │   Binding    │      │
│  └──────────────┘  └──────────────┘  └──────┬───────┘      │
│                                              │               │
│  ┌──────────────────────────────────────────▼───────────┐   │
│  │          PipelineBinding                              │   │
│  │    (Owns Arc<RwLock<PipelineOwner>>)                 │   │
│  └──────────────────────┬────────────────────────────────┘   │
└─────────────────────────┼─────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                    Core Framework Layer                      │
│                   (flui_core::pipeline)                      │
│                                                              │
│  ┌───────────────────────────────────────────────────────┐  │
│  │              PipelineOwner                            │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌─────────────┐ │  │
│  │  │   Build     │→ │   Layout     │→ │    Paint    │ │  │
│  │  │  Pipeline   │  │   Pipeline   │  │  Pipeline   │ │  │
│  │  └─────────────┘  └──────────────┘  └─────────────┘ │  │
│  │         ↓                  ↓                ↓         │  │
│  │  ┌──────────────────────────────────────────────────┐│  │
│  │  │           ElementTree (Slab-based)              ││  │
│  │  │   Component Elements ↔ Render Elements          ││  │
│  │  └──────────────────────────────────────────────────┘│  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

### Critical Path: Root Widget Attachment

The attachment of a root widget to the pipeline involves several coordinated steps across layers:

```
User Code: run_app(HelloWorldApp)
    ↓
AppBinding::attach_root_widget()
    ↓
PipelineBinding::attach_root_widget()
    ↓
PipelineOwner::attach() ← THIS IS WHERE THE BUGS WERE
    ├─ 1. Create BuildContext
    ├─ 2. Set build scope flag
    ├─ 3. Build widget tree (with BuildContextGuard) ← Bug #1 was here
    ├─ 4. Clear build scope flag
    ├─ 5. Set root element in tree
    └─ 6. Request initial layout ← Bug #2 was here (missing!)
```

## Problem Deep Dive

### Bug #1: BuildContext Guard Lifetime

**The Issue:**

BuildContext is stored in thread-local storage and accessed via a RAII guard (`BuildContextGuard`). The guard must stay alive for the entire View → Element conversion, including recursive child builds.

**Original Broken Code:**
```rust
let element = {
    let _guard = BuildContextGuard::new(&ctx);  // ← Guard created
    widget.into_element()  // ← Returns immediately
};  // ← Guard DROPPED here, but children need it!

// Problem: When into_element() recursively calls child.build(),
// those children try to access BuildContext from thread-local storage,
// but the guard is already dropped!
```

**Why This Broke:**

The View trait's `into_element()` method doesn't directly take a BuildContext parameter. Instead, it accesses BuildContext from thread-local storage:

```rust
// In flui_core/src/view/mod.rs
impl<V: View> IntoElement for V {
    fn into_element(self) -> Element {
        let ctx = current_build_context();  // ← Accesses thread-local
        let element_like = self.build(ctx);
        element_like.into_element()  // ← May recurse for children!
    }
}
```

When the guard is dropped too early, `current_build_context()` panics because the thread-local storage is empty.

**The Fix:**

Use a closure-based approach to extend the guard's lifetime:

```rust
let element = crate::view::with_build_context(&ctx, || {
    widget.into_element()  // ← Entire execution happens inside guard's scope
});

// The with_build_context() helper:
pub fn with_build_context<F, R>(context: &BuildContext, f: F) -> R
where
    F: FnOnce() -> R,
{
    let _guard = BuildContextGuard::new(context);
    f()  // ← Guard lives for entire closure execution
}
```

**Key Insight:** Rust's borrow checker ensures the guard outlives the closure execution, including all recursive calls within `f()`.

### Bug #2: Missing Layout Request

**The Issue:**

Flutter's rendering pipeline has three distinct phases that must execute in order:

1. **Build Phase**: Convert Views to Elements
2. **Layout Phase**: Compute sizes and positions
3. **Paint Phase**: Generate rendering commands

Each phase must be explicitly triggered. The old `FluiApp::build_root()` did this correctly:

```rust
// Old code (commit ed498f9) - CORRECT
fn build_root(&mut self) {
    let root_element = with_build_context(&ctx, || self.root_view.build_any());
    let root_id = self.pipeline.set_root(root_element);  // ← Build complete
    self.pipeline.request_layout(root_id);  // ← Trigger layout phase!
}
```

But during refactoring to `PipelineOwner::attach()`, this critical step was lost:

```rust
// New code (BROKEN)
pub fn attach<V>(&mut self, widget: V) -> ElementId
where
    V: View + 'static,
{
    // ... BuildContext setup ...
    let element = with_build_context(&ctx, || widget.into_element());
    let root_id = self.set_root(element);  // ← Build complete
    // ❌ MISSING: self.request_layout(root_id);

    tracing::info!(root_id = ?root_id, "Root view attached to pipeline");
    root_id
}
```

**Why This Matters:**

Without the layout request:
- `set_root()` only updates the element tree structure
- Layout pipeline never marks the root as dirty
- `flush_layout()` returns early because nothing needs layout
- Paint pipeline never executes (depends on layout)
- Result: Blank screen, even though elements exist in the tree

**The Fix:**

Add the explicit layout request:

```rust
let root_id = self.set_root(element);

// CRITICAL: Request layout for the entire tree after attaching root
// Without this, the UI won't layout/paint until an external trigger
self.request_layout(root_id);
```

**Design Principle:** Attachment must trigger the complete initialization sequence: Build → Layout → Paint.

### Bug #3: Resize Event Handling

**The Issue:**

Window resize changes the available space for layout but doesn't automatically invalidate the UI tree. The GPU surface must be reconfigured AND the layout must be recalculated.

**Original Code:**
```rust
WindowEvent::Resized(size) => {
    self.renderer.resize(size.width, size.height);  // ← GPU only
    // ❌ MISSING: Request layout with new constraints
}
```

**Why This Broke:**

The `renderer.resize()` only reconfigures the wgpu surface for the new dimensions. It doesn't touch the UI tree at all. The root element still has old BoxConstraints from the previous layout pass.

**The Fix:**

Request layout for the root element with new constraints:

```rust
WindowEvent::Resized(size) => {
    // 1. Reconfigure GPU surface
    self.renderer.resize(size.width, size.height);

    // 2. Trigger UI tree relayout with new constraints
    let pipeline = self.binding.pipeline.pipeline_owner();
    let mut pipeline_write = pipeline.write();
    if let Some(root_id) = pipeline_write.root_element_id() {
        pipeline_write.request_layout(root_id);
        tracing::debug!("Requested layout for root after resize");
    }
}
```

**Design Principle:** Any change to layout constraints must trigger relayout. Window resize is a constraint change.

## Performance Analysis

### BuildContext Allocation Strategy

**Question from User:** "а у нас там нет проблем того что каждый фремйм создает build context или что либо?"

**Answer:** No, BuildContext is NOT created per-frame. Here's why:

**Allocation Points:**

1. **Startup (Once):**
   ```rust
   // In PipelineOwner::attach() - called once
   let ctx = BuildContext::new(
       self.tree.clone(),
       ElementId::new(ROOT_PLACEHOLDER)
   );
   ```

2. **Rebuilds (Reuses HookContext):**
   ```rust
   // In BuildPipeline::rebuild_component()
   let ctx = BuildContext::with_hook_context_and_queue(
       tree.clone(),
       element_id,
       hook_context.clone(),  // ← Arc clone is cheap!
       self.rebuild_queue.clone(),
   );
   ```

**HookContext Persistence:**

The critical optimization is that `HookContext` is stored in the component's state and reused across rebuilds:

```rust
fn extract_or_create_hook_context(component: &mut ComponentElement)
    -> Arc<Mutex<HookContext>>
{
    if let Some(ctx) = component.state_mut()
        .downcast_mut::<Arc<Mutex<HookContext>>>()
    {
        // ✅ Reuse existing HookContext (cheap Arc clone)
        ctx.clone()
    } else {
        // Only on FIRST build
        let ctx = Arc::new(Mutex::new(HookContext::new()));
        component.set_state(Box::new(ctx.clone()));
        ctx
    }
}
```

**Frame Rendering Path:**

```
render_frame()
    ↓
binding.scheduler.begin_frame()
    ↓
binding.renderer.draw_frame()
    ↓
pipeline_owner.build_frame()
    ↓
build_pipeline.flush_rebuild_queue()  ← Only processes dirty elements!
    ↓
build_pipeline.rebuild_dirty()
    ↓
rebuild_component() ← Uses with_hook_context_and_queue (no new allocation)
```

**Memory Cost:**

- BuildContext creation at startup: **1 allocation**
- BuildContext during rebuilds: **0 allocations** (reuses HookContext via Arc)
- BuildContextGuard (RAII): **Stack allocated** (zero heap cost)

### Comparison with Flutter

| Aspect | Flutter | FLUI |
|--------|---------|------|
| BuildContext lifecycle | Per-widget instance | Per-component instance |
| HookContext persistence | N/A (uses InheritedWidget) | Stored in component state |
| Thread-safety | Single-threaded (Dart isolates) | Multi-threaded (Arc/Mutex) |
| Allocation strategy | GC-managed | Arc reference counting |

FLUI's approach is more efficient than Flutter's for multi-threaded scenarios because HookContext can be shared across threads safely via Arc/Mutex.

## Design Patterns Applied

### 1. RAII (Resource Acquisition Is Initialization)

BuildContextGuard uses RAII to manage thread-local storage:

```rust
pub struct BuildContextGuard {
    // Stores pointer to thread-local BuildContext
}

impl BuildContextGuard {
    pub fn new(ctx: &BuildContext) -> Self {
        // Set thread-local storage
        Self { /* ... */ }
    }
}

impl Drop for BuildContextGuard {
    fn drop(&mut self) {
        // Clear thread-local storage
    }
}
```

**Benefit:** Automatic cleanup, no memory leaks, exception-safe.

### 2. Closure-Based Scoping

`with_build_context()` uses closures to enforce correct guard lifetime:

```rust
pub fn with_build_context<F, R>(context: &BuildContext, f: F) -> R
where
    F: FnOnce() -> R,
{
    let _guard = BuildContextGuard::new(context);
    f()  // ← Borrow checker ensures guard outlives this call
}
```

**Benefit:** Compiler-enforced correctness, no runtime overhead.

### 3. Pipeline Coordination Pattern

PipelineOwner coordinates three phases with clear separation:

```rust
impl PipelineOwner {
    // Phase 1: Build dirty components
    pub fn build_frame(&mut self) { /* ... */ }

    // Phase 2: Layout with constraints
    pub fn layout_frame(&mut self, constraints: BoxConstraints) -> Size { /* ... */ }

    // Phase 3: Paint to layers
    pub fn paint_frame(&mut self) -> Scene { /* ... */ }
}
```

**Benefit:** Clear separation of concerns, easy to reason about.

### 4. Request/Flush Pattern

Changes are requested first, then flushed in batch:

```rust
// Request phase (can happen anytime)
pipeline.request_layout(element_id);  // ← Just marks dirty

// Flush phase (happens during frame rendering)
pipeline.flush_layout(constraints);  // ← Processes all dirty elements
```

**Benefit:** Batching reduces redundant work, improves performance.

## Trade-offs and Alternatives

### Alternative 1: Pass BuildContext Explicitly

**Approach:** Change `IntoElement` to take BuildContext as parameter:

```rust
trait IntoElement {
    fn into_element(self, ctx: &BuildContext) -> Element;
}
```

**Pros:**
- No thread-local storage needed
- No RAII guard complexity
- More explicit control flow

**Cons:**
- ❌ Breaking change to all View implementations
- ❌ Verbose (every widget must thread BuildContext through)
- ❌ Doesn't match Flutter's ergonomics

**Decision:** Rejected. Thread-local approach is more ergonomic.

### Alternative 2: Auto-request Layout in set_root()

**Approach:** Make `set_root()` automatically call `request_layout()`:

```rust
pub fn set_root(&mut self, element: Element) -> ElementId {
    let root_id = self.tree.write().set_root(element);
    self.request_layout(root_id);  // ← Automatic
    root_id
}
```

**Pros:**
- Can't forget to request layout
- Simpler attach() implementation

**Cons:**
- ❌ Less flexible (what if we want to set root without layout?)
- ❌ Hides an important step
- ❌ Doesn't match Flutter's explicit model

**Decision:** Rejected. Explicit is better than implicit for critical initialization steps.

### Alternative 3: Resize Listener Pattern

**Approach:** Use a callback/listener pattern for resize events:

```rust
pipeline.on_resize(|new_size| {
    // Automatically request layout
});
```

**Pros:**
- Decouples resize handling from event loop
- Could support multiple listeners

**Cons:**
- ❌ More complexity
- ❌ Overkill for current needs
- ❌ Extra indirection

**Decision:** Rejected. Direct call is simpler and sufficient.

## Future Improvements

### 1. Layout Request Batching During Resize

**Problem:** Rapid resize events (e.g., dragging window edge) trigger many layout requests.

**Solution:** Debounce resize events or batch layout requests within a frame:

```rust
// Potential optimization
if self.resize_debounce_timer.elapsed() < Duration::from_millis(16) {
    return;  // Skip this resize, wait for next frame
}
```

### 2. Attachment Lifecycle Tests

**Problem:** These bugs weren't caught by tests because we lack integration tests for the attachment lifecycle.

**Solution:** Add tests to prevent regressions:

```rust
#[test]
fn test_attach_triggers_layout() {
    let pipeline = PipelineOwner::new();
    pipeline.attach(TestWidget);

    // Verify layout was requested
    assert!(pipeline.has_dirty_layout());
}

#[test]
fn test_nested_views_access_build_context() {
    let pipeline = PipelineOwner::new();

    // Should not panic
    pipeline.attach(NestedWidget {
        child: InnerWidget,
    });
}
```

### 3. API Documentation

**Problem:** The attach() → layout → paint sequence isn't documented in the API.

**Solution:** Add comprehensive doc comments:

```rust
/// Attach a root widget to the pipeline.
///
/// # Lifecycle
///
/// 1. Creates BuildContext for initial build
/// 2. Converts View → Element tree
/// 3. Sets element as pipeline root
/// 4. **Requests initial layout** (critical!)
///
/// After attach(), call `layout_frame()` and `paint_frame()` to render.
```

## Lessons Learned

1. **RAII guards require careful lifetime management** - Block scopes can drop guards too early
2. **Git history is invaluable during refactoring** - Old working code shows what was lost
3. **User feedback accelerates debugging** - "Look at git history" hint was crucial
4. **Separation of concerns must preserve behavior** - Clean architecture doesn't mean dropping steps
5. **Thread-local storage needs RAII discipline** - Easy to get wrong without compiler help
6. **Flutter's patterns translate well to Rust** - But require Rust idioms (RAII, Arc, etc.)

## References

- Flutter RenderView.scheduleInitialLayout(): https://api.flutter.dev/flutter/rendering/RenderView/scheduleInitialLayout.html
- Flutter BuildContext: https://api.flutter.dev/flutter/widgets/BuildContext-class.html
- Rust RAII pattern: https://doc.rust-lang.org/rust-by-example/scope/raii.html
- Historical code: commit ed498f9 `FluiApp::build_root()`
