# FLUI Pipeline and Binding Architecture Analysis

**Date**: 2025-11-21  
**Status**: Comprehensive architecture review with identified issues and refactoring recommendations

---

## Executive Summary

The FLUI pipeline and binding system has a **good foundational architecture** but suffers from several **integration inconsistencies** and **architectural gaps** that need refactoring before the system can scale reliably. The main issues are:

1. **RebuildQueue → BuildPipeline Integration** is inconsistent
2. **Missing attach() lifecycle documentation** and error handling
3. **Layout marking inconsistency** between internal and external requests
4. **RenderState flag synchronization issues** 
5. **Binding abstraction gaps** - some layers expose implementation details
6. **Frame flow coordination** needs clarification and safer patterns

---

## Current Architecture

### High-Level Frame Flow

```
User calls run_app(root_widget)
    ↓
AppBinding::ensure_initialized()
    ├─ GestureBinding (event routing)
    ├─ SchedulerBinding (wraps flui-scheduler, 60 FPS)
    ├─ RendererBinding (render coordination)
    └─ PipelineBinding (widget tree lifecycle)
    ↓
WgpuEmbedder::render_frame() [per winit RedrawRequested]
    ↓
Scheduler.begin_frame()
    ├─ [PERSISTENT CALLBACKS] 
    │   └─ flush_rebuild_queue() ← Signal-driven rebuilds
    └─ [ONE-TIME CALLBACKS] (animations, timers, etc.)
    ↓
RendererBinding.draw_frame(constraints)
    ↓
Pipeline.build_frame(constraints)  ← Main entry point
    ├─ Phase 1: BUILD
    │   ├─ flush_rebuild_queue() (again? inconsistent!)
    │   ├─ flush_batch()
    │   └─ rebuild_dirty_parallel()
    ├─ Phase 2: LAYOUT
    │   ├─ Scan needs_layout flags
    │   ├─ compute_layout()
    │   └─ Mark for paint
    └─ Phase 3: PAINT
        ├─ generate_layers()
        └─ Return CanvasLayer
    ↓
GPU Render + Present
    ↓
Scheduler.end_frame()
```

### Three-Tree Architecture

```
┌─────────────────────────────────────────────────────┐
│ PipelineOwner (Facade)                              │
├─────────────────────────────────────────────────────┤
│                                                       │
│  tree: Arc<RwLock<ElementTree>>                      │
│    └─ Slab<Element>                                  │
│         ├─ ComponentElement (dirty-trackable)        │
│         ├─ RenderElement (layout/paint-able)         │
│         └─ ProviderElement (dependency tracking)     │
│                                                       │
│  coordinator: FrameCoordinator                       │
│    ├─ build: BuildPipeline                          │
│    │   ├─ dirty_elements: Vec<(ElementId, depth)>   │
│    │   ├─ batcher: Optional BuildBatcher            │
│    │   └─ rebuild_queue: RebuildQueue               │
│    ├─ layout: LayoutPipeline                        │
│    │   └─ dirty: LockFreeDirtySet                   │
│    └─ paint: PaintPipeline                          │
│        └─ dirty: LockFreeDirtySet                   │
│                                                       │
│  root_mgr: RootManager (tracks root_id)             │
│  rebuild_queue: RebuildQueue (signals → rebuilds)   │
│  features: PipelineFeatures (optional production)   │
│                                                       │
└─────────────────────────────────────────────────────┘
```

### Binding System

```
AppBinding (Singleton)
├─ GestureBinding
│   └─ EventRouter (dispatches window/pointer events)
├─ SchedulerBinding
│   ├─ wraps flui-scheduler::Scheduler
│   ├─ target_fps: 60
│   └─ Persistent frame callbacks
├─ RendererBinding
│   ├─ pipeline: Arc<dyn Pipeline>
│   └─ draw_frame(constraints) → Scene
└─ PipelineBinding
    └─ pipeline_owner: Arc<RwLock<PipelineOwner>>
        └─ attach_root_widget<V: View>()
```

---

## Issues & Architectural Problems

### 1. **CRITICAL: RebuildQueue Flushed Twice Per Frame**

**Location**: `pipeline_owner.rs:build_frame()` and `app_binding.rs:wire_up()`

**Problem**:
```rust
// In AppBinding::wire_up()
scheduler.add_persistent_frame_callback(Arc::new(move |_timing| {
    pipeline_owner.write().flush_rebuild_queue();  // FLUSH #1
}));

// In PipelineOwner::build_frame()
loop {
    self.build.flush_rebuild_queue();  // FLUSH #2 (redundant!)
    self.build.flush_batch();
    
    let build_count = self.build.dirty_count();
    if build_count == 0 { break; }
    self.build.rebuild_dirty_parallel(tree);
}
```

**Impact**:
- First flush finds pending rebuilds → adds to dirty_elements
- Second flush in loop finds empty queue → no-op
- Works but violates "single responsibility" - confusing flow
- Makes it unclear when rebuild_queue is actually processed

**Recommendation**:
Remove the `flush_rebuild_queue()` call from the loop. It should only happen ONCE at frame start via scheduler callback.

```rust
// ✅ CORRECT: Flushed once at frame start
pub fn build_frame(...) -> Result<...> {
    // RebuildQueue is already flushed by scheduler callback
    // (before build_frame is called)
    
    loop {
        // REMOVE flush_rebuild_queue() from here
        self.build.flush_batch();
        
        let build_count = self.build.dirty_count();
        if build_count == 0 { break; }
        self.build.rebuild_dirty_parallel(tree);
    }
}
```

---

### 2. **Layout Marking Inconsistency**

**Location**: `pipeline_owner.rs:request_layout()` vs `attach()`

**Problem**:
```rust
// In request_layout() - MANUAL MARKING (two places)
pub fn request_layout(&mut self, node_id: ElementId) {
    // Mark in dirty set
    self.coordinator.layout_mut().mark_dirty(node_id);
    
    // Also set needs_layout flag in RenderState
    let tree = self.tree.read();
    if let Some(crate::element::Element::Render(render_elem)) = tree.get(node_id) {
        let render_state = render_elem.render_state().write();
        render_state.mark_needs_layout();
        render_state.clear_constraints();  // Clear cached constraints
    }
}

// In attach() - AUTOMATIC MARKING (framework does it)
pub fn attach<V>(&mut self, widget: V) -> ElementId {
    let root_id = self.set_root(element);
    
    // Framework automatically requests layout for root
    self.request_layout(root_id);  // ✅ Correct
    
    root_id
}

// But in layout_pipeline.rs - SCANNING FOR FLAGS
pub fn build_frame(...) {
    // Phase 2: Layout
    let mut tree_guard = tree.write();
    
    // Scan for RenderElements with needs_layout flag and add to dirty set
    let all_ids: Vec<_> = tree_guard.all_element_ids().collect();
    for id in all_ids {
        if let Some(crate::element::Element::Render(render_elem)) = tree_guard.get(id) {
            let render_state = render_elem.render_state().read();
            if render_state.needs_layout() {  // ← SCANNING for flag
                self.layout.mark_dirty(id);
            }
        }
    }
}
```

**Impact**:
- External requests must manually set BOTH dirty set AND RenderState flag
- Internal framework code scans for flags and marks dirty set
- **Two different code paths** for the same semantic operation
- Confusing for developers: "Do I call request_layout or mark the flag directly?"
- Easy to miss one step and leave layout incomplete

**Root Cause**:
The RenderState `needs_layout()` flag should be the SINGLE source of truth. The dirty set should be a read-cache of that flag, not a separate authority.

**Recommendation**:
Create a dedicated method that encapsulates the pattern:

```rust
// ✅ CORRECT: Single method, both flags set atomically
fn mark_layout_dirty(&mut self, node_id: ElementId) {
    // Mark in dirty set (for coordinator)
    self.coordinator.layout_mut().mark_dirty(node_id);
    
    // Mark in RenderState (for scanning)
    let tree = self.tree.read();
    if let Some(Element::Render(render_elem)) = tree.get(node_id) {
        let render_state = render_elem.render_state().write();
        render_state.mark_needs_layout();
        render_state.clear_constraints();
    }
}

// External API (what users call)
pub fn request_layout(&mut self, node_id: ElementId) {
    self.mark_layout_dirty(node_id);
}

// Framework rebuilds also call this
pub fn attach<V>(&mut self, widget: V) -> ElementId {
    let root_id = self.set_root(element);
    self.mark_layout_dirty(root_id);  // ✅ Single call
    root_id
}
```

---

### 3. **Inconsistent Handling of Component Rebuild Results**

**Location**: `build_pipeline.rs:rebuild_component()` and `build_pipeline.rs:rebuild_provider()`

**Problem**:
```rust
// rebuild_component() marks dirty, rebuilds, clears dirty flag
fn rebuild_component(&mut self, tree: ..., element_id: ElementId, ...) -> bool {
    let (old_child_id, hook_context) = {
        let mut tree_guard = tree.write();
        // ...
        if !component.is_dirty() {
            return false;  // Skip if not dirty
        }
        // ...
    };
    
    // Build and reconcile...
    
    {
        let mut tree_guard = tree.write();
        // Clear dirty flag AFTER rebuild
        if let Some(element) = tree_guard.get_mut(element_id) {
            if let Some(component) = element.as_component_mut() {
                component.clear_dirty();  // ✅ Cleared
            }
        }
    }
    true
}

// rebuild_provider() also marks itself clean
fn rebuild_provider(&mut self, tree: ..., element_id: ElementId, ...) -> bool {
    let dependents = {
        let mut tree_guard = tree.write();
        // ...
        if !provider.is_dirty() {
            return false;
        }
        
        let deps = provider.dependents().iter().copied().collect();
        provider.clear_dirty();  // ✅ Cleared
        deps
    };
    
    // Schedule dependents for rebuild...
    true
}

// RenderElement rebuild in loop
for (element_id, depth) in dirty.drain(..) {
    match element_type {
        Some(ElementType::Component) => {
            // ... rebuilds and clears dirty
        }
        Some(ElementType::Render) => {
            // RenderElements don't rebuild - they only relayout
            // NO dirty flag clearing!  ← INCONSISTENT
        }
        Some(ElementType::Provider) => {
            // ... rebuilds and clears dirty
        }
    }
}
```

**Impact**:
- ComponentElement and ProviderElement explicitly clear dirty flag after rebuild
- RenderElement loop just skips ("don't rebuild") but never had dirty flag in first place
- When layout_pipeline scans for `needs_layout()` flag, it finds RenderElements
- Two different mechanisms: explicit flag + dirty set for Components/Providers, but only flag for Renders
- Architectural confusion: "Is the dirty set redundant with the flag?"

**Recommendation**:
RenderElements should NEVER be in the build dirty set. Create separate lists:

```rust
// In rebuild_dirty_parallel
for (element_id, depth) in dirty.drain(..) {
    let element_type = {
        let tree_guard = tree.read();
        // Determine type
    };
    
    match element_type {
        Some(ElementType::Component) => {
            if self.rebuild_component(tree, element_id, depth) {
                rebuilt_count += 1;
            }
        }
        Some(ElementType::Provider) => {
            if self.rebuild_provider(tree, element_id, depth) {
                rebuilt_count += 1;
            }
        }
        Some(ElementType::Render) => {
            // ERROR: RenderElement in build dirty set!
            // This should never happen
            tracing::error!(
                element_id = ?element_id,
                "RenderElement in build dirty set (should be in layout dirty set only)"
            );
            // Either skip or handle appropriately
        }
        None => {
            tracing::warn!(?element_id, "Element type is None - skipping");
        }
    }
}
```

---

### 4. **Missing Validation in attach()**

**Location**: `pipeline_owner.rs:attach()`

**Problem**:
```rust
pub fn attach<V>(&mut self, widget: V) -> ElementId
where
    V: crate::view::View + Clone + Send + Sync + 'static,
{
    // Check if root already exists
    if self.root_element_id().is_some() {
        panic!(
            "Root widget already attached to PipelineOwner!\n\
            \n\
            Only one root widget is supported at a time.\n\
            \n\
            If you need to replace the root widget, call remove_root() first."
        );
    }
    
    // Create ComponentElement wrapper
    let view_clone = widget.clone();
    let builder: crate::view::BuildFn = Box::new(move || {
        // ← BuildFn is called during rebuild
        let view = view_clone.clone();
        let element = view.into_element();
        element
    });
    
    let mut component = ComponentElement::new(builder);
    component.mark_dirty();  // Mark for initial build
    
    let element = Element::Component(component);
    let root_id = self.set_root(element);
    
    // Schedule initial build
    self.coordinator.build_mut().schedule(root_id, 0);  // ← Second mark?
    
    // Request layout for entire tree
    self.request_layout(root_id);  // ✅ Correct
    
    root_id
}
```

**Issues**:
1. Component is marked dirty explicitly (`component.mark_dirty()`)
2. Then scheduled again (`self.coordinator.build_mut().schedule(root_id, 0)`)
3. Creates redundant dirty marking
4. **Missing**: No error handling if View::build panics during initial build
5. **Missing**: No cleanup if set_root fails
6. **Missing**: Documentation of what happens if widget.clone() or into_element panics

**Recommendation**:
Simplify the attach logic and add error handling:

```rust
pub fn attach<V>(&mut self, widget: V) -> Result<ElementId, AttachError>
where
    V: crate::view::View + Clone + Send + Sync + 'static,
{
    // Validate preconditions
    if self.root_element_id().is_some() {
        return Err(AttachError::RootAlreadyAttached);
    }
    
    // Try to build the root widget (may panic if builder fails)
    // This validates that the widget can build before committing to tree
    let view_clone = widget.clone();
    let builder: crate::view::BuildFn = Box::new(move || {
        let view = view_clone.clone();
        view.into_element()
    });
    
    // Create and insert component
    let mut component = ComponentElement::new(builder);
    let element = Element::Component(component);
    let root_id = self.set_root(element)?;
    
    // Single place to mark for build (not double-marked)
    self.schedule_build_for(root_id, 0);
    
    // Request layout (separate concern - layout dirty set)
    self.mark_layout_dirty(root_id);
    
    Ok(root_id)
}

#[derive(Debug)]
pub enum AttachError {
    RootAlreadyAttached,
    ElementTreeFull,
    // ... other errors
}
```

---

### 5. **Binding Abstraction Gaps**

**Location**: `pipeline_binding.rs` vs actual usage

**Problem**:
```rust
// PipelineBinding is a thin wrapper:
pub struct PipelineBinding {
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
}

impl PipelineBinding {
    pub fn attach_root_widget<V>(&self, widget: V) {
        let mut pipeline = self.pipeline_owner.write();
        pipeline.attach(widget);
    }
    
    pub fn pipeline_owner(&self) -> Arc<RwLock<PipelineOwner>> {
        self.pipeline_owner.clone()
    }
}

// In app_binding.rs:
binding.pipeline.attach_root_widget(app);  // ✅ Correct usage

// But RendererBinding exposes Pipeline trait:
pub struct RendererBinding {
    pipeline: Arc<dyn Pipeline>,  // ← Abstract trait object
}

impl RendererBinding {
    pub fn draw_frame(&self, constraints: BoxConstraints) -> Scene {
        // Uses trait methods only
        match self.pipeline.build_frame(constraints) {
            Ok(layer_opt) => { /* ... */ }
            Err(e) => { /* ... */ }
        }
    }
    
    pub fn pipeline(&self) -> Arc<dyn Pipeline> {
        self.pipeline.clone()
    }
}

// But AppBinding wires them together:
let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));

let mut binding = Self {
    gesture: GestureBinding::new(),
    scheduler: SchedulerBinding::new(),
    renderer: RendererBinding::new(pipeline_owner.clone()),  // ← Takes Arc<RwLock<>>
    pipeline: PipelineBinding::new(pipeline_owner),          // ← Same Arc<RwLock<>>
};
```

**Issues**:
1. `RendererBinding::new()` accepts `Arc<RwLock<PipelineOwner>>` but signature says `P: Pipeline`
   - Actually constructs `Arc<P>` inside
   - Type confusion: `new()` takes Arc<RwLock<>>, but trait uses `Arc<dyn Pipeline>`
2. `PipelineBinding::pipeline_owner()` is just returning the Arc - why not make it public?
3. `RendererBinding::pipeline()` returns `Arc<dyn Pipeline>` - but users can't call `attach_root_widget` on it
4. No clear separation of concerns between PipelineBinding (for View attachment) and RendererBinding (for rendering)

**Recommendation**:
Clarify the abstraction layers:

```rust
// RendererBinding: RENDERING ONLY (not widget lifecycle)
pub struct RendererBinding {
    pipeline: Arc<dyn Pipeline>,
}

impl RendererBinding {
    pub fn new(pipeline: Arc<dyn Pipeline>) -> Self {
        Self { pipeline }
    }
    
    pub fn draw_frame(&self, constraints: BoxConstraints) -> Scene {
        // Render using pipeline trait
    }
}

// PipelineBinding: WIDGET LIFECYCLE (not rendering)
pub struct PipelineBinding {
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
}

impl PipelineBinding {
    pub fn new(pipeline_owner: Arc<RwLock<PipelineOwner>>) -> Self {
        Self { pipeline_owner }
    }
    
    pub fn attach_root_widget<V>(&self, widget: V) -> ElementId
    where
        V: View + Clone + Send + Sync + 'static,
    {
        let mut owner = self.pipeline_owner.write();
        owner.attach(widget).expect("Failed to attach root widget")
    }
    
    // For internal access (e.g., by scheduler callbacks)
    pub(crate) fn pipeline_owner(&self) -> Arc<RwLock<PipelineOwner>> {
        self.pipeline_owner.clone()
    }
}

// AppBinding: ORCHESTRATION
pub struct AppBinding {
    gesture: GestureBinding,
    scheduler: SchedulerBinding,
    renderer: RendererBinding,
    pipeline: PipelineBinding,
}

impl AppBinding {
    pub fn new() -> Self {
        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
        
        Self {
            gesture: GestureBinding::new(),
            scheduler: SchedulerBinding::new(),
            renderer: RendererBinding::new(pipeline_owner.clone()),  // ← Pass Arc<dyn Pipeline>
            pipeline: PipelineBinding::new(pipeline_owner),
        }
    }
}
```

---

### 6. **Frame Flow Ambiguity**

**Location**: Comments in `app_binding.rs:wire_up()` vs actual flow

**Current documentation**:
```rust
// Frame flow (integrated):
//   WgpuEmbedder::render_frame()
//     → scheduler.begin_frame()
//       → [PERSISTENT CALLBACKS] flush_rebuild_queue()
//       → [ONE-TIME CALLBACKS] animations, tickers, etc.
//     → renderer.draw_frame()
//       → pipeline.build_frame() [NO flush_rebuild_queue here anymore]
//         → build/layout/paint pipelines [respect frame budget]
//     → scheduler.end_frame() [record timing]
```

**Actual code doesn't exist yet**:
- No evidence that `WgpuEmbedder` or embedders actually call `scheduler.begin_frame()`
- No evidence that they call `scheduler.end_frame()`
- The scheduler callbacks are registered but we don't know if they're called

**Problem**:
This is the critical integration point that ties everything together, but it's:
1. Not implemented in embedders
2. Not documented in embedder code
3. Not tested
4. Could explain why frame timing isn't working

**Recommendation**:
Create a clear frame lifecycle:

```rust
// In WgpuEmbedder or similar
fn render_frame(&mut self) -> Result<(), RenderError> {
    let scheduler = self.binding.scheduler.scheduler();
    
    // BEGIN FRAME: Run scheduler callbacks (persistent + one-time)
    // This is where signals are flushed!
    scheduler.begin_frame()?;
    
    // RENDER FRAME: Build scene and render to GPU
    let constraints = BoxConstraints::tight(self.window_size);
    let scene = self.binding.renderer.draw_frame(constraints);
    
    // PRESENT: GPU render and vsync
    self.present_scene(&scene)?;
    
    // END FRAME: Record timing statistics
    scheduler.end_frame();
    
    Ok(())
}
```

---

## Summary of Architectural Issues

| Issue | Severity | Location | Impact |
|-------|----------|----------|--------|
| RebuildQueue flushed twice per frame | HIGH | `pipeline_owner.rs`, `app_binding.rs` | Confusing flow, potential double-processing |
| Layout marking inconsistency | HIGH | `request_layout()` vs `attach()` | Easy to miss setting both flags |
| Component rebuild redundant marking | MEDIUM | `build_pipeline.rs` | Code smell, but works |
| Missing validation in attach() | MEDIUM | `pipeline_owner.rs:attach()` | No error recovery |
| Binding abstraction gaps | MEDIUM | `*_binding.rs` | Confusing responsibilities |
| Frame flow not implemented | CRITICAL | Embedders | Core frame loop missing! |

---

## Recommended Refactoring Plan

### Phase 1: Clarify Frame Flow (CRITICAL)

**Implement proper frame loop in embedders:**

```rust
// Step 1: Implement in WgpuEmbedder
fn render_frame(&mut self) -> Result<(), RenderError> {
    // Guard: prevent re-entrance during frame
    if self.is_rendering {
        tracing::warn!("Frame rendering in progress, skipping frame");
        return Ok(());
    }
    self.is_rendering = true;
    
    let _guard = profiling::puffin_scope!("render_frame");
    
    // Phase 1: FRAME START (scheduler callbacks, signal flushing)
    {
        let scheduler = self.binding.scheduler.scheduler();
        scheduler.begin_frame()
            .map_err(|e| RenderError::SchedulerError(e))?;
    }
    
    // Phase 2: PIPELINE (build/layout/paint)
    let constraints = BoxConstraints::tight(self.window_size);
    let scene = self.binding.renderer.draw_frame(constraints);
    
    // Phase 3: GPU RENDER
    self.gpu_renderer.render(&scene)?;
    
    // Phase 4: FRAME END (timing statistics)
    {
        let scheduler = self.binding.scheduler.scheduler();
        scheduler.end_frame();
    }
    
    self.is_rendering = false;
    Ok(())
}
```

### Phase 2: Consolidate Layout Marking

**Create single point of truth:**

```rust
impl PipelineOwner {
    /// Mark a RenderElement as needing layout (both dirty set AND flag)
    pub(crate) fn mark_layout_dirty(&mut self, node_id: ElementId) {
        // Mark in dirty set (for layout phase to scan)
        self.coordinator.layout_mut().mark_dirty(node_id);
        
        // Mark in RenderState flag (for scanning during build_frame)
        let tree = self.tree.read();
        if let Some(Element::Render(render_elem)) = tree.get(node_id) {
            let render_state = render_elem.render_state().write();
            render_state.mark_needs_layout();
            render_state.clear_constraints();
        }
    }
    
    /// Public API for external code
    pub fn request_layout(&mut self, node_id: ElementId) {
        self.mark_layout_dirty(node_id);
    }
}
```

### Phase 3: Fix RebuildQueue Flushing

**Remove duplicate flush:**

```rust
// In frame_coordinator.rs:build_frame()
pub fn build_frame(...) -> Result<...> {
    let frame_span = tracing::info_span!("frame", ?constraints);
    let _frame_guard = frame_span.enter();
    
    self.budget.lock().reset();
    
    // Phase 1: Build
    let mut iterations = 0;
    let mut total_build_count = 0;
    loop {
        // REMOVE: self.build.flush_rebuild_queue();
        // ↑ This is done by scheduler callback BEFORE build_frame()
        
        // Only flush batch (which is local to BuildPipeline)
        self.build.flush_batch();
        
        let build_count = self.build.dirty_count();
        if build_count == 0 {
            break;
        }
        
        let build_span = tracing::info_span!("build_iteration", iteration = iterations);
        let _build_guard = build_span.enter();
        
        self.build.rebuild_dirty_parallel(tree);
        total_build_count += build_count;
        
        iterations += 1;
        
        if iterations > 100 {
            tracing::warn!("Build loop exceeded 100 iterations");
            break;
        }
    }
    
    // Continue with layout and paint phases...
}
```

### Phase 4: Validate attach() Lifecycle

**Add proper error handling and documentation:**

```rust
impl PipelineOwner {
    pub fn attach<V>(&mut self, widget: V) -> Result<ElementId, AttachError>
    where
        V: View + Clone + Send + Sync + 'static,
    {
        if self.root_element_id().is_some() {
            return Err(AttachError::RootAlreadyAttached);
        }
        
        let view_clone = widget.clone();
        let builder: ViewBuildFn = Box::new(move || {
            view_clone.clone().into_element()
        });
        
        let component = ComponentElement::new(builder);
        let element = Element::Component(component);
        
        let root_id = self.set_root(element);
        
        // Single scheduling point (not double-marked)
        self.schedule_build_for(root_id, 0);
        
        // Layout dirty set (separate concern)
        self.mark_layout_dirty(root_id);
        
        tracing::info!(root_id = ?root_id, "Root widget attached");
        Ok(root_id)
    }
}
```

### Phase 5: Clarify Binding Responsibilities

**Document and enforce separation of concerns:**

```
GestureBinding
  └─ Responsibility: Platform events → EventRouter
  
SchedulerBinding
  └─ Responsibility: Frame scheduling, task queue, vsync
  
PipelineBinding
  └─ Responsibility: Widget lifecycle (attach, rebuild queue)
  └─ NOT: Rendering
  
RendererBinding
  └─ Responsibility: Frame rendering (build_frame → GPU)
  └─ NOT: Widget lifecycle
  
AppBinding
  └─ Responsibility: Orchestration and wiring
  └─ NOT: Implementation
```

---

## Testing Strategy

### Unit Tests (per component)

```rust
// pipeline_owner.rs tests
#[test]
fn test_rebuild_queue_flushed_once_per_frame() {
    let mut owner = PipelineOwner::new();
    let queue = owner.rebuild_queue();
    
    queue.push(ElementId::new(1), 0);
    assert_eq!(queue.len(), 1);
    
    // Flush once
    owner.flush_rebuild_queue();
    assert_eq!(queue.len(), 0);
    
    // Second flush should be no-op
    owner.flush_rebuild_queue();
    assert_eq!(queue.len(), 0);
}

#[test]
fn test_request_layout_sets_both_flags() {
    let mut owner = PipelineOwner::new();
    // Create a render element...
    let elem_id = /* setup */;
    
    owner.request_layout(elem_id);
    
    // Verify:
    // 1. Dirty set is marked
    assert!(owner.coordinator.layout().is_dirty(elem_id));
    
    // 2. RenderState flag is marked
    let tree = owner.tree().read();
    if let Some(Element::Render(elem)) = tree.get(elem_id) {
        let rs = elem.render_state().read();
        assert!(rs.needs_layout());
    }
}

#[test]
fn test_attach_requires_valid_widget() {
    let mut owner = PipelineOwner::new();
    
    struct PanicWidget;
    impl View for PanicWidget {
        fn build(self, _: &BuildContext) -> impl IntoElement {
            panic!("Widget builder panicked")
        }
    }
    
    // Should propagate panic or return error
    // (depending on final design)
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owner.attach(PanicWidget)
    }));
    
    assert!(result.is_err());
}
```

### Integration Tests (frame flow)

```rust
#[tokio::test]
async fn test_full_frame_cycle() {
    // Create binding (simulates app startup)
    let binding = AppBinding::ensure_initialized();
    
    // Attach widget
    binding.pipeline.attach_root_widget(TestWidget);
    
    // Simulate frame
    let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
    
    // This should:
    // 1. Flush rebuild queue
    // 2. Build dirty components
    // 3. Layout render objects
    // 4. Paint and return layer
    let scene = binding.renderer.draw_frame(constraints);
    
    assert!(scene.has_content());
}

#[test]
fn test_signal_triggers_rebuild() {
    let binding = AppBinding::ensure_initialized();
    
    struct Counter;
    impl View for Counter {
        fn build(self, ctx: &BuildContext) -> impl IntoElement {
            let count = use_signal(ctx, 0);
            // Widget with signal...
        }
    }
    
    binding.pipeline.attach_root_widget(Counter);
    
    // Signal change should schedule rebuild
    // ...verify rebuild_queue has pending items
}
```

---

## File Changes Required

### Core Files to Modify

1. **`crates/flui_core/src/pipeline/frame_coordinator.rs`**
   - Remove `flush_rebuild_queue()` from `build_frame()` loop

2. **`crates/flui_core/src/pipeline/pipeline_owner.rs`**
   - Add `mark_layout_dirty()` helper
   - Simplify `request_layout()`
   - Refactor `attach()` with error handling

3. **`crates/flui_core/src/pipeline/build_pipeline.rs`**
   - Add validation that ComponentElements/Providers only in build dirty set

4. **`crates/flui_app/src/binding/app_binding.rs`**
   - Update documentation and comments
   - Verify frame lifecycle wiring

5. **`crates/flui_app/src/embedder/desktop.rs` (or similar)**
   - Implement proper `begin_frame()` and `end_frame()` calls

6. **`crates/flui_app/src/binding/pipeline.rs`**
   - Update `attach_root_widget()` to handle Result

7. **`crates/flui_app/src/binding/renderer.rs`**
   - Clarify type signatures and responsibilities

---

## Conclusion

The FLUI architecture has **solid foundations** but needs **focused refactoring** to clarify integration points and fix the identified inconsistencies. The main goal should be:

1. **Single responsibility**: Each component has ONE clear job
2. **Clear data flow**: Rebuild queue → build → layout → paint
3. **No redundancy**: One signal processing path, not two
4. **Safe APIs**: Error handling and validation where needed
5. **Well documented**: Frame lifecycle clearly explained

The recommended refactoring can be done in 5 phases with minimal breaking changes to the public API.
