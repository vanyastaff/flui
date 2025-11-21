# FLUI Pipeline & Binding Architecture - Quick Reference

**For quick lookups during development and refactoring**

---

## File Locations

### Core Pipeline Files
```
crates/flui_core/src/pipeline/
├── pipeline_owner.rs          ← Main facade (PipelineOwner)
├── frame_coordinator.rs       ← Phase orchestration (BUILD/LAYOUT/PAINT)
├── build_pipeline.rs          ← Widget rebuild phase
├── layout_pipeline.rs         ← Size computation phase
├── paint_pipeline.rs          ← Layer generation phase
├── rebuild_queue.rs           ← Signal → rebuild scheduling (CRITICAL)
├── dirty_tracking.rs          ← Lock-free dirty sets
├── pipeline_trait.rs          ← Abstract Pipeline trait
└── pipeline_builder.rs        ← Builder pattern for PipelineOwner
```

### Binding Files
```
crates/flui_app/src/binding/
├── app_binding.rs             ← Main singleton (orchestration)
├── pipeline.rs                ← Widget lifecycle (attach)
├── scheduler.rs               ← Frame scheduling wrapper
├── renderer.rs                ← Rendering coordination
├── gesture.rs                 ← Event routing
└── base.rs                    ← BindingBase trait
```

### Element Tree Files
```
crates/flui_core/src/element/
├── element_tree.rs            ← Slab-based element storage
├── element.rs                 ← Element enum (Component/Render/Provider)
├── component_element.rs       ← Component wrapper
├── render_element.rs          ← Render wrapper with RenderState
└── provider_element.rs        ← Dependency provider
```

### Render State Files
```
crates/flui_core/src/render/
├── render_state.rs            ← Layout/paint flags (CRITICAL)
├── render_element.rs          ← RenderElement wrapper
└── render_box.rs              ← RenderBox trait implementations
```

---

## Critical Code Paths

### Path 1: Signal Change → Rebuild

```rust
// User code
signal.set(new_value)
    ↓
// signal.rs (in flui_core/src/hooks/)
Signal::set()
    └─ rebuild_queue.push(element_id, depth)

// Next frame
// app_binding.rs line ~75
scheduler.add_persistent_frame_callback(Arc::new(move |_timing| {
    pipeline_owner.write().flush_rebuild_queue();  // ← FIRST FLUSH
}));

// frame_coordinator.rs line ~140
pub fn build_frame(...) {
    loop {
        // ISSUE: flush_rebuild_queue() called AGAIN here (line ~143)
        self.build.flush_rebuild_queue();  // ← SECOND FLUSH (REDUNDANT!)
        
        self.build.flush_batch();
        if self.build.dirty_count() == 0 { break; }
        self.build.rebuild_dirty_parallel(tree);
    }
}
```

### Path 2: Window Resize → Layout

```rust
// Window event
WindowEvent::Resized(new_size)
    ↓
// embedder (desktop.rs or android.rs) - NOT IMPLEMENTED YET
handle_event(WindowEvent::Resized)
    └─ pipeline.request_layout(root_id)

// pipeline_owner.rs line ~509
pub fn request_layout(&mut self, node_id: ElementId) {
    // Mark in dirty set
    self.coordinator.layout_mut().mark_dirty(node_id);
    
    // Also set flag in RenderState
    let tree = self.tree.read();
    if let Some(Element::Render(render_elem)) = tree.get(node_id) {
        let render_state = render_elem.render_state().write();
        render_state.mark_needs_layout();  // ← TWO PLACES
        render_state.clear_constraints();
    }
}
```

### Path 3: Frame Rendering (INCOMPLETE)

```rust
// WgpuEmbedder (missing this loop!)
// Current: RedrawRequested → render_frame()
// Ideal: Should call scheduler lifecycle methods

fn render_frame(&mut self) -> Result<(), RenderError> {
    // MISSING: scheduler.begin_frame()
    
    let constraints = BoxConstraints::tight(self.window_size);
    let scene = self.binding.renderer.draw_frame(constraints);
    
    // Render and present...
    
    // MISSING: scheduler.end_frame()
}
```

---

## Key Data Structures

### RebuildQueue
```rust
pub struct RebuildQueue {
    inner: Arc<Mutex<HashSet<(ElementId, usize)>>>,
}

// Location: flui_core/src/pipeline/rebuild_queue.rs
// Thread-safe: Yes (Arc<Mutex<>>)
// Used by: Signals (any thread), BuildPipeline (frame start)
// Lifespan: Shared across entire app lifetime
```

### RenderState
```rust
pub struct RenderState {
    // Computed layout
    size: Option<Size>,
    offset: Offset,
    constraints: Option<BoxConstraints>,
    
    // Dirty flags
    needs_layout: AtomicBool,
    needs_paint: AtomicBool,
    
    // Cached layer (optional)
    // cached_layer: Option<Layer>,
}

// Location: flui_core/src/render/render_state.rs
// Thread-safe: Yes (atomic flags + RwLock wrapper)
// Accessed by: Layout phase, Paint phase, hot path (read-heavy)
```

### BuildPipeline
```rust
pub struct BuildPipeline {
    dirty_elements: Vec<(ElementId, usize)>,  // Sorted by depth
    build_count: usize,
    in_build_scope: bool,
    build_locked: bool,
    batcher: Option<BuildBatcher>,
    rebuild_queue: RebuildQueue,
}

// Location: flui_core/src/pipeline/build_pipeline.rs
// Thread-safe: No (wrapped in FrameCoordinator, which is in PipelineOwner)
// Responsibility: Track dirty components, schedule rebuilds
// Key method: rebuild_dirty_parallel()
```

### FrameCoordinator
```rust
pub struct FrameCoordinator {
    build: BuildPipeline,
    layout: LayoutPipeline,
    paint: PaintPipeline,
    budget: Arc<Mutex<FrameBudget>>,
}

// Location: flui_core/src/pipeline/frame_coordinator.rs
// Responsibility: Orchestrate three phases (BUILD → LAYOUT → PAINT)
// Key method: build_frame() - executes all phases atomically
```

### PipelineOwner
```rust
pub struct PipelineOwner {
    tree: Arc<RwLock<ElementTree>>,
    coordinator: FrameCoordinator,
    root_mgr: RootManager,
    rebuild_queue: RebuildQueue,
    on_build_scheduled: Option<Box<dyn Fn() + Send + Sync>>,
    frame_counter: u64,
    features: PipelineFeatures,
}

// Location: flui_core/src/pipeline/pipeline_owner.rs
// Responsibility: Facade over pipeline, delegates to components
// Key methods: build_frame(), attach(), request_layout()
// Thread-safe: Yes (tree is Arc<RwLock<>>, rebuild_queue is Arc<Mutex<>>)
```

---

## Critical Patterns

### Pattern 1: Component Rebuild (Three-Stage Locking)

```rust
// Good pattern: Minimize lock time during expensive operation

fn rebuild_component(tree: Arc<RwLock<ElementTree>>, element_id: ElementId) {
    // STAGE 1: Read phase (write lock - VERY SHORT)
    let (old_child, hook_ctx) = {
        let mut tree_guard = tree.write();  // Critical section START
        let component = tree_guard.get_mut(element_id)?;
        
        if !component.is_dirty() { return false; }
        
        (component.child(), extract_hook_context(component))
    };  // Critical section END - Release write lock
    
    // STAGE 2: Build phase (NO LOCK - can be expensive)
    let new_element = {
        // View::build() may take milliseconds
        // Multiple signal changes during build is OK
        // They'll be scheduled in rebuild_queue for next frame
        view.build(ctx)
    };
    
    // STAGE 3: Reconcile phase (write lock - VERY SHORT)
    {
        let mut tree_guard = tree.write();  // Critical section START
        reconcile_child(tree_guard, element_id, old_child, new_element);
        tree_guard.get_mut(element_id)?.clear_dirty();
    }  // Critical section END
}
```

### Pattern 2: Layout Marking (BROKEN - NEEDS FIX)

```rust
// Current code (BROKEN - two places):
pub fn request_layout(&mut self, node_id: ElementId) {
    // Place 1: Dirty set
    self.coordinator.layout_mut().mark_dirty(node_id);
    
    // Place 2: RenderState flag
    let tree = self.tree.read();
    if let Some(Element::Render(render_elem)) = tree.get(node_id) {
        let render_state = render_elem.render_state().write();
        render_state.mark_needs_layout();  // ← ALSO HERE
        render_state.clear_constraints();
    }
}

// Refactored code (CORRECT - one place):
fn mark_layout_dirty(&mut self, node_id: ElementId) {
    // Dirty set (for phase to scan)
    self.coordinator.layout_mut().mark_dirty(node_id);
    
    // RenderState flag (for scanning during build_frame)
    let tree = self.tree.read();
    if let Some(Element::Render(render_elem)) = tree.get(node_id) {
        let render_state = render_elem.render_state().write();
        render_state.mark_needs_layout();
        render_state.clear_constraints();
    }
}

pub fn request_layout(&mut self, node_id: ElementId) {
    self.mark_layout_dirty(node_id);  // ← Single call
}
```

### Pattern 3: Deduplication (RebuildQueue)

```rust
// Signal changes multiple times:
signal.set(1);
signal.set(2);
signal.set(3);

// RebuildQueue deduplicates automatically:
rebuild_queue.push(elem_id, depth);  // Insert 1
rebuild_queue.push(elem_id, depth);  // Ignored (already in HashSet)
rebuild_queue.push(elem_id, depth);  // Ignored (already in HashSet)

rebuild_queue.len() == 1  // ✓ Saved 2 rebuilds!
```

---

## Common Mistakes

### Mistake 1: Only Setting One Flag
```rust
// ❌ WRONG: Only setting dirty set
self.coordinator.layout_mut().mark_dirty(node_id);

// ❌ WRONG: Only setting RenderState flag
render_elem.render_state().write().mark_needs_layout();

// ✅ CORRECT: Setting both
self.request_layout(node_id);  // Calls both internally
```

### Mistake 2: Calling flush_rebuild_queue() Multiple Times
```rust
// ❌ WRONG: Called in scheduler callback AND in build_frame loop
// app_binding.rs
scheduler.add_persistent_frame_callback(move |_| {
    pipeline_owner.write().flush_rebuild_queue();  // First time
});

// frame_coordinator.rs
pub fn build_frame(...) {
    loop {
        self.build.flush_rebuild_queue();  // Second time (redundant!)
        // ...
    }
}

// ✅ CORRECT: Called only once at frame start via scheduler
```

### Mistake 3: Forgetting to Mark for Layout After Build
```rust
// ❌ WRONG: Component rebuilt but layout not marked
fn attach<V>(&mut self, widget: V) {
    let root_id = self.set_root(element);
    self.coordinator.build_mut().schedule(root_id, 0);
    // Missing: request_layout(root_id)!
    root_id
}

// ✅ CORRECT: Both build and layout scheduled
fn attach<V>(&mut self, widget: V) {
    let root_id = self.set_root(element);
    self.schedule_build_for(root_id, 0);
    self.request_layout(root_id);  // ← Also mark for layout
    root_id
}
```

### Mistake 4: Holding Lock During Expensive Operation
```rust
// ❌ WRONG: Lock held while building (can be slow)
let mut tree_guard = tree.write();
let new_element = view.build(ctx);  // View::build() might be slow!
// Still holding lock...

// ✅ CORRECT: Lock only while accessing tree
let (old_child, hook_ctx) = {
    let mut tree_guard = tree.write();
    (component.child(), extract_hook_context(component))
};

// Release lock before expensive operation
let new_element = view.build(ctx);

// Re-acquire lock only for reconcile
let mut tree_guard = tree.write();
reconcile_child(tree_guard, element_id, old_child, new_element);
```

---

## Dirty Tracking Quick Reference

### BuildPipeline Dirty Set
```
What: Components waiting to rebuild
Type: Vec<(ElementId, usize)>  [not a set, allows duplicates]
When: Signals change, or schedule_build_for() called
How: Sorted by depth, then deduplicated in rebuild_dirty()
Flushed: BuildPipeline::flush_rebuild_queue() at frame start

Issues:
- Allows duplicates (deduped during rebuild)
- flush_rebuild_queue() called twice per frame (redundant!)
```

### LayoutPipeline Dirty Set
```
What: RenderElements waiting for layout
Type: LockFreeDirtySet  [lock-free atomic, thread-safe]
When: RenderState.needs_layout flag set
How: Scanned from RenderState flags at start of layout phase
Flushed: Drain all marked elements
```

### PaintPipeline Dirty Set
```
What: RenderElements waiting for paint
Type: LockFreeDirtySet  [lock-free atomic, thread-safe]
When: Mark happens after layout completes
How: Drain all marked elements
Flushed: Drain all marked elements
```

### RenderState Flags
```
needs_layout: AtomicBool
  └─ Set when: Element created, window resized, parent layout changes
  └─ Read by: build_frame() scanning phase
  └─ Cleared when: Layout completes

needs_paint: AtomicBool
  └─ Set when: Layout marks for paint, or request_paint() called
  └─ Read by: Paint phase
  └─ Cleared when: Paint completes
```

---

## Testing Checklist

### Unit Tests
- [ ] RebuildQueue deduplicates
- [ ] request_layout() sets both dirty set AND flag
- [ ] Component rebuild clears dirty flag
- [ ] Layout marks for paint
- [ ] Paint clears needs_paint flag

### Integration Tests
- [ ] Signal change triggers rebuild
- [ ] Window resize triggers layout
- [ ] Full frame cycle completes
- [ ] No deadlocks under concurrent signal changes

### Performance Tests
- [ ] Frame time under 16.67ms @ 60fps
- [ ] Signal changes don't cause frame stalls
- [ ] No memory leaks (profile with valgrind)
- [ ] Lock contention minimal (profile with flamegraph)

---

## When to Use What

### Use PipelineOwner::request_layout() when:
- Window resize event
- Theme changes (requires relayout)
- Orientation changes
- External widget moved/resized

### Use signal.set() when:
- User input (button click, text change)
- Animation state updates
- Any reactive data changes

### Use schedule_build_for() when:
- Provider needs to rebuild dependents
- Hot reload (reassemble_tree)

### Use attach() when:
- App startup (flui_app::run_app)
- Replacing root widget (tear down old first)

---

## Performance Tips

1. **Minimize lock time**: Don't do expensive work while holding locks
2. **Batch updates**: Multiple signal changes in same frame = 1 rebuild
3. **Enable hit test cache**: ~5-15% CPU savings during mouse movement
4. **Use parallel build**: Enables if `parallel` feature enabled and tree large enough
5. **Profile with tracing**: Use `RUST_LOG=debug` to see phase timings

---

## Debug Commands

```bash
# Run with debug logging
RUST_LOG=debug cargo run --example counter

# Run with trace logging (very verbose)
RUST_LOG=trace cargo run --example counter

# Run with specific filters
RUST_LOG=flui_core=debug,flui_app=info cargo run

# Run with filtering (warnings only)
RUST_LOG=warn cargo run

# Run tests with output
cargo test --lib -- --nocapture

# Profile with flamegraph
cargo install flamegraph
cargo flamegraph --example counter
```

---

## References

- Main architecture doc: `PIPELINE_AND_BINDING_ARCHITECTURE.md`
- Diagrams: `ARCHITECTURE_DIAGRAMS.md`
- Hook rules: `crates/flui_core/src/hooks/RULES.md`
- Render guide: `crates/flui_rendering/RENDER_OBJECT_GUIDE.md`
- Build docs: `crates/flui_core/src/pipeline/mod.rs`
