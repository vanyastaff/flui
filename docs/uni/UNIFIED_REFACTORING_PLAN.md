# FLUI Unified Refactoring Plan - Complete Architecture Overhaul

**Combining Pipeline Analysis + Bindings Analysis**  
**Date:** November 21, 2025  
**Version:** 1.0 - Unified Plan

---

## Executive Summary

This document unifies two separate architectural analyses:

1. **Pipeline Analysis** (from ANALYSIS_SUMMARY.txt)
   - 7 issues identified in pipeline layer
   - Focus: Frame lifecycle, rebuild queue, dirty tracking
   - Scope: `flui_core/src/pipeline/` (~2,500 lines)

2. **Bindings Analysis** (from WINIT_BINDINGS_ANALYSIS.md)
   - 4 issues identified in bindings layer
   - Focus: Architecture simplification, on-demand rendering
   - Scope: `flui_app/src/binding/` (~400 lines)

**Combined Result:** 11 total issues, unified refactoring plan, 2-3 weeks timeline

---

## Issues Cross-Reference Matrix

| ID | Issue | Layer | Severity | Effort | Related |
|----|-------|-------|----------|--------|---------|
| **P1** | Frame lifecycle not implemented | Pipeline | CRITICAL | 1 day | B2 |
| **P2** | RebuildQueue flushed twice | Pipeline | CRITICAL | 3 hours | B1, B2 |
| **P3** | Layout marking inconsistency | Pipeline | HIGH | 6 hours | - |
| **P4** | Missing validation in attach() | Pipeline | HIGH | 8 hours | B1 |
| **P5** | Component rebuild duplicate marking | Pipeline | MEDIUM | 4 hours | P2 |
| **P6** | Binding abstraction gaps | Pipeline | MEDIUM | 6 hours | B1 |
| **P7** | RenderElement in build dirty set | Pipeline | LOW | 2 hours | - |
| **B1** | PipelineBinding is redundant | Bindings | HIGH | 4 hours | P6 |
| **B2** | On-demand rendering missing | Bindings | HIGH | 4 hours | P1 |
| **B3** | Circular references in callbacks | Bindings | MEDIUM | 2 hours | P2 |
| **B4** | Pipeline ownership duplication | Bindings | MEDIUM | 3 hours | B1 |

**Legend:**  
- P = Pipeline issue (from previous analysis)
- B = Bindings issue (from new analysis)

---

## Architecture Overview - Current vs Ideal

### Current Architecture (Problematic)

```
┌─────────────────────────────────────────────────────────────────┐
│                         User Application                         │
└──────────────────────────┬──────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                         run_app()                                │
│  - Creates EventLoop                                             │
│  - NO begin_frame() / end_frame() calls ❌                       │
│  - ALWAYS requests redraw ❌                                     │
└──────────────────────────┬──────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                    AppBinding (Singleton)                        │
│  ┌───────────────┐  ┌──────────────┐  ┌────────────────────┐   │
│  │ GestureBinding│  │SchedulerBinding│ │RendererBinding    │   │
│  └───────────────┘  └──────────────┘  └────────────────────┘   │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ PipelineBinding ❌ REDUNDANT LAYER                       │   │
│  │   - Just wraps Arc<RwLock<PipelineOwner>>               │   │
│  │   - No added value                                       │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                  │
│  Scheduler callback ❌ CIRCULAR REF:                            │
│    Arc<RwLock<PipelineOwner>> captured → memory leak risk      │
│                                                                  │
│  wire_up() ❌ DOUBLE FLUSH:                                     │
│    flush_rebuild_queue() in callback +                          │
│    flush_rebuild_queue() in build_frame loop                    │
└──────────────────────────┬──────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                      PipelineOwner                               │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │ FrameCoordinator                                          │  │
│  │   ├─ BuildPipeline  (dirty_elements: Vec)                │  │
│  │   ├─ LayoutPipeline (dirty_set: LockFreeDirtySet) ❌      │  │
│  │   │   - Must ALSO set RenderState.needs_layout flag      │  │
│  │   │   - Two places = easy to miss                        │  │
│  │   └─ PaintPipeline  (dirty_set: LockFreeDirtySet)        │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                  │
│  attach() ❌ ISSUES:                                             │
│    - No error handling                                          │
│    - Duplicate marking (mark_dirty + schedule)                  │
│    - No cleanup on failure                                      │
└──────────────────────────────────────────────────────────────────┘
```

### Ideal Architecture (After Refactoring)

```
┌─────────────────────────────────────────────────────────────────┐
│                         User Application                         │
└──────────────────────────┬──────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                         run_app()                                │
│  ✅ Calls scheduler.begin_frame()                                │
│  ✅ Calls scheduler.end_frame()                                  │
│  ✅ On-demand rendering (only when dirty)                        │
└──────────────────────────┬──────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                    AppBinding (Singleton)                        │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │ Core Pipeline: Arc<RwLock<PipelineOwner>> ✅            │   │
│  │   - Single source of truth                               │   │
│  │   - No duplication                                       │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                  │
│  ┌───────────────┐  ┌──────────────┐  ┌────────────────────┐   │
│  │ GestureBinding│  │SchedulerBinding│ │RendererBinding    │   │
│  └───────────────┘  └──────────────┘  └────────────────────┘   │
│                                                                  │
│  ✅ PipelineBinding REMOVED - methods moved to AppBinding       │
│                                                                  │
│  ✅ Scheduler callback uses Weak<RwLock<PipelineOwner>>         │
│     - No circular references                                    │
│     - Proper cleanup on shutdown                                │
│                                                                  │
│  ✅ flush_rebuild_queue() called ONCE per frame                 │
│     - Only in scheduler callback                                │
│     - Removed from build_frame loop                             │
│                                                                  │
│  ✅ needs_redraw: Arc<AtomicBool>                               │
│     - On-demand rendering                                       │
│     - 50-100x lower idle CPU                                    │
└──────────────────────────┬──────────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────────┐
│                      PipelineOwner                               │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │ FrameCoordinator                                          │  │
│  │   ├─ BuildPipeline  (dirty_elements: Vec)                │  │
│  │   ├─ LayoutPipeline (dirty_set: LockFreeDirtySet) ✅      │  │
│  │   │   - mark_layout_dirty() helper                       │  │
│  │   │   - Sets both dirty set AND RenderState flag         │  │
│  │   └─ PaintPipeline  (dirty_set: LockFreeDirtySet)        │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                  │
│  ✅ attach() returns Result<ElementId, PipelineError>           │
│     - Proper error handling                                     │
│     - No duplicate marking                                      │
│     - Cleanup on failure                                        │
└──────────────────────────────────────────────────────────────────┘
```

---

## Unified Refactoring Phases

### Phase 1: Frame Lifecycle Integration (CRITICAL)
**Duration:** 1 day  
**Issues:** P1, B2  
**Files Changed:** 3

#### What to Do

1. **Add begin_frame() / end_frame() to embedders**

```rust
// crates/flui_app/src/embedder/desktop.rs
impl DesktopEmbedder {
    pub fn render_frame(&mut self) {
        // ✅ ADD: Begin frame
        self.binding.scheduler.scheduler().begin_frame();
        
        // Existing: Draw frame
        let constraints = BoxConstraints::tight(self.window_size);
        let scene = self.binding.draw_frame(constraints);
        
        // Present to GPU
        self.renderer.render(&scene);
        
        // ✅ ADD: End frame
        self.binding.scheduler.scheduler().end_frame();
    }
}
```

2. **Implement on-demand rendering in run_app**

```rust
// crates/flui_app/src/lib.rs
Event::AboutToWait => {
    if let Some(ref emb) = embedder {
        // ✅ CHANGE: Only redraw when needed
        if binding.needs_redraw() {
            emb.window().request_redraw();
        }
    }
}

Event::WindowEvent { 
    event: WindowEvent::RedrawRequested,
    ..
} => {
    if let Some(ref mut emb) = embedder {
        emb.render_frame();
        
        // ✅ ADD: Clear dirty flag
        binding.mark_rendered();
    }
}
```

#### Testing

```bash
# Verify begin_frame() triggers scheduler callbacks
RUST_LOG=flui_app=debug,flui_scheduler=debug cargo run --example counter

# Verify on-demand rendering reduces CPU
# Before: ~5-10% CPU idle
# After: ~0.1% CPU idle
```

---

### Phase 2: Remove PipelineBinding Layer (HIGH)
**Duration:** 4-6 hours  
**Issues:** B1, B4, P6  
**Files Changed:** 5

#### What to Do

1. **Delete PipelineBinding**

```bash
rm crates/flui_app/src/binding/pipeline.rs
```

2. **Update AppBinding**

```rust
// crates/flui_app/src/binding/app_binding.rs
pub struct AppBinding {
    // ✅ ADD: Direct pipeline ownership
    pipeline_owner: Arc<RwLock<PipelineOwner>>,
    
    // ✅ REMOVE: pipeline: PipelineBinding field
    
    gesture: GestureBinding,
    scheduler: SchedulerBinding,
    renderer: RendererBinding,
    
    // ✅ ADD: On-demand rendering flag
    needs_redraw: Arc<AtomicBool>,
}

impl AppBinding {
    // ✅ ADD: Methods moved from PipelineBinding
    pub fn attach_root_widget<V: View + 'static>(&self, app: V) {
        let element = app.into_element();
        let mut owner = self.pipeline_owner.write();
        owner.set_root(element);
        self.request_redraw();
    }
    
    pub fn pipeline(&self) -> Arc<RwLock<PipelineOwner>> {
        self.pipeline_owner.clone()
    }
    
    // ✅ ADD: On-demand rendering methods
    pub fn request_redraw(&self) {
        self.needs_redraw.store(true, Ordering::Relaxed);
    }
    
    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw.load(Ordering::Relaxed)
    }
    
    pub fn mark_rendered(&self) {
        self.needs_redraw.store(false, Ordering::Relaxed);
    }
}
```

3. **Update RendererBinding**

```rust
// crates/flui_app/src/binding/renderer.rs
pub struct RendererBinding {
    // ✅ REMOVE: pipeline field
}

impl RendererBinding {
    pub fn new() -> Self {
        Self {}
    }
    
    // ✅ CHANGE: Take pipeline as parameter
    pub fn draw_frame(
        &self,
        pipeline: &Arc<RwLock<PipelineOwner>>,
        constraints: BoxConstraints,
    ) -> Scene {
        let mut owner = pipeline.write();
        owner.build_frame(constraints).unwrap_or_else(|_| Scene::empty())
    }
}
```

4. **Update all callers**

```rust
// Before
binding.pipeline.attach_root_widget(MyApp);
let pipeline = binding.pipeline.pipeline_owner();

// After
binding.attach_root_widget(MyApp);
let pipeline = binding.pipeline();
```

#### Testing

```bash
# Verify all tests pass
cargo test -p flui_app

# Verify examples compile and run
cargo run --example counter
cargo run --example profile_card
```

---

### Phase 3: Fix Circular References (MEDIUM)
**Duration:** 2-3 hours  
**Issues:** B3, P2  
**Files Changed:** 2

#### What to Do

1. **Use Weak in scheduler callback**

```rust
// crates/flui_app/src/binding/app_binding.rs
fn wire_up(&self, needs_redraw: Arc<AtomicBool>) {
    // ✅ CHANGE: Use Weak to avoid circular ref
    let pipeline_weak = Arc::downgrade(&self.pipeline_owner);
    
    self.scheduler.scheduler().add_persistent_frame_callback(
        Arc::new(move |_timing| {
            // ✅ ADD: Try to upgrade Weak
            if let Some(pipeline) = pipeline_weak.upgrade() {
                let mut owner = pipeline.write();
                if owner.flush_rebuild_queue() {
                    needs_redraw.store(true, Ordering::Relaxed);
                }
            } else {
                tracing::warn!("Pipeline dropped during frame callback");
            }
        })
    );
}
```

2. **Remove duplicate flush from build_frame**

```rust
// crates/flui_core/src/pipeline/frame_coordinator.rs
pub fn build_frame(&mut self, tree: Arc<RwLock<ElementTree>>, constraints: BoxConstraints) 
    -> Result<Scene, PipelineError> 
{
    loop {
        // ✅ REMOVE: This line (redundant flush)
        // self.build.flush_rebuild_queue();
        
        self.build.flush_batch();
        
        if self.build.dirty_count() == 0 {
            break;
        }
        
        self.build.rebuild_dirty_parallel(tree.clone());
    }
    
    // Layout and paint...
}
```

#### Testing

```bash
# Verify no memory leaks with valgrind
valgrind --leak-check=full cargo run --example counter

# Verify scheduler callbacks run
RUST_LOG=flui_scheduler=debug cargo run --example counter
```

---

### Phase 4: Consolidate Layout Marking (HIGH)
**Duration:** 6 hours  
**Issues:** P3  
**Files Changed:** 3

#### What to Do

1. **Create helper method**

```rust
// crates/flui_core/src/pipeline/pipeline_owner.rs
impl PipelineOwner {
    /// Mark element for layout (sets both dirty set AND RenderState flag)
    fn mark_layout_dirty(&mut self, node_id: ElementId) {
        // Place 1: Add to dirty set
        self.coordinator.layout_mut().mark_dirty(node_id);
        
        // Place 2: Set RenderState flag
        let tree = self.tree.read();
        if let Some(Element::Render(render_elem)) = tree.get(node_id) {
            let mut render_state = render_elem.render_state().write();
            render_state.mark_needs_layout();
            render_state.clear_constraints();
        }
    }
    
    /// Public API: Request layout for an element
    pub fn request_layout(&mut self, node_id: ElementId) {
        self.mark_layout_dirty(node_id);
    }
}
```

2. **Update attach() to use helper**

```rust
// Before (duplicate code)
pub fn attach<V: View + 'static>(&mut self, widget: V) -> ElementId {
    let element = widget.into_element();
    let root_id = self.set_root(element);
    
    // Duplicate marking
    self.coordinator.build_mut().schedule(root_id, 0);
    self.coordinator.layout_mut().mark_dirty(root_id);
    
    // Also set flag manually
    let tree = self.tree.read();
    if let Some(Element::Render(render_elem)) = tree.get(root_id) {
        let mut render_state = render_elem.render_state().write();
        render_state.mark_needs_layout();
    }
    
    root_id
}

// After (using helpers)
pub fn attach<V: View + 'static>(&mut self, widget: V) 
    -> Result<ElementId, PipelineError> 
{
    let element = widget.into_element();
    let root_id = self.set_root(element);
    
    // ✅ USE: Single method for build
    self.schedule_build_for(root_id, 0);
    
    // ✅ USE: Single method for layout
    self.mark_layout_dirty(root_id);
    
    Ok(root_id)
}
```

#### Testing

```bash
# Test layout marking
cargo test -p flui_core test_request_layout

# Test attach with layout
cargo test -p flui_core test_attach_marks_layout
```

---

### Phase 5: Add Validation to attach() (HIGH)
**Duration:** 8 hours  
**Issues:** P4  
**Files Changed:** 2

#### What to Do

1. **Return Result instead of panicking**

```rust
// crates/flui_core/src/pipeline/pipeline_owner.rs
impl PipelineOwner {
    /// Attach a widget as root (with error handling)
    pub fn attach<V: View + 'static>(&mut self, widget: V) 
        -> Result<ElementId, PipelineError> 
    {
        // Validate no existing root
        if self.root_mgr.has_root() {
            return Err(PipelineError::invalid_state(
                "Root widget already attached. Call teardown() first."
            )?);
        }
        
        // Build element (can panic - wrap in catch_unwind if needed)
        let element = widget.into_element();
        
        // Set as root
        let root_id = self.set_root(element);
        
        // Schedule initial build
        self.schedule_build_for(root_id, 0);
        
        // Mark for layout
        self.mark_layout_dirty(root_id);
        
        Ok(root_id)
    }
    
    /// Teardown existing root (for hot reload, testing)
    pub fn teardown(&mut self) -> Result<(), PipelineError> {
        if let Some(root_id) = self.root_element_id() {
            let mut tree = self.tree.write();
            tree.remove(root_id)?;
            self.root_mgr.clear();
            Ok(())
        } else {
            Err(PipelineError::invalid_state("No root to teardown")?)
        }
    }
}
```

2. **Update callers to handle Result**

```rust
// Before
let root_id = pipeline.attach(MyApp);

// After
let root_id = pipeline.attach(MyApp)
    .expect("Failed to attach root widget");

// Or with proper error handling
match pipeline.attach(MyApp) {
    Ok(root_id) => {
        tracing::info!("Root attached: {:?}", root_id);
    }
    Err(err) => {
        tracing::error!("Failed to attach root: {}", err);
        return Err(err);
    }
}
```

#### Testing

```bash
# Test error handling
cargo test -p flui_core test_attach_twice_fails
cargo test -p flui_core test_teardown_and_reattach

# Integration test
cargo test -p flui_app test_hot_reload
```

---

### Phase 6: Minor Cleanups (MEDIUM)
**Duration:** 6-8 hours  
**Issues:** P5, P7  
**Files Changed:** 3

#### What to Do

1. **Remove duplicate marking in component rebuild**

```rust
// crates/flui_core/src/pipeline/build_pipeline.rs

// Before (duplicate)
fn process_component(...) {
    component.mark_dirty();
    self.schedule(element_id, depth);  // Also marks dirty!
}

// After (single method)
fn process_component(...) {
    self.schedule(element_id, depth);  // Only this
}
```

2. **Add validation for RenderElement in build dirty set**

```rust
// crates/flui_core/src/pipeline/build_pipeline.rs
pub fn schedule(&mut self, element_id: ElementId, depth: usize) {
    // ✅ ADD: Validate element type
    let tree = self.tree.read();
    if let Some(element) = tree.get(element_id) {
        match element {
            Element::Component(_) | Element::Provider(_) => {
                // Valid - components can be rebuilt
                self.dirty_elements.push((element_id, depth));
            }
            Element::Render(_) => {
                // Invalid - RenderElements don't rebuild
                tracing::warn!(
                    "Attempted to schedule RenderElement for build: {:?}",
                    element_id
                );
            }
        }
    }
}
```

#### Testing

```bash
# Test validation
cargo test -p flui_core test_schedule_render_element_warns

# Verify no duplicate marking
cargo test -p flui_core test_component_rebuild_no_duplicate
```

---

## Summary of All Changes

### Files to Modify

| File | Changes | Lines Changed | Effort |
|------|---------|---------------|--------|
| `flui_app/src/lib.rs` | Add begin_frame/end_frame, on-demand rendering | +15 | 2h |
| `flui_app/src/embedder/desktop.rs` | Update render_frame() | +10 | 1h |
| `flui_app/src/embedder/android.rs` | Update render_frame() | +10 | 1h |
| `flui_app/src/binding/app_binding.rs` | Remove PipelineBinding, add methods | +80, -20 | 4h |
| `flui_app/src/binding/renderer.rs` | Remove pipeline field | +5, -15 | 1h |
| `flui_app/src/binding/pipeline.rs` | **DELETE FILE** | -150 | 1h |
| `flui_app/src/binding/mod.rs` | Remove PipelineBinding exports | -2 | 10m |
| `flui_core/src/pipeline/pipeline_owner.rs` | Add helpers, return Result | +50, -10 | 6h |
| `flui_core/src/pipeline/frame_coordinator.rs` | Remove duplicate flush | -1 | 10m |
| `flui_core/src/pipeline/build_pipeline.rs` | Add validation, remove duplicate | +15, -5 | 2h |
| **Total** | | **+182, -203** | **18-20h** |

### Tests to Add

| Test | Location | Purpose |
|------|----------|---------|
| `test_frame_lifecycle` | flui_app | Verify begin_frame/end_frame called |
| `test_on_demand_rendering` | flui_app | Verify no redraw when clean |
| `test_no_circular_refs` | flui_app | Verify Weak cleanup |
| `test_attach_twice_fails` | flui_core | Verify error on double attach |
| `test_mark_layout_dirty` | flui_core | Verify both places marked |
| `test_schedule_render_warns` | flui_core | Verify validation works |

### Documentation to Update

- [ ] `FINAL_ARCHITECTURE_V2.md` - Update binding diagram
- [ ] `PIPELINE_ARCHITECTURE.md` - Add frame lifecycle
- [ ] `README.md` - Mention performance improvements
- [ ] `CHANGELOG.md` - Document breaking changes
- [ ] Add `MIGRATION_GUIDE.md` (already created)

---

## Testing Strategy

### Phase 1: Unit Tests

```bash
# Test each component in isolation
cargo test -p flui_core
cargo test -p flui_app

# Verify no regressions
cargo test --workspace
```

### Phase 2: Integration Tests

```bash
# Test full frame cycle
cargo test -p flui_app test_frame_lifecycle

# Test signal → rebuild → render
cargo test -p flui_app test_signal_triggers_rebuild

# Test window resize → layout
cargo test -p flui_app test_window_resize_layout
```

### Phase 3: Performance Tests

```bash
# Measure idle CPU (should be <0.5%)
cargo run --example counter --release
# Let idle for 30 seconds, check CPU usage

# Measure frame times (should be <16.67ms @ 60fps)
RUST_LOG=debug cargo run --example counter
# Check tracing output for frame times

# Profile for lock contention
cargo install flamegraph
cargo flamegraph --example counter
# Check for lock wait time
```

### Phase 4: Platform Tests

```bash
# Desktop
cargo run --example counter  # Windows
cargo run --example counter  # macOS
cargo run --example counter  # Linux

# Android
cargo apk run --example android_demo
```

---

## Timeline Estimate

| Phase | Duration | Can Parallelize |
|-------|----------|-----------------|
| Phase 1: Frame Lifecycle | 1 day | No (foundation) |
| Phase 2: Remove PipelineBinding | 6 hours | After Phase 1 |
| Phase 3: Fix Circular Refs | 3 hours | With Phase 2 |
| Phase 4: Layout Marking | 6 hours | After Phase 1 |
| Phase 5: Validate attach() | 8 hours | After Phase 4 |
| Phase 6: Minor Cleanups | 8 hours | With Phase 5 |
| **Total** | **2-3 weeks** | |

### Optimized Parallel Timeline

```
Week 1:
  Day 1: Phase 1 (Frame Lifecycle) ← Blocks everything
  Day 2: Phase 2 (Remove PipelineBinding) + Phase 3 (Circular Refs)
  Day 3: Phase 4 (Layout Marking)
  Day 4: Phase 5 (Validate attach())
  Day 5: Testing + Bug Fixes

Week 2:
  Day 1: Phase 6 (Minor Cleanups)
  Day 2-3: Integration Testing
  Day 4: Documentation
  Day 5: Code Review + PR

Total: 2 weeks focused work
```

---

## Success Criteria

### Must Have (Blocking PR Merge)

- [ ] All tests pass
- [ ] No clippy warnings
- [ ] No rustdoc warnings
- [ ] All examples run correctly
- [ ] begin_frame/end_frame implemented
- [ ] On-demand rendering works
- [ ] PipelineBinding removed
- [ ] No circular references
- [ ] attach() returns Result

### Should Have (Nice to Have)

- [ ] 50%+ lower idle CPU usage
- [ ] Frame times under 16.67ms
- [ ] Migration guide complete
- [ ] All documentation updated
- [ ] Performance benchmarks added

### Nice to Have (Post-Merge)

- [ ] Blog post about architecture
- [ ] Video demo of improvements
- [ ] Hot reload support
- [ ] Visual debugging tools

---

## Risk Assessment

### High Risk

1. **Breaking API changes** - Users depend on PipelineBinding
   - **Mitigation**: Provide clear migration guide
   - **Rollback**: Keep old code in backup branch

2. **Performance regression** - Changes affect critical path
   - **Mitigation**: Comprehensive performance testing
   - **Rollback**: Easy to revert (well-isolated changes)

### Medium Risk

3. **Scheduler integration bugs** - begin_frame/end_frame timing
   - **Mitigation**: Thorough testing on all platforms
   - **Rollback**: Can disable new code path

4. **Memory leaks** - Weak references might not cleanup properly
   - **Mitigation**: Valgrind testing, AddressSanitizer
   - **Rollback**: Revert to Arc if issues found

### Low Risk

5. **Minor edge cases** - attach() validation, etc.
   - **Mitigation**: Comprehensive unit tests
   - **Rollback**: Easy to fix incrementally

---

## Monitoring Post-Merge

### Metrics to Track

1. **CPU Usage** (idle)
   - Baseline: 5-10%
   - Target: <0.5%
   - How: Task manager / htop

2. **Frame Times**
   - Baseline: 1-3ms avg
   - Target: <1ms avg
   - How: Tracing spans

3. **Memory Usage**
   - Baseline: Current
   - Target: Same or lower
   - How: Valgrind, AddressSanitizer

4. **Crash Reports**
   - Target: Zero crashes
   - How: GitHub issues, crash logs

### Monitoring Period

- **First week**: Daily monitoring
- **First month**: Weekly monitoring
- **After month**: Normal monitoring

---

## Communication Plan

### Before Merge

1. Post RFC in GitHub Discussions
2. Share migration guide
3. Announce breaking changes clearly
4. Ask for feedback on plan

### During Development

1. Daily updates in project channel
2. Demo videos of progress
3. Early testing with volunteers
4. Address concerns promptly

### After Merge

1. Release notes with highlights
2. Migration assistance
3. Monitor issues closely
4. Quick hotfixes if needed

---

## Rollback Plan

If critical issues discovered after merge:

1. **Immediate**: Revert the PR
2. **Short-term**: Document the issue in detail
3. **Medium-term**: Fix in isolated branch with tests
4. **Long-term**: Re-merge with regression tests

Keep old code in `backup/pre-refactor` branch for 3 months.

---

## Next Steps

1. **Review this plan** with team
2. **Get approval** for breaking changes
3. **Create GitHub issues** for each phase
4. **Set up branches** (one per phase)
5. **Start Phase 1** implementation

---

## Questions?

- **Architecture questions**: See PIPELINE_AND_BINDING_ARCHITECTURE.md
- **Implementation details**: See QUICK_REFERENCE.md
- **Migration help**: See MIGRATION_GUIDE.md
- **Diagrams**: See ARCHITECTURE_DIAGRAMS.md

---

**Document Version:** 1.0  
**Last Updated:** November 21, 2025  
**Next Review:** After Phase 1 completion