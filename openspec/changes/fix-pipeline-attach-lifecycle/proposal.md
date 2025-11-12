# Proposal: Fix PipelineOwner Root Widget Attachment Lifecycle

**Change ID:** `fix-pipeline-attach-lifecycle`
**Status:** ✅ Implemented
**Author:** Claude Code
**Date:** 2025-01-11
**Implementation Completed:** 2025-01-11

## Problem Statement

During the refactoring of `PipelineBinding` to separate concerns between `flui-app` and `flui-core`, critical bugs were introduced in the root widget attachment lifecycle. These bugs prevented the hello_world_view demo from rendering anything on screen.

### Issue 1: BuildContext Guard Lifetime

The `BuildContextGuard` was created in a block scope and dropped before child widgets could access it during recursive `into_element()` calls:

```rust
// WRONG: Guard dropped before children can access it
let element = {
    let _guard = BuildContextGuard::new(&ctx);
    widget.into_element()  // ← Children can't access BuildContext!
};
```

**Impact:** Application panicked with "No BuildContext available! Are you calling this outside of View::build()?"

### Issue 2: Missing Layout Request After Root Attachment

The `PipelineOwner::attach()` method set the root element but didn't trigger the initial layout phase:

```rust
// WRONG: No layout request
let root_id = self.set_root(element);
// Missing: self.request_layout(root_id);
```

**Impact:** Even after fixing the BuildContext panic, the application showed a blank screen because the layout and paint phases never executed.

### Issue 3: Window Resize Not Triggering UI Updates

The `WindowEvent::Resized` handler only reconfigured the GPU surface but didn't request layout for the UI tree:

```rust
// WRONG: Only resizes GPU surface
WindowEvent::Resized(size) => {
    self.renderer.resize(size.width, size.height);
    // Missing: request_layout() for root element
}
```

**Impact:** Changing window size didn't update the UI layout, leaving widgets with stale dimensions.

## Root Cause Analysis

These issues stemmed from losing critical initialization logic during the migration from the old `FluiApp::build_root()` method to the new `PipelineOwner::attach()` API:

**Old Working Code (commit ed498f9):**
```rust
fn build_root(&mut self) {
    let root_element = with_build_context(&ctx, || self.root_view.build_any());
    let root_id = self.pipeline.set_root(root_element);
    self.pipeline.request_layout(root_id);  // ← This was lost!
    tracing::info!("Root view built with ID: {:?}", root_id);
}
```

The refactoring correctly separated concerns but failed to preserve the complete initialization sequence.

## Proposed Solution

### Fix 1: Extend BuildContext Guard Lifetime

Use the `with_build_context()` closure-based approach to ensure the guard lives for the entire View → Element conversion, including recursive child builds:

```rust
// CORRECT: Guard lives for entire closure execution
let element = crate::view::with_build_context(&ctx, || widget.into_element());
```

**Why this works:**
- The `with_build_context()` function ensures `BuildContextGuard` stays alive for the closure
- Thread-local storage allows nested View builds to access the same context
- RAII pattern guarantees cleanup after all child widgets finish building

### Fix 2: Add Explicit Layout Request in attach()

Modify `PipelineOwner::attach()` to explicitly request layout after setting the root element:

```rust
// Set as pipeline root (automatically schedules initial build)
let root_id = self.set_root(element);

// CRITICAL: Request layout for the entire tree after attaching root
// Without this, the UI won't layout/paint until an external trigger
self.request_layout(root_id);

tracing::info!(root_id = ?root_id, "Root view attached to pipeline");
```

**Rationale:**
- `set_root()` only updates the element tree structure
- Layout and paint phases must be explicitly triggered
- This matches Flutter's behavior: attachment → layout → paint

### Fix 3: Request Layout on Window Resize

Add layout request to the resize event handler:

```rust
WindowEvent::Resized(size) => {
    tracing::debug!(width = size.width, height = size.height, "Window resized");

    // Delegate resize to GpuRenderer (handles surface reconfiguration)
    self.renderer.resize(size.width, size.height);

    // Request layout for the entire tree with new window size
    // This ensures UI adapts to the new dimensions
    let pipeline = self.binding.pipeline.pipeline_owner();
    let mut pipeline_write = pipeline.write();
    if let Some(root_id) = pipeline_write.root_element_id() {
        pipeline_write.request_layout(root_id);
        tracing::debug!("Requested layout for root after resize");
    }
}
```

**Why this is necessary:**
- Window size change invalidates all layout calculations
- Root element must be re-laid out with new BoxConstraints
- Child widgets inherit new constraints from root

## Success Criteria

1. ✅ **BuildContext Available**: All nested View::build() calls can access BuildContext without panics
2. ✅ **Initial Render Works**: hello_world_view demo shows blue background and white text immediately after startup
3. ✅ **Layout Phase Executes**: Logs show "Layout complete count=1" and "Paint complete count=1" on first frame
4. ✅ **Resize Responsive**: Changing window size triggers layout with new constraints
5. ✅ **Zero Performance Regression**: BuildContext is created once (not per-frame), HookContext persists across rebuilds

## Implementation Impact

### Files Modified

1. **`crates/flui_core/src/pipeline/pipeline_owner.rs`** (lines 351-382)
   - Fixed BuildContext guard lifetime using `with_build_context()`
   - Added `request_layout()` call after `set_root()`

2. **`crates/flui_app/src/embedder/wgpu.rs`** (lines 251-265)
   - Added layout request in `WindowEvent::Resized` handler

### Breaking Changes

None. These are internal bug fixes that don't affect the public API.

### Migration Guide

No migration needed. Users who were experiencing blank screens or resize issues will automatically benefit from these fixes.

## Verification

**Before Fix:**
```
thread 'main' panicked at crates\flui_core\src\view\build_context.rs:581:30:
No BuildContext available! Are you calling this outside of View::build()?
```

**After Fix 1:**
```
INFO    Root view attached to pipeline
Build complete count=1
RenderParagraph::paint: size is None, cannot paint text  ← Still broken
```

**After Fix 2:**
```
INFO    Root view attached to pipeline
Layout complete count=1
Paint complete count=1
Drawing commands text_count=1 rects=1  ← Working!
```

**After Fix 3:**
```
Window resized width=801 height=600
Requested layout for root after resize
Layout complete count=1 constraints=801.0x600.0  ← Responsive!
```

## Related Changes

- Commit 5c478ab: Initial BuildContext guard fix attempt
- Commit afc8c1d: Added request_layout() after attach
- Commit 8c42968: Used with_build_context() closure approach
- Commit 6cbf49d: Fixed window resize behavior

## Future Considerations

1. **Performance Optimization**: Consider batching layout requests during rapid resize events
2. **API Consistency**: Ensure all attachment points (not just root) follow this lifecycle pattern
3. **Testing**: Add integration tests to prevent regressions in attachment lifecycle
4. **Documentation**: Update architecture docs to emphasize the attach → layout → paint sequence

## References

- Flutter's RenderView.scheduleInitialLayout(): https://api.flutter.dev/flutter/rendering/RenderView/scheduleInitialLayout.html
- Historical code: commit ed498f9 `FluiApp::build_root()`
- `crates/flui_core/src/view/build_context.rs` (BuildContext implementation)
- `crates/flui_core/src/pipeline/pipeline_owner.rs` (Pipeline coordination)
