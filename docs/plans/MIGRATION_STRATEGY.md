# Migration Strategy: Existing Crates → GPUI-Enhanced Architecture

> **Created**: 2026-01-22  
> **Status**: Ready for Execution  
> **Effort**: 3-4 weeks  
> **Risk**: Low (incremental changes)

---

## Overview

This document provides a **step-by-step migration plan** to upgrade existing FLUI crates with GPUI-enhanced patterns while preserving all working code.

**Philosophy**: Enhance, don't rewrite. 🔄 not ❌.

---

## Week 1: Workspace Restoration (5 days)

**Goal**: Get all core crates compiling together

**Risk**: Low - just dependency management

---

### Day 1: Foundation Layer ✅

**Task**: Re-enable foundation crates

**Steps**:

1. **Update `Cargo.toml` workspace members**:
   ```toml
   members = [
       # Foundation (already active)
       "crates/flui_types",
       "crates/flui-platform",
       
       # Re-enable foundation
       "crates/flui-foundation",
       "crates/flui-tree",
       "crates/flui_log",
   ]
   ```

2. **Build and test**:
   ```bash
   cargo build -p flui-foundation
   cargo test -p flui-foundation
   cargo build -p flui-tree
   cargo test -p flui-tree
   ```

3. **Fix any issues**:
   - Update import paths if needed
   - Check for deprecated APIs
   - Verify parking_lot/slab versions

**Verification**:
```bash
cargo build --workspace
# Should compile: flui_types, flui-platform, flui-foundation, flui-tree, flui_log
```

**Deliverable**: Foundation crates compiling ✅

---

### Day 2: Rendering Stack ✅

**Task**: Re-enable painting and layer crates

**Steps**:

1. **Update `Cargo.toml`**:
   ```toml
   members = [
       # ... previous ...
       "crates/flui_painting",
       "crates/flui-layer",
       "crates/flui-semantics",
   ]
   ```

2. **Build chain** (respecting dependencies):
   ```bash
   cargo build -p flui_painting
   cargo build -p flui-layer
   cargo build -p flui-semantics
   ```

3. **Run tests**:
   ```bash
   cargo test -p flui_painting
   cargo test -p flui-layer
   cargo test -p flui-semantics
   ```

**Verification**:
```bash
cargo build --workspace
# Should compile: foundation + painting + layer + semantics
```

**Deliverable**: Rendering support crates compiling ✅

---

### Day 3: Engine + Interaction ✅

**Task**: Re-enable flui_engine and flui_interaction

**Steps**:

1. **Update `Cargo.toml`**:
   ```toml
   members = [
       # ... previous ...
       "crates/flui_engine",
       "crates/flui_interaction",
   ]
   ```

2. **Verify wgpu version** (CRITICAL):
   ```bash
   # Check Cargo.toml uses wgpu 25.x (NOT 26.x or 27.x)
   grep "wgpu" crates/flui_engine/Cargo.toml
   # Should see: wgpu = { workspace = true, version = "25.0" }
   ```

3. **Build**:
   ```bash
   cargo build -p flui_engine
   cargo build -p flui_interaction
   ```

4. **Test**:
   ```bash
   cargo test -p flui_interaction  # Should pass
   # flui_engine tests may need GPU, skip if headless
   ```

**Known Issues**:
- wgpu tests require GPU context
- Use `--lib` to skip integration tests if needed

**Deliverable**: Engine + interaction compiling ✅

---

### Day 4: Rendering + Scheduler ⚠️

**Task**: Re-enable flui_rendering and flui-scheduler

**Steps**:

1. **Update `Cargo.toml`**:
   ```toml
   members = [
       # ... previous ...
       "crates/flui_rendering",
       "crates/flui-scheduler",
       "crates/flui_animation",
   ]
   ```

2. **Build** (may have errors - expected):
   ```bash
   cargo build -p flui_rendering 2>&1 | tee build_errors.txt
   cargo build -p flui-scheduler
   cargo build -p flui_animation
   ```

3. **Common fixes**:
   - Import path updates
   - Trait bound adjustments
   - Update to new flui_types geometry APIs

4. **Document issues**:
   ```bash
   # Create issue tracker
   echo "# Rendering Issues" > docs/plans/WEEK1_ISSUES.md
   # Add each error with context
   ```

**Expected Issues**:
- Some RenderObject implementations may need updates
- Protocol trait bounds may conflict

**Deliverable**: Identified all compilation issues ✅

---

### Day 5: View + App Layer ⚠️

**Task**: Re-enable flui-view and flui_app

**Steps**:

1. **Update `Cargo.toml`**:
   ```toml
   members = [
       # ... previous ...
       "crates/flui-view",
       "crates/flui_app",
   ]
   ```

2. **Build**:
   ```bash
   cargo build -p flui-view 2>&1 | tee view_errors.txt
   cargo build -p flui_app 2>&1 | tee app_errors.txt
   ```

3. **Fix immediate issues**:
   - Update imports
   - Fix obvious type mismatches
   - Don't refactor yet - just get compiling

4. **Create fix plan**:
   ```bash
   # Categorize errors
   echo "## Critical" >> docs/plans/WEEK1_ISSUES.md
   echo "## Minor" >> docs/plans/WEEK1_ISSUES.md
   echo "## Can defer" >> docs/plans/WEEK1_ISSUES.md
   ```

**Goal**: Understand what needs fixing, not fix everything

**Deliverable**: Full workspace compiles (with warnings OK) ✅

---

### Week 1 Deliverables

- ✅ All 14 core crates re-enabled in workspace
- ✅ `cargo build --workspace` succeeds (warnings OK)
- ✅ Foundation tests pass
- ✅ Interaction tests pass
- 📋 Documented list of issues for Week 2-3

**Metrics**:
- **Compilation success**: 100%
- **Test pass rate**: >80% (some may be skipped)
- **Time spent**: 5 days

---

## Week 2: Phase 5 V2 (flui-view) - GPUI Enhancements

**Goal**: Apply GPUI patterns to flui-view

**Risk**: Medium - API changes affect elements

---

### Day 1: Associated Types Design 🎨

**Task**: Design Element trait with associated types

**Steps**:

1. **Read current Element trait**:
   ```bash
   cat crates/flui-view/src/element/behavior.rs
   ```

2. **Create V2 design document**:
   ```bash
   cat > docs/plans/ELEMENT_V2_DESIGN.md << 'EOF'
   # Element Trait V2 Design
   
   ## Current
   
   ```rust
   pub trait Element {
       fn layout(&mut self, constraints: Constraints) -> Size;
       fn paint(&self, context: &mut PaintContext);
   }
   ```
   
   ## V2 (GPUI-style)
   
   ```rust
   pub trait Element: 'static + Send {
       type LayoutState: 'static;
       type PrepaintState: 'static;
       
       fn request_layout(
           &mut self, 
           cx: &mut LayoutContext
       ) -> (LayoutId, Self::LayoutState);
       
       fn prepaint(
           &mut self,
           layout: &mut Self::LayoutState,
           cx: &mut PrepaintContext
       ) -> Self::PrepaintState;
       
       fn paint(
           &self,
           layout: &Self::LayoutState,
           prepaint: &Self::PrepaintState,
           cx: &mut PaintContext
       );
   }
   ```
   EOF
   ```

3. **Prototype StatelessElement**:
   ```bash
   # Create prototype in docs/prototypes/
   mkdir -p docs/prototypes
   cat > docs/prototypes/stateless_element_v2.rs << 'EOF'
   // Prototype: StatelessElement with associated types
   // ... implementation ...
   EOF
   ```

4. **Review with team** (if applicable):
   - Discuss type bounds
   - Consider backward compatibility
   - Plan migration path

**Deliverable**: Element V2 trait design approved ✅

---

### Day 2: Implement Element V2 Trait 🔧

**Task**: Add associated types to Element trait

**Steps**:

1. **Update element/behavior.rs**:
   ```rust
   // Add to existing file
   
   /// GPUI-style element with type-safe state threading
   pub trait ElementV2: 'static + Send {
       type LayoutState: 'static;
       type PrepaintState: 'static;
       
       fn request_layout(
           &mut self,
           cx: &mut LayoutContext,
       ) -> (LayoutId, Self::LayoutState);
       
       fn prepaint(
           &mut self,
           layout: &mut Self::LayoutState,
           cx: &mut PrepaintContext,
       ) -> Self::PrepaintState;
       
       fn paint(
           &self,
           layout: &Self::LayoutState,
           prepaint: &Self::PrepaintState,
           cx: &mut PaintContext,
       );
   }
   ```

2. **Add deprecation notice to old trait**:
   ```rust
   #[deprecated(since = "0.2.0", note = "Use ElementV2 instead")]
   pub trait Element {
       // ... existing ...
   }
   ```

3. **Create migration guide**:
   ```bash
   cat > docs/MIGRATION_ELEMENT_V2.md << 'EOF'
   # Migrating to Element V2
   
   ## Old Code
   ```rust
   impl Element for MyElement {
       fn layout(&mut self, constraints: Constraints) -> Size {
           // ...
       }
   }
   ```
   
   ## New Code
   ```rust
   struct MyElementLayout {
       child_layout_id: LayoutId,
   }
   
   struct MyElementPrepaint {
       child_bounds: Bounds,
   }
   
   impl ElementV2 for MyElement {
       type LayoutState = MyElementLayout;
       type PrepaintState = MyElementPrepaint;
       
       fn request_layout(&mut self, cx: &mut LayoutContext) 
           -> (LayoutId, Self::LayoutState) 
       {
           // ...
       }
   }
   ```
   EOF
   ```

**Deliverable**: ElementV2 trait implemented ✅

---

### Day 3: Migrate StatelessElement ⚡

**Task**: Update StatelessElement to use ElementV2

**Steps**:

1. **Update element/generic.rs** (or wherever StatelessElement lives):
   ```rust
   pub struct StatelessLayoutState {
       child_layout_id: Option<LayoutId>,
   }
   
   pub struct StatelessPrepaintState {
       child_hitbox: Option<Hitbox>,
   }
   
   impl<V: StatelessView> ElementV2 for StatelessElement<V> {
       type LayoutState = StatelessLayoutState;
       type PrepaintState = StatelessPrepaintState;
       
       fn request_layout(&mut self, cx: &mut LayoutContext) 
           -> (LayoutId, Self::LayoutState) 
       {
           let child_layout_id = self.child.as_ref()
               .map(|child| child.request_layout(cx));
           
           let layout_id = cx.request_layout(self.view.size_hint());
           
           (layout_id, StatelessLayoutState { child_layout_id })
       }
       
       fn prepaint(
           &mut self,
           layout: &mut Self::LayoutState,
           cx: &mut PrepaintContext,
       ) -> Self::PrepaintState {
           let child_hitbox = layout.child_layout_id
               .and_then(|id| {
                   self.child.as_mut()?.prepaint(id, cx)
               });
           
           StatelessPrepaintState { child_hitbox }
       }
       
       fn paint(
           &self,
           layout: &Self::LayoutState,
           prepaint: &Self::PrepaintState,
           cx: &mut PaintContext,
       ) {
           if let Some(child) = &self.child {
               child.paint(&prepaint.child_hitbox, cx);
           }
       }
   }
   ```

2. **Test**:
   ```bash
   cargo test -p flui-view --lib
   ```

3. **Fix any breakages**:
   - Update BuildContext if needed
   - Fix LayoutContext/PrepaintContext APIs

**Deliverable**: StatelessElement using ElementV2 ✅

---

### Day 4: Add Source Location Tracking 📍

**Task**: Add #[track_caller] and Location storage

**Steps**:

1. **Update element/generic.rs**:
   ```rust
   pub struct StatelessElement<V: StatelessView> {
       view: V,
       child: Option<ElementId>,
       
       #[cfg(debug_assertions)]
       source_location: Option<&'static std::panic::Location<'static>>,
   }
   
   impl<V: StatelessView> StatelessElement<V> {
       #[track_caller]
       pub fn new(view: V) -> Self {
           Self {
               view,
               child: None,
               
               #[cfg(debug_assertions)]
               source_location: Some(std::panic::Location::caller()),
           }
       }
       
       #[cfg(debug_assertions)]
       pub fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
           self.source_location
       }
   }
   ```

2. **Update error messages**:
   ```rust
   impl<V: StatelessView> ElementV2 for StatelessElement<V> {
       fn request_layout(&mut self, cx: &mut LayoutContext) 
           -> (LayoutId, Self::LayoutState) 
       {
           #[cfg(debug_assertions)]
           tracing::debug!(
               target: "flui_view::element",
               location = %self.source_location().unwrap(),
               "Requesting layout"
           );
           
           // ... rest ...
       }
   }
   ```

3. **Add to all Element types**:
   - StatefulElement
   - InheritedElement
   - RenderElement
   - ProxyElement

4. **Test debug output**:
   ```bash
   RUST_LOG=debug cargo test -p flui-view -- --nocapture
   # Should see source locations in logs
   ```

**Deliverable**: All elements track source location ✅

---

### Day 5: Testing + Documentation 🧪

**Task**: Comprehensive testing and docs

**Steps**:

1. **Write migration tests**:
   ```rust
   // crates/flui-view/tests/element_v2_tests.rs
   
   #[test]
   fn test_stateless_element_v2() {
       // Test associated types
       // Test three-phase lifecycle
       // Test source location
   }
   ```

2. **Update examples**:
   ```bash
   # Update all examples to use ElementV2
   find crates/flui-view/examples -name "*.rs" -exec sed -i 's/impl Element/impl ElementV2/g' {} \;
   ```

3. **Write docs**:
   ```bash
   cat > docs/VIEW_V2_GUIDE.md << 'EOF'
   # View V2 Upgrade Guide
   
   ## What Changed
   
   - Element trait now has associated types
   - Three-phase lifecycle (request_layout → prepaint → paint)
   - Source location tracking in debug builds
   
   ## Migration Steps
   
   1. Replace `Element` with `ElementV2`
   2. Add `LayoutState` and `PrepaintState` associated types
   3. Split `layout()` into `request_layout()` and `prepaint()`
   4. Add `#[track_caller]` to constructors
   
   ## Examples
   
   [... full examples ...]
   EOF
   ```

4. **Run full test suite**:
   ```bash
   cargo test -p flui-view
   cargo test --workspace  # Check nothing broke
   ```

**Deliverable**: flui-view V2 complete with tests ✅

---

### Week 2 Deliverables

- ✅ Element trait with associated types
- ✅ Three-phase lifecycle (request_layout → prepaint → paint)
- ✅ Source location tracking (debug builds)
- ✅ All element types migrated
- ✅ Tests passing
- 📚 Migration guide written

**Metrics**:
- **API coverage**: 100% of element types
- **Test pass rate**: >95%
- **Documentation**: Complete migration guide

---

## Week 3: Phase 6 V2 (flui_rendering) - GPUI Enhancements

**Goal**: Apply GPUI patterns to flui_rendering

**Risk**: Medium - affects layout pipeline

---

### Day 1: Pipeline Phase Tracking Design 🎨

**Task**: Design PipelinePhase enum and tracking

**Steps**:

1. **Create design document**:
   ```bash
   cat > docs/plans/PIPELINE_V2_DESIGN.md << 'EOF'
   # Pipeline Phase Tracking V2
   
   ## Current
   
   ```rust
   pub struct PipelineOwner {
       objects: Slab<Box<dyn RenderObject>>,
       needs_layout: Vec<RenderId>,
   }
   
   impl PipelineOwner {
       pub fn flush_layout(&mut self) {
           // No phase tracking
       }
   }
   ```
   
   ## V2 (GPUI-style)
   
   ```rust
   #[derive(Copy, Clone, Debug, PartialEq, Eq)]
   pub enum PipelinePhase {
       Idle,
       Layout,
       Compositing,
       Paint,
   }
   
   pub struct PipelineOwner {
       phase: Arc<RwLock<PipelinePhase>>,
       objects: Slab<Box<dyn RenderObject>>,
       needs_layout: Vec<RenderId>,
   }
   
   impl PipelineOwner {
       #[track_caller]
       pub fn assert_layout_phase(&self) {
           debug_assert!(
               *self.phase.read() == PipelinePhase::Layout,
               "Can only layout during Layout phase (called from {})",
               std::panic::Location::caller()
           );
       }
       
       pub fn flush_pipeline(&mut self) -> Scene {
           // Phase 1: Layout
           self.set_phase(PipelinePhase::Layout);
           self.flush_layout();
           
           // Phase 2: Compositing
           self.set_phase(PipelinePhase::Compositing);
           self.flush_compositing();
           
           // Phase 3: Paint
           self.set_phase(PipelinePhase::Paint);
           let scene = self.flush_paint();
           
           self.set_phase(PipelinePhase::Idle);
           scene
       }
   }
   ```
   EOF
   ```

2. **Prototype in docs/prototypes/**:
   ```rust
   // docs/prototypes/pipeline_phase_tracking.rs
   // Full working prototype
   ```

**Deliverable**: Pipeline V2 design approved ✅

---

### Day 2: Implement Pipeline Phase Tracking 🔧

**Task**: Add PipelinePhase to PipelineOwner

**Steps**:

1. **Update pipeline/mod.rs**:
   ```rust
   #[derive(Copy, Clone, Debug, PartialEq, Eq)]
   pub enum PipelinePhase {
       Idle,
       Layout,
       Compositing,
       Paint,
   }
   
   pub struct PipelineOwner {
       phase: Arc<RwLock<PipelinePhase>>,
       // ... existing fields ...
   }
   
   impl PipelineOwner {
       pub fn new() -> Self {
           Self {
               phase: Arc::new(RwLock::new(PipelinePhase::Idle)),
               // ... existing ...
           }
       }
       
       pub fn phase(&self) -> PipelinePhase {
           *self.phase.read()
       }
       
       fn set_phase(&self, phase: PipelinePhase) {
           *self.phase.write() = phase;
       }
       
       #[track_caller]
       pub fn assert_layout_phase(&self) {
           #[cfg(debug_assertions)]
           {
               let current = self.phase();
               debug_assert!(
                   current == PipelinePhase::Layout,
                   "Expected Layout phase, got {:?} (called from {})",
                   current,
                   std::panic::Location::caller()
               );
           }
       }
       
       // Similarly for assert_compositing_phase, assert_paint_phase
   }
   ```

2. **Update flush methods**:
   ```rust
   impl PipelineOwner {
       pub fn flush_pipeline(&mut self) -> Scene {
           tracing::debug!("Starting pipeline flush");
           
           self.set_phase(PipelinePhase::Layout);
           tracing::trace!("Phase: Layout");
           self.flush_layout();
           
           self.set_phase(PipelinePhase::Compositing);
           tracing::trace!("Phase: Compositing");
           self.flush_compositing();
           
           self.set_phase(PipelinePhase::Paint);
           tracing::trace!("Phase: Paint");
           let scene = self.flush_paint();
           
           self.set_phase(PipelinePhase::Idle);
           tracing::debug!("Pipeline flush complete");
           
           scene
       }
   }
   ```

3. **Test**:
   ```bash
   cargo test -p flui_rendering --lib
   ```

**Deliverable**: Pipeline phase tracking working ✅

---

### Day 3: Add Phase Assertions to RenderObject 🛡️

**Task**: Add assertions to layout/paint methods

**Steps**:

1. **Update RenderObject trait**:
   ```rust
   pub trait RenderObject {
       fn layout(&mut self, owner: &PipelineOwner, constraints: Constraints) {
           owner.assert_layout_phase();  // Add this
           self.perform_layout(constraints);
       }
       
       fn paint(&self, owner: &PipelineOwner, context: &mut PaintContext) {
           owner.assert_paint_phase();  // Add this
           self.perform_paint(context);
       }
       
       // ... rest ...
   }
   ```

2. **Update all RenderObject implementations**:
   ```bash
   # Find all implementations
   grep -r "impl RenderObject" crates/flui_rendering/src/
   
   # Update each one to call assertions
   ```

3. **Test with intentional violations**:
   ```rust
   #[test]
   #[should_panic(expected = "Expected Layout phase")]
   fn test_paint_during_layout_panics() {
       let owner = PipelineOwner::new();
       owner.set_phase(PipelinePhase::Layout);
       
       // Try to paint during layout - should panic
       owner.assert_paint_phase();
   }
   ```

**Deliverable**: All RenderObjects have phase assertions ✅

---

### Day 4: Hitbox System 📦

**Task**: Implement Bounds + ContentMask hitboxes

**Steps**:

1. **Create hitbox.rs**:
   ```rust
   // crates/flui_rendering/src/hitbox.rs
   
   use flui_types::{Rect, Path};
   
   /// Bounds represent the rectangular region an object occupies
   #[derive(Copy, Clone, Debug, PartialEq)]
   pub struct Bounds {
       pub origin: Point,
       pub size: Size,
   }
   
   impl Bounds {
       pub fn from_rect(rect: Rect) -> Self {
           Self {
               origin: rect.origin,
               size: rect.size,
           }
       }
       
       pub fn contains(&self, point: Point) -> bool {
           self.to_rect().contains(point)
       }
       
       pub fn to_rect(&self) -> Rect {
           Rect::new(self.origin, self.size)
       }
   }
   
   /// ContentMask restricts hit testing to a specific shape within bounds
   #[derive(Clone, Debug)]
   pub enum ContentMask {
       None,
       Path(Path),
       RoundedRect { corner_radius: f32 },
   }
   
   /// Hitbox combines bounds with optional content mask
   #[derive(Clone, Debug)]
   pub struct Hitbox {
       pub bounds: Bounds,
       pub content_mask: ContentMask,
   }
   
   impl Hitbox {
       pub fn new(bounds: Bounds) -> Self {
           Self {
               bounds,
               content_mask: ContentMask::None,
           }
       }
       
       pub fn with_mask(bounds: Bounds, mask: ContentMask) -> Self {
           Self {
               bounds,
               content_mask: mask,
           }
       }
       
       pub fn hit_test(&self, point: Point) -> bool {
           // First check bounds
           if !self.bounds.contains(point) {
               return false;
           }
           
           // Then check content mask
           match &self.content_mask {
               ContentMask::None => true,
               ContentMask::Path(path) => path.contains(point),
               ContentMask::RoundedRect { corner_radius } => {
                   // TODO: Check rounded rect
                   true
               }
           }
       }
   }
   ```

2. **Update PrepaintState to include Hitbox**:
   ```rust
   // In element V2 implementations
   pub struct StatelessPrepaintState {
       child_hitbox: Option<Hitbox>,  // Changed from Bounds
   }
   ```

3. **Update hit testing**:
   ```rust
   impl RenderObject {
       fn hit_test(&self, hitbox: &Hitbox, position: Offset) -> bool {
           hitbox.hit_test(position)
       }
   }
   ```

4. **Test**:
   ```rust
   #[test]
   fn test_hitbox_with_rounded_corners() {
       let bounds = Bounds::from_rect(Rect::from_xywh(0.0, 0.0, 100.0, 100.0));
       let hitbox = Hitbox::with_mask(
           bounds,
           ContentMask::RoundedRect { corner_radius: 10.0 }
       );
       
       assert!(hitbox.hit_test(Point::new(50.0, 50.0))); // Center
       // TODO: Test corners
   }
   ```

**Deliverable**: Hitbox system working ✅

---

### Day 5: Source Location + Testing 📍

**Task**: Add source location to RenderObjects

**Steps**:

1. **Update RenderObject structs**:
   ```rust
   pub struct RenderPadding {
       padding: EdgeInsets,
       child: Option<RenderId>,
       
       #[cfg(debug_assertions)]
       source_location: Option<&'static std::panic::Location<'static>>,
   }
   
   impl RenderPadding {
       #[track_caller]
       pub fn new(padding: EdgeInsets) -> Self {
           Self {
               padding,
               child: None,
               
               #[cfg(debug_assertions)]
               source_location: Some(std::panic::Location::caller()),
           }
       }
   }
   ```

2. **Update error messages**:
   ```rust
   impl RenderObject for RenderPadding {
       fn layout(&mut self, owner: &PipelineOwner, constraints: Constraints) {
           #[cfg(debug_assertions)]
           tracing::debug!(
               location = %self.source_location.unwrap(),
               "Layout RenderPadding"
           );
           
           // ... rest ...
       }
   }
   ```

3. **Run full test suite**:
   ```bash
   cargo test -p flui_rendering
   cargo test --workspace
   ```

4. **Write V2 guide**:
   ```bash
   cat > docs/RENDERING_V2_GUIDE.md << 'EOF'
   # Rendering V2 Upgrade Guide
   
   ## What Changed
   
   - Pipeline phase tracking (Idle/Layout/Compositing/Paint)
   - Phase assertions prevent API misuse
   - Hitbox system (Bounds + ContentMask)
   - Source location tracking (debug builds)
   
   ## Migration Steps
   
   [... full guide ...]
   EOF
   ```

**Deliverable**: flui_rendering V2 complete ✅

---

### Week 3 Deliverables

- ✅ Pipeline phase tracking (4 phases)
- ✅ Phase guard assertions (#[track_caller])
- ✅ Hitbox system (Bounds + ContentMask)
- ✅ Source location for RenderObjects
- ✅ Tests passing
- 📚 Migration guide written

**Metrics**:
- **API coverage**: 100% of render objects
- **Test pass rate**: >95%
- **Documentation**: Complete migration guide

---

## Week 4: Integration + Polish

**Goal**: Ensure everything works together

**Risk**: Low - validation and testing

---

### Day 1-2: Integration Testing 🧪

**Task**: End-to-end tests

**Steps**:

1. **Create integration tests**:
   ```rust
   // tests/integration/full_pipeline_test.rs
   
   #[test]
   fn test_view_to_render_pipeline() {
       // 1. Create View tree
       let view = Container::new()
           .padding(EdgeInsets::all(10.0))
           .child(Text::new("Hello"));
       
       // 2. Build Element tree
       let mut build_owner = BuildOwner::new();
       let root_element = build_owner.build_root(view);
       
       // 3. Create RenderObject tree
       let mut pipeline = PipelineOwner::new();
       root_element.create_render_object(&mut pipeline);
       
       // 4. Layout
       let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
       pipeline.flush_pipeline();
       
       // 5. Paint
       let scene = pipeline.flush_pipeline();
       
       // 6. Verify
       assert!(scene.layers.len() > 0);
   }
   ```

2. **Multi-window test**:
   ```rust
   #[test]
   fn test_multi_window() {
       let app = WidgetsFlutterBinding::instance();
       
       let window1 = app.create_window();
       let window2 = app.create_window();
       
       // Build in both windows
       // Verify isolation
   }
   ```

3. **Gesture integration test**:
   ```rust
   #[test]
   fn test_gesture_routing() {
       // Create UI with buttons
       // Record gesture
       // Replay gesture
       // Verify events fired
   }
   ```

**Deliverable**: Integration tests passing ✅

---

### Day 3: Performance Benchmarking 📊

**Task**: Benchmark and optimize

**Steps**:

1. **RefCell vs RwLock benchmark** (ADR-007):
   ```rust
   // benches/refcell_vs_rwlock.rs
   
   use criterion::{black_box, criterion_group, criterion_main, Criterion};
   
   fn benchmark_refcell(c: &mut Criterion) {
       c.bench_function("app_state_refcell", |b| {
           let app = AppWithRefCell::new();
           b.iter(|| {
               // Simulate frame
               app.borrow_mut().update();
               black_box(app.borrow().state());
           });
       });
   }
   
   fn benchmark_rwlock(c: &mut Criterion) {
       c.bench_function("app_state_rwlock", |b| {
           let app = AppWithRwLock::new();
           b.iter(|| {
               // Simulate frame
               app.write().update();
               black_box(app.read().state());
           });
       });
   }
   
   criterion_group!(benches, benchmark_refcell, benchmark_rwlock);
   criterion_main!(benches);
   ```

2. **Run benchmarks**:
   ```bash
   cargo bench --bench refcell_vs_rwlock
   ```

3. **Phase tracking overhead**:
   ```rust
   // Measure with and without phase tracking
   cargo bench --bench pipeline_overhead
   ```

4. **Document results**:
   ```bash
   cat > docs/PERFORMANCE_RESULTS.md << 'EOF'
   # Performance Benchmark Results
   
   ## RefCell vs RwLock (ADR-007)
   
   - RefCell: 120ns per access
   - RwLock: 145ns per access
   - **Recommendation**: RefCell for single-threaded UI
   
   ## Phase Tracking Overhead
   
   - Without tracking: 1.2ms per frame
   - With tracking (release): 1.21ms per frame (+0.8%)
   - With tracking (debug): 1.35ms per frame (+12.5%)
   - **Recommendation**: Keep (minimal overhead in release)
   
   [... more results ...]
   EOF
   ```

**Deliverable**: Performance benchmarks + decisions ✅

---

### Day 4: Documentation + Examples 📚

**Task**: Complete documentation

**Steps**:

1. **Update CLAUDE.md**:
   ```markdown
   ## V2 Enhancements (2026-01)
   
   FLUI now includes GPUI-inspired enhancements:
   
   ### Element V2 (flui-view)
   - Associated types for type-safe state
   - Three-phase lifecycle
   - Source location tracking
   
   ### Pipeline V2 (flui_rendering)
   - Pipeline phase tracking
   - Phase guard assertions
   - Hitbox system
   - Source location for RenderObjects
   
   See docs/VIEW_V2_GUIDE.md and docs/RENDERING_V2_GUIDE.md for details.
   ```

2. **Create examples**:
   ```bash
   # Update all examples to use V2 APIs
   mkdir -p examples/v2/
   cp examples/* examples/v2/
   # Update to use ElementV2, PipelinePhase, etc.
   ```

3. **Write migration guide**:
   ```bash
   cat > docs/MIGRATION_V1_TO_V2.md << 'EOF'
   # Migration Guide: V1 → V2
   
   ## Overview
   
   This guide helps you migrate from FLUI V1 to V2.
   
   ## Breaking Changes
   
   ### Element Trait
   
   [... detailed guide ...]
   
   ## Non-Breaking Changes
   
   ### Pipeline Phase Tracking
   
   [... guide ...]
   
   ## Timeline
   
   - V1 deprecated: 2026-02-01
   - V1 removed: 2026-06-01
   EOF
   ```

4. **Update README**:
   ```markdown
   ## Features
   
   - ✅ GPUI-inspired architecture with type-safe state
   - ✅ Three-phase element lifecycle
   - ✅ Pipeline phase tracking with safety guards
   - ✅ Comprehensive gesture recognition
   - ✅ Type-safe arity system
   - ✅ Source location tracking (debug builds)
   ```

**Deliverable**: Documentation complete ✅

---

### Day 5: Polish + Release Prep 🎁

**Task**: Final polish and prepare for release

**Steps**:

1. **Run lints**:
   ```bash
   cargo clippy --workspace -- -D warnings
   cargo fmt --all
   ```

2. **Update CHANGELOG**:
   ```markdown
   # Changelog
   
   ## [0.2.0] - 2026-02-XX
   
   ### Added
   - GPUI-inspired element V2 with associated types
   - Three-phase lifecycle (request_layout → prepaint → paint)
   - Pipeline phase tracking (Idle/Layout/Compositing/Paint)
   - Hitbox system (Bounds + ContentMask)
   - Source location tracking in debug builds
   
   ### Deprecated
   - Old Element trait (use ElementV2)
   
   ### Changed
   - PipelineOwner now tracks current phase
   ```

3. **Create release checklist**:
   ```bash
   cat > docs/RELEASE_CHECKLIST.md << 'EOF'
   # Release Checklist
   
   - [ ] All tests passing
   - [ ] Benchmarks run
   - [ ] Documentation updated
   - [ ] Examples updated
   - [ ] CHANGELOG.md updated
   - [ ] Version bumped to 0.2.0
   - [ ] Git tag created
   - [ ] crates.io publish (if applicable)
   EOF
   ```

4. **Final test run**:
   ```bash
   cargo test --workspace --release
   cargo build --workspace --release
   ```

**Deliverable**: Ready for release ✅

---

### Week 4 Deliverables

- ✅ Integration tests passing
- ✅ Performance benchmarks complete
- ✅ ADR-007 (RefCell vs RwLock) decided
- ✅ Documentation complete
- ✅ Examples updated
- 📦 Release 0.2.0 ready

---

## Timeline Summary

| Week | Phase | Focus | Deliverable |
|------|-------|-------|-------------|
| **1** | Restore | Re-enable crates | Working workspace |
| **2** | View V2 | GPUI patterns (flui-view) | ElementV2 complete |
| **3** | Rendering V2 | GPUI patterns (flui_rendering) | Pipeline V2 complete |
| **4** | Polish | Integration + docs | Release 0.2.0 |

**Total Time**: 4 weeks (20 working days)

---

## Risk Mitigation

### High-Risk Items

1. **Week 1 compilation issues**
   - **Mitigation**: Document issues daily, prioritize critical fixes
   - **Fallback**: Keep some crates disabled if blocking

2. **Element V2 API design**
   - **Mitigation**: Prototype first (Day 1), review before implementation
   - **Fallback**: Keep Element V1 alongside V2 during transition

3. **Performance regression**
   - **Mitigation**: Benchmark early (Week 4 Day 3)
   - **Fallback**: Feature flags to disable phase tracking

### Medium-Risk Items

1. **Test failures after migration**
   - **Mitigation**: Fix progressively, allow <95% pass rate initially
   - **Fallback**: Mark flaky tests as `#[ignore]` temporarily

2. **Documentation gaps**
   - **Mitigation**: Write docs alongside code
   - **Fallback**: Community can help with examples

---

## Success Criteria

### Week 1
- ✅ All core crates compile
- ✅ >80% tests pass
- 📋 Issue list created

### Week 2
- ✅ ElementV2 trait implemented
- ✅ StatelessElement/StatefulElement migrated
- ✅ >95% flui-view tests pass

### Week 3
- ✅ PipelinePhase tracking works
- ✅ Hitbox system implemented
- ✅ >95% flui_rendering tests pass

### Week 4
- ✅ Integration tests pass
- ✅ Benchmarks complete
- ✅ Documentation complete
- 📦 Release 0.2.0

---

## Month 2: FLUI V3 (Reactive Patterns)

> **NEW**: Extended migration to include reactive patterns from Xilem, Iced, Druid  
> **See**: `OTHER_FRAMEWORKS_ANALYSIS.md` for rationale

After V2 (GPUI patterns), we add reactive patterns for best-in-class architecture.

---

### Week 5-6: Lens Pattern (Druid-inspired)

**Goal**: Type-safe, composable data access

**Day 1: Design**
1. Read `OTHER_FRAMEWORKS_ANALYSIS.md` Section 3
2. Design Lens trait
3. Plan derive macro structure

**Day 2-3: Implementation**
```rust
// Add to flui-view/src/lens.rs

pub trait Lens<T, U>: Clone + 'static {
    fn get<'a>(&self, data: &'a T) -> &'a U;
    fn get_mut<'a>(&self, data: &'a mut T) -> &'a mut U;
    
    fn then<V>(self, other: impl Lens<U, V>) -> Then<Self, V>;
}
```

**Day 4-5: Derive Macro**
- Create `flui-lens-derive` crate
- Implement `#[derive(Lens)]`
- Generate field lenses

**Day 6-7: LensView Widget**
```rust
pub struct LensView<L, V, T, U> {
    lens: L,
    child: V,
    _phantom: PhantomData<(T, U)>,
}
```

**Day 8-10: Testing + Docs**
- Unit tests for lens composition
- Integration with existing View system
- Documentation + examples

**Deliverable**: Lens pattern working ✅

---

### Week 7-8: Elm Architecture (Iced-inspired)

**Goal**: Message-based reactive updates

**Day 1: Design**
1. Read `OTHER_FRAMEWORKS_ANALYSIS.md` Section 2
2. Design MessageView trait
3. Plan message dispatcher

**Day 2-4: MessageView Trait**
```rust
// Add to flui-view/src/message.rs

pub trait MessageView: Sized + 'static {
    type State: 'static;
    type Message: Clone + 'static;
    
    fn create_state(&self) -> Self::State;
    
    fn update(
        &self,
        state: &mut Self::State,
        message: Self::Message,
    ) -> UpdateResult;
    
    fn view(
        &self,
        state: &Self::State,
    ) -> impl IntoView<Self::Message>;
}

pub enum UpdateResult {
    None,
    RequestRebuild,
    Command(Box<dyn Future<Output = Message>>),
}
```

**Day 5-7: Message Dispatcher**
```rust
// Add to flui-view/src/owner.rs

pub struct MessageDispatcher<M> {
    sender: mpsc::UnboundedSender<M>,
    receiver: mpsc::UnboundedReceiver<M>,
}

impl BuildOwner {
    pub fn process_messages(&mut self) {
        for message in self.dispatcher.drain() {
            // Route to element
            // Call update()
            // Mark dirty if needed
        }
    }
}
```

**Day 8-10: Integration + Examples**
- Integrate with Element tree
- TodoMVC example
- Migration guide (StatefulView → MessageView)

**Deliverable**: Elm architecture working ✅

---

### Week 9: Adapt Nodes (Xilem-inspired)

**Goal**: Type-safe component composition

**Day 1-2: Design + Implementation**
```rust
// Add to flui-view/src/adapt.rs

pub struct AdaptView<P, C, StateFn, MsgFn> {
    child: C,
    state_transform: StateFn,
    message_transform: MsgFn,
    _phantom: PhantomData<P>,
}

impl<P, C, StateFn, MsgFn> AdaptView<P, C, StateFn, MsgFn>
where
    C: MessageView,
    StateFn: Fn(&mut P) -> &mut C::State + Clone + 'static,
    MsgFn: Fn(C::Message) -> ParentMessage + Clone + 'static,
{
    pub fn new(
        child: C,
        state_transform: StateFn,
        message_transform: MsgFn,
    ) -> Self { /* ... */ }
}
```

**Day 3-4: Integration**
- Integrate with Lens (Week 5-6)
- Integrate with Messages (Week 7-8)
- Test composition

**Day 5: Complete Example**
```rust
// Example combining all V3 patterns

#[derive(Lens)]  // Week 5-6
struct AppState {
    counter: i32,
}

#[derive(Clone)]
enum AppMessage {  // Week 7-8
    Increment,
}

impl MessageView for App {  // Week 7-8
    fn view(&self, state: &AppState) -> impl View {
        AdaptView::new(  // Week 9
            CounterView::new(),
            AppState::counter,  // Lens
            |msg| AppMessage::Increment,  // Adapt
        )
    }
}
```

**Deliverable**: Complete reactive architecture ✅

---

### Week 10: Polish + Release 0.3.0

**Day 1-2: Documentation**
- Update CLAUDE.md
- Write V3 guide
- API documentation

**Day 3: Examples**
- TodoMVC (Elm architecture)
- Counter with Lens
- Composition demo

**Day 4: Benchmarks**
- Lens overhead
- Message dispatch performance
- Compare with V2

**Day 5: Release**
- Update CHANGELOG
- Version bump to 0.3.0
- Create Git tag
- **Release 0.3.0** 🎉

**Deliverable**: FLUI 0.3.0 with reactive patterns ✅

---

## V3 Success Criteria

### Week 6 (Lens Complete)
- ✅ Lens trait implemented
- ✅ Derive macro working
- ✅ LensView widget
- ✅ Tests passing

### Week 8 (Messages Complete)
- ✅ MessageView trait implemented
- ✅ Message dispatcher working
- ✅ Integration with BuildOwner
- ✅ TodoMVC example

### Week 10 (V3 Complete)
- ✅ All V3 patterns working together
- ✅ Complete reactive example
- ✅ Documentation complete
- 📦 Release 0.3.0

---

## Month 3+: Advanced Features

### Week 11-12: Command System
- Async effects in update()
- Command executor
- Integration with tokio

### Week 13: Subscription System
- Long-running listeners
- Keyboard, timer subscriptions
- Cleanup on unmount

### Week 14: Dev Tools
- Time-travel debugging
- State inspector
- Message logger

**Deliverable**: FLUI 1.0.0 🚀

---

## Post-1.0 Tasks

### Widget Library
- Re-enable flui_widgets
- Migrate widgets to V2/V3 APIs
- Add new widgets (ListView, GridView, etc.)

### Examples + Apps
- Create example apps
- Build first real application
- Performance profiling

### Platform Integration
- Complete flui-platform implementations
- Android support
- iOS support (if applicable)

---

## Conclusion

**Bottom Line (Updated with V3)**:

### V2 (Month 1)
- ✅ **4 weeks** to GPUI-enhanced architecture
- ✅ **Low risk** - enhancing existing code
- ✅ **High value** - production patterns from Zed
- ✅ **Deliverable**: Release 0.2.0

### V3 (Month 2)
- ✅ **5 weeks** to reactive architecture
- ✅ **Medium risk** - new patterns, but well-proven
- ✅ **Very high value** - best-in-class from Xilem/Iced/Druid
- ✅ **Deliverable**: Release 0.3.0

### Total Timeline
- **Month 1**: GPUI V2 (4 weeks)
- **Month 2**: Reactive V3 (5 weeks)
- **Month 3**: Advanced features + 1.0 (4 weeks)
- **Total**: **3 months** to production-ready framework with world-class architecture

**Recommendation**: Start Week 1 of V2 migration! 🚀

**Next Steps**:

1. **This Week**: Review all documents
   - `MIGRATION_STRATEGY.md` (this file)
   - `OTHER_FRAMEWORKS_ANALYSIS.md` (V3 rationale)
   - `ARCHITECTURE_DECISIONS.md` (ADR-008, ADR-009, ADR-010)

2. **Week 1**: Start V2 migration
   - Create Git branch: `feature/gpui-v2-migration`
   - Follow Week 1 Day 1: Re-enable foundation crates
   - Track progress daily

3. **Month 2**: Start V3 (after 0.2.0 release)
   - Create branch: `feature/reactive-v3`
   - Follow Week 5-10 plan
   - Release 0.3.0

4. **Month 3**: Polish + 1.0
   - Advanced features
   - Performance optimization
   - Release 1.0.0

**Questions/Concerns**: Document in `docs/plans/MIGRATION_QUESTIONS.md`

**Status**: Ready to begin V2, V3 planned ✅
