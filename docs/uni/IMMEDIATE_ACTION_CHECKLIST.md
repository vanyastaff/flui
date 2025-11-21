# FLUI Refactoring - Immediate Action Checklist

**Start Here! This checklist guides you through the refactoring step-by-step.**

---

## Pre-Work (30 minutes)

### Setup

- [ ] Create backup branch
  ```bash
  git checkout -b backup/pre-refactor
  git push origin backup/pre-refactor
  ```

- [ ] Create feature branch
  ```bash
  git checkout main
  git pull
  git checkout -b refactor/unified-architecture
  ```

- [ ] Run baseline tests
  ```bash
  cargo test --workspace > baseline_tests.txt
  cargo clippy --workspace > baseline_clippy.txt
  ```

- [ ] Measure baseline performance
  ```bash
  cargo build --release --example counter
  # Run and note idle CPU usage: _____% 
  # Run and note frame time: _____ms
  ```

- [ ] Read key documents
  - [ ] UNIFIED_REFACTORING_PLAN.md (this is master plan)
  - [ ] QUICK_REFERENCE.md (for lookups during work)
  - [ ] WINIT_BINDINGS_ANALYSIS.md (for bindings details)

---

## Phase 1: Frame Lifecycle (Day 1) 🔥 CRITICAL

### Step 1.1: Add begin_frame/end_frame to DesktopEmbedder (2 hours)

- [ ] Open `crates/flui_app/src/embedder/desktop.rs`

- [ ] Find `render_frame()` method (around line ~180)

- [ ] Add scheduler calls:
  ```rust
  pub fn render_frame(&mut self) {
      // ✅ ADD THIS LINE
      self.binding.scheduler.scheduler().begin_frame();
      
      // Existing code
      let constraints = BoxConstraints::tight(self.window_size);
      let scene = self.binding.draw_frame(constraints);
      
      // GPU rendering (existing)
      if let Err(err) = self.renderer.render(&scene) {
          tracing::error!("Render failed: {}", err);
      }
      
      // ✅ ADD THIS LINE
      self.binding.scheduler.scheduler().end_frame();
  }
  ```

- [ ] Test it works:
  ```bash
  RUST_LOG=flui_scheduler=debug cargo run --example counter
  # Should see: "begin_frame" and "end_frame" in logs
  ```

### Step 1.2: Add begin_frame/end_frame to AndroidEmbedder (1 hour)

- [ ] Open `crates/flui_app/src/embedder/android.rs`

- [ ] Find `render_frame()` method

- [ ] Add same scheduler calls as desktop

- [ ] Test on Android device:
  ```bash
  cargo apk run --example android_demo
  # Check logcat for "begin_frame" and "end_frame"
  ```

### Step 1.3: Verify scheduler callbacks run (1 hour)

- [ ] Run with tracing:
  ```bash
  RUST_LOG=debug cargo run --example counter
  ```

- [ ] Check logs for:
  - `[SCHEDULER] Flushed rebuild queue at frame start` ✅
  - Frame callbacks executing ✅

- [ ] Test signal updates:
  ```rust
  // In counter example, click button
  // Should see rebuild queue flush in logs
  ```

### Step 1.4: Commit Phase 1 ✅

- [ ] Run tests:
  ```bash
  cargo test --workspace
  # All tests should pass
  ```

- [ ] Commit:
  ```bash
  git add crates/flui_app/src/embedder/
  git commit -m "feat: Implement frame lifecycle (begin_frame/end_frame)
  
  - Add scheduler.begin_frame() at frame start
  - Add scheduler.end_frame() at frame end
  - Enables scheduler callbacks to run properly
  - Fixes issue P1 (Frame lifecycle not implemented)
  
  Refs: UNIFIED_REFACTORING_PLAN.md Phase 1"
  ```

---

## Phase 2: Remove PipelineBinding (Day 2 Morning) 🎯

### Step 2.1: Update AppBinding structure (2 hours)

- [ ] Open `crates/flui_app/src/binding/app_binding.rs`

- [ ] Replace existing `AppBinding` struct with refactored version from `app_binding_refactored.rs`:
  ```rust
  pub struct AppBinding {
      pipeline_owner: Arc<RwLock<PipelineOwner>>,  // ← Direct ownership
      gesture: GestureBinding,
      scheduler: SchedulerBinding,
      renderer: RendererBinding,
      needs_redraw: Arc<AtomicBool>,  // ← NEW
  }
  ```

- [ ] Update `ensure_initialized()` method:
  ```rust
  pub fn ensure_initialized() -> Arc<Self> {
      INSTANCE.get_or_init(|| {
          let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
          let needs_redraw = Arc::new(AtomicBool::new(false));
          
          let mut binding = Self {
              pipeline_owner: pipeline_owner.clone(),
              gesture: GestureBinding::new(),
              scheduler: SchedulerBinding::new(),
              renderer: RendererBinding::new(),
              needs_redraw: needs_redraw.clone(),
          };
          
          binding.wire_up(needs_redraw);
          Arc::new(binding)
      }).clone()
  }
  ```

- [ ] Add new methods:
  ```rust
  pub fn attach_root_widget<V: View + 'static>(&self, app: V) { /* ... */ }
  pub fn pipeline(&self) -> Arc<RwLock<PipelineOwner>> { /* ... */ }
  pub fn request_redraw(&self) { /* ... */ }
  pub fn needs_redraw(&self) -> bool { /* ... */ }
  pub fn mark_rendered(&self) { /* ... */ }
  pub fn draw_frame(&self, constraints: BoxConstraints) -> Scene { /* ... */ }
  ```

- [ ] Test compilation:
  ```bash
  cargo check -p flui_app
  # Will have errors - expected, fix in next steps
  ```

### Step 2.2: Update RendererBinding (1 hour)

- [ ] Open `crates/flui_app/src/binding/renderer.rs`

- [ ] Replace with refactored version from `renderer_binding_refactored.rs`:
  ```rust
  pub struct RendererBinding {
      // No fields needed!
  }
  
  impl RendererBinding {
      pub fn new() -> Self {
          Self {}
      }
      
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

### Step 2.3: Delete PipelineBinding (30 minutes)

- [ ] Delete file:
  ```bash
  git rm crates/flui_app/src/binding/pipeline.rs
  ```

- [ ] Update `crates/flui_app/src/binding/mod.rs`:
  ```rust
  // Remove these lines:
  // mod pipeline;
  // pub use pipeline::PipelineBinding;
  ```

- [ ] Test compilation:
  ```bash
  cargo check -p flui_app
  # Should compile now
  ```

### Step 2.4: Update run_app (1 hour)

- [ ] Open `crates/flui_app/src/lib.rs`

- [ ] Update imports:
  ```rust
  // Remove PipelineBinding if imported
  ```

- [ ] Update event loop:
  ```rust
  Event::AboutToWait => {
      if let Some(ref emb) = embedder {
          // ✅ CHANGE: On-demand rendering
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

### Step 2.5: Fix all compilation errors (1 hour)

- [ ] Find all `binding.pipeline.` calls:
  ```bash
  grep -rn "binding.pipeline\." crates/
  ```

- [ ] Replace with `binding.`:
  ```rust
  // Before
  binding.pipeline.attach_root_widget(app);
  
  // After
  binding.attach_root_widget(app);
  ```

- [ ] Test compilation:
  ```bash
  cargo build --workspace
  # Should compile with no errors
  ```

### Step 2.6: Run tests (30 minutes)

- [ ] Run all tests:
  ```bash
  cargo test --workspace
  ```

- [ ] Fix any failing tests

- [ ] Run examples:
  ```bash
  cargo run --example counter
  cargo run --example profile_card
  ```

### Step 2.7: Commit Phase 2 ✅

- [ ] Commit:
  ```bash
  git add -A
  git commit -m "refactor: Remove PipelineBinding layer
  
  - Move methods directly to AppBinding
  - Simplify RendererBinding (no pipeline field)
  - Add on-demand rendering (needs_redraw flag)
  - Reduce code duplication
  
  Fixes issues B1, B4, P6
  
  BREAKING CHANGE: 
  - binding.pipeline.attach_root_widget() → binding.attach_root_widget()
  - binding.pipeline.pipeline_owner() → binding.pipeline()
  
  Refs: UNIFIED_REFACTORING_PLAN.md Phase 2"
  ```

---

## Phase 3: Fix Circular References (Day 2 Afternoon) 🔄

### Step 3.1: Use Weak in scheduler callback (1 hour)

- [ ] Open `crates/flui_app/src/binding/app_binding.rs`

- [ ] Find `wire_up()` method

- [ ] Update to use Weak:
  ```rust
  fn wire_up(&self, needs_redraw: Arc<AtomicBool>) {
      // ✅ CHANGE: Use Weak
      let pipeline_weak = Arc::downgrade(&self.pipeline_owner);
      
      self.scheduler.scheduler().add_persistent_frame_callback(
          Arc::new(move |_timing| {
              // ✅ ADD: Try upgrade
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

### Step 3.2: Remove duplicate flush (30 minutes)

- [ ] Open `crates/flui_core/src/pipeline/frame_coordinator.rs`

- [ ] Find `build_frame()` method (around line 143)

- [ ] Comment out redundant flush:
  ```rust
  pub fn build_frame(...) -> Result<Scene, PipelineError> {
      loop {
          // ✅ REMOVE: Redundant flush (already done in scheduler callback)
          // self.build.flush_rebuild_queue();
          
          self.build.flush_batch();
          
          if self.build.dirty_count() == 0 {
              break;
          }
          
          self.build.rebuild_dirty_parallel(tree.clone());
      }
      
      // ... rest of method
  }
  ```

### Step 3.3: Test for memory leaks (1 hour)

- [ ] Install valgrind (if not installed):
  ```bash
  # Linux
  sudo apt install valgrind
  
  # macOS
  brew install valgrind
  ```

- [ ] Run with valgrind:
  ```bash
  cargo build --example counter
  valgrind --leak-check=full ./target/debug/examples/counter
  # Run for 30 seconds, close window
  # Check output for "definitely lost: 0 bytes"
  ```

- [ ] If leaks found, debug with:
  ```bash
  RUST_LOG=debug valgrind ./target/debug/examples/counter 2>&1 | tee leak_log.txt
  ```

### Step 3.4: Commit Phase 3 ✅

- [ ] Commit:
  ```bash
  git add -A
  git commit -m "fix: Remove circular references in scheduler callbacks
  
  - Use Weak<RwLock<PipelineOwner>> instead of Arc
  - Remove duplicate flush_rebuild_queue() from build_frame loop
  - Prevents memory leaks on shutdown
  
  Fixes issues B3, P2
  
  Refs: UNIFIED_REFACTORING_PLAN.md Phase 3"
  ```

---

## Phase 4: Consolidate Layout Marking (Day 3) 📐

### Step 4.1: Create helper method (2 hours)

- [ ] Open `crates/flui_core/src/pipeline/pipeline_owner.rs`

- [ ] Add private helper:
  ```rust
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
  }
  ```

- [ ] Update `request_layout()`:
  ```rust
  pub fn request_layout(&mut self, node_id: ElementId) {
      self.mark_layout_dirty(node_id);
  }
  ```

### Step 4.2: Update attach() to use helper (1 hour)

- [ ] Find `attach()` method (around line 200)

- [ ] Simplify using helper:
  ```rust
  pub fn attach<V: View + 'static>(&mut self, widget: V) -> ElementId {
      let element = widget.into_element();
      let root_id = self.set_root(element);
      
      // ✅ USE HELPERS: Single call each
      self.schedule_build_for(root_id, 0);
      self.mark_layout_dirty(root_id);  // ← Uses helper
      
      root_id
  }
  ```

### Step 4.3: Test layout marking (2 hours)

- [ ] Add unit test in `crates/flui_core/src/pipeline/pipeline_owner.rs`:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      
      #[test]
      fn test_mark_layout_dirty_sets_both() {
          let mut owner = PipelineOwner::new();
          
          // Create a render element
          let element = create_test_render_element();
          let id = owner.tree.write().insert(element);
          
          // Mark for layout
          owner.mark_layout_dirty(id);
          
          // Verify dirty set
          assert!(owner.coordinator.layout().is_dirty(id));
          
          // Verify RenderState flag
          let tree = owner.tree.read();
          let render_elem = tree.get(id).unwrap();
          if let Element::Render(r) = render_elem {
              assert!(r.render_state().read().needs_layout());
          } else {
              panic!("Expected Render element");
          }
      }
  }
  ```

- [ ] Run tests:
  ```bash
  cargo test -p flui_core test_mark_layout_dirty
  ```

### Step 4.4: Commit Phase 4 ✅

- [ ] Commit:
  ```bash
  git add -A
  git commit -m "refactor: Consolidate layout marking into helper method
  
  - Add mark_layout_dirty() helper
  - Sets both dirty set AND RenderState flag
  - Simplifies attach() implementation
  - Prevents bugs from missing one of the two places
  
  Fixes issue P3
  
  Refs: UNIFIED_REFACTORING_PLAN.md Phase 4"
  ```

---

## Phase 5: Add Validation to attach() (Day 4) ✅

### Step 5.1: Make attach() return Result (3 hours)

- [ ] Open `crates/flui_core/src/pipeline/pipeline_owner.rs`

- [ ] Change signature:
  ```rust
  pub fn attach<V: View + 'static>(&mut self, widget: V) 
      -> Result<ElementId, PipelineError> 
  {
      // Validate no existing root
      if self.root_mgr.has_root() {
          return Err(PipelineError::invalid_state(
              "Root widget already attached. Call teardown() first."
          )?);
      }
      
      // Build element
      let element = widget.into_element();
      
      // Set as root
      let root_id = self.set_root(element);
      
      // Schedule
      self.schedule_build_for(root_id, 0);
      self.mark_layout_dirty(root_id);
      
      Ok(root_id)
  }
  ```

### Step 5.2: Add teardown() method (1 hour)

- [ ] Add new method:
  ```rust
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
  ```

### Step 5.3: Update all callers (2 hours)

- [ ] Find all `attach()` calls:
  ```bash
  grep -rn "\.attach(" crates/
  ```

- [ ] Update each call:
  ```rust
  // Before
  let root_id = pipeline.attach(MyApp);
  
  // After
  let root_id = pipeline.attach(MyApp)?;
  // Or
  let root_id = pipeline.attach(MyApp)
      .expect("Failed to attach root widget");
  ```

- [ ] Update `AppBinding::attach_root_widget()`:
  ```rust
  pub fn attach_root_widget<V: View + 'static>(&self, app: V) {
      let element = app.into_element();
      let mut owner = self.pipeline_owner.write();
      owner.set_root(element);
      
      // Handle error
      if let Err(err) = owner.attach(app) {
          panic!("Failed to attach root widget: {}", err);
      }
      
      self.request_redraw();
  }
  ```

### Step 5.4: Add tests (2 hours)

- [ ] Test double attach fails:
  ```rust
  #[test]
  fn test_attach_twice_fails() {
      let mut owner = PipelineOwner::new();
      
      owner.attach(TestWidget1).unwrap();
      let result = owner.attach(TestWidget2);
      
      assert!(result.is_err());
  }
  ```

- [ ] Test teardown and reattach:
  ```rust
  #[test]
  fn test_teardown_and_reattach() {
      let mut owner = PipelineOwner::new();
      
      owner.attach(TestWidget1).unwrap();
      owner.teardown().unwrap();
      owner.attach(TestWidget2).unwrap();  // Should succeed
  }
  ```

### Step 5.5: Commit Phase 5 ✅

- [ ] Commit:
  ```bash
  git add -A
  git commit -m "feat: Add validation and error handling to attach()
  
  - attach() now returns Result<ElementId, PipelineError>
  - Add teardown() method for cleaning up root
  - Prevent double attach with validation
  - Enable hot reload and better testing
  
  Fixes issue P4
  
  BREAKING CHANGE: attach() signature changed
  
  Refs: UNIFIED_REFACTORING_PLAN.md Phase 5"
  ```

---

## Phase 6: Minor Cleanups (Day 5) 🧹

### Step 6.1: Remove duplicate marking (1 hour)

- [ ] Open `crates/flui_core/src/pipeline/build_pipeline.rs`

- [ ] Find duplicate mark_dirty + schedule calls

- [ ] Remove mark_dirty, keep only schedule:
  ```rust
  // Before
  component.mark_dirty();
  self.schedule(element_id, depth);
  
  // After
  self.schedule(element_id, depth);  // This marks dirty internally
  ```

### Step 6.2: Add validation for RenderElement (2 hours)

- [ ] In `build_pipeline.rs`, update `schedule()`:
  ```rust
  pub fn schedule(&mut self, element_id: ElementId, depth: usize) {
      let tree = self.tree.read();
      if let Some(element) = tree.get(element_id) {
          match element {
              Element::Component(_) | Element::Provider(_) => {
                  self.dirty_elements.push((element_id, depth));
              }
              Element::Render(_) => {
                  tracing::warn!(
                      "Attempted to schedule RenderElement for build: {:?}. \
                       RenderElements don't rebuild - use request_layout() instead.",
                      element_id
                  );
              }
          }
      }
  }
  ```

### Step 6.3: Add tests (1 hour)

- [ ] Test validation:
  ```rust
  #[test]
  fn test_schedule_render_element_warns() {
      let mut build = BuildPipeline::new();
      
      // Create render element
      let render_id = create_test_render_element();
      
      // Should warn, not panic
      build.schedule(render_id, 0);
      
      // Should not be in dirty set
      assert_eq!(build.dirty_count(), 0);
  }
  ```

### Step 6.4: Commit Phase 6 ✅

- [ ] Commit:
  ```bash
  git add -A
  git commit -m "refactor: Minor cleanups and validation
  
  - Remove duplicate marking in component rebuild
  - Add validation for RenderElement in build dirty set
  - Improve error messages and warnings
  
  Fixes issues P5, P7
  
  Refs: UNIFIED_REFACTORING_PLAN.md Phase 6"
  ```

---

## Final Steps (Day 5 Afternoon) 🎉

### Documentation

- [ ] Update `CHANGELOG.md`:
  ```markdown
  ## [0.8.0] - 2025-11-22
  
  ### Added
  - Frame lifecycle (begin_frame/end_frame) integration
  - On-demand rendering (50-100x lower idle CPU)
  - Validation in attach() method
  - Teardown support for hot reload
  
  ### Changed
  - **BREAKING**: Removed PipelineBinding layer
  - **BREAKING**: attach() now returns Result
  - Simplified bindings architecture
  - Fixed circular references in callbacks
  
  ### Fixed
  - Double flush of rebuild queue
  - Layout marking inconsistency
  - Memory leaks on shutdown
  ```

- [ ] Update README.md with performance improvements

- [ ] Update architecture docs:
  - [ ] `FINAL_ARCHITECTURE_V2.md`
  - [ ] `PIPELINE_ARCHITECTURE.md`

### Testing

- [ ] Run full test suite:
  ```bash
  cargo test --workspace
  ```

- [ ] Run all examples:
  ```bash
  cargo run --example counter
  cargo run --example profile_card
  cargo run --example hello_world_view
  ```

- [ ] Test on all platforms:
  - [ ] Windows
  - [ ] macOS
  - [ ] Linux
  - [ ] Android device

### Performance Validation

- [ ] Measure improvements:
  ```bash
  # Build release
  cargo build --release --example counter
  
  # Run and measure
  ./target/release/examples/counter
  
  # Record metrics:
  # CPU idle (before): _____%
  # CPU idle (after):  _____%  (should be <0.5%)
  # 
  # Frame time (before): _____ms
  # Frame time (after):  _____ms  (should be same or better)
  ```

### Code Quality

- [ ] Run clippy:
  ```bash
  cargo clippy --workspace -- -D warnings
  # Fix any warnings
  ```

- [ ] Run rustfmt:
  ```bash
  cargo fmt --all
  ```

- [ ] Check documentation:
  ```bash
  cargo doc --workspace --no-deps
  # Fix any warnings
  ```

### Create PR

- [ ] Push branch:
  ```bash
  git push origin refactor/unified-architecture
  ```

- [ ] Create PR with description:
  ```
  Title: Refactor unified architecture - bindings + pipeline improvements
  
  ## Summary
  
  This PR implements a comprehensive refactoring of FLUI's architecture,
  addressing 11 identified issues across the pipeline and bindings layers.
  
  ## Changes
  
  - ✅ Frame lifecycle integration (begin_frame/end_frame)
  - ✅ On-demand rendering (50-100x lower idle CPU)
  - ✅ Removed PipelineBinding layer
  - ✅ Fixed circular references
  - ✅ Consolidated layout marking
  - ✅ Added validation to attach()
  - ✅ Minor cleanups and improvements
  
  ## Breaking Changes
  
  - `binding.pipeline.attach_root_widget()` → `binding.attach_root_widget()`
  - `binding.pipeline.pipeline_owner()` → `binding.pipeline()`
  - `attach()` now returns `Result<ElementId, PipelineError>`
  
  See MIGRATION_GUIDE.md for details.
  
  ## Performance
  
  - Idle CPU usage: 5-10% → <0.5% ✅
  - Frame times: Unchanged or improved ✅
  - Memory usage: Same or lower ✅
  
  ## Testing
  
  - ✅ All tests pass
  - ✅ All examples run
  - ✅ Tested on Windows/macOS/Linux/Android
  - ✅ No clippy warnings
  - ✅ No rustdoc warnings
  
  Refs: UNIFIED_REFACTORING_PLAN.md
  ```

- [ ] Request reviews from team

---

## Troubleshooting

### If tests fail

1. Check which test failed
2. Look at error message
3. Check relevant section in UNIFIED_REFACTORING_PLAN.md
4. Fix and re-run

### If performance didn't improve

1. Verify on-demand rendering is active:
   ```bash
   RUST_LOG=flui_app=trace cargo run --example counter
   # Should see "No redraw needed (clean frame)" when idle
   ```

2. Check begin_frame/end_frame are called:
   ```bash
   RUST_LOG=flui_scheduler=debug cargo run --example counter
   # Should see frame lifecycle logs
   ```

### If memory leaks detected

1. Run with detailed leak check:
   ```bash
   valgrind --leak-check=full --show-leak-kinds=all ./target/debug/examples/counter
   ```

2. Check Weak references are used in callbacks

3. Verify no circular Arc references

---

## Success Checklist ✅

- [ ] All 6 phases completed
- [ ] All tests pass
- [ ] All examples run
- [ ] Performance improved
- [ ] Documentation updated
- [ ] PR created and reviewed
- [ ] Migration guide provided

---

## Time Tracking

| Phase | Estimated | Actual | Notes |
|-------|-----------|--------|-------|
| Phase 1 | 1 day | _____ | |
| Phase 2 | 6 hours | _____ | |
| Phase 3 | 3 hours | _____ | |
| Phase 4 | 6 hours | _____ | |
| Phase 5 | 8 hours | _____ | |
| Phase 6 | 4 hours | _____ | |
| Testing/Docs | 8 hours | _____ | |
| **Total** | **2-3 weeks** | _____ | |

---

## Notes

Use this space for notes during implementation:

```


```

---

**Good luck! 🚀**

Remember:
- Take breaks between phases
- Run tests frequently
- Commit after each phase
- Ask for help if stuck
- Reference UNIFIED_REFACTORING_PLAN.md for details
