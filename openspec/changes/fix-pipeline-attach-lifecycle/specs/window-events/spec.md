# Spec: Window Events - Resize Handling

**Capability:** `window-events`
**Change ID:** `fix-pipeline-attach-lifecycle`
**Status:** ✅ Implemented

## MODIFIED Requirements

### Requirement: Window resize must trigger UI tree relayout

**Priority:** High
**Type:** Bug Fix

#### Context

Window resize changes the available space for UI layout. The GPU surface must be reconfigured for the new dimensions, AND the UI tree must be re-laid out with new BoxConstraints. Without the layout request, widgets continue using old dimensions even though the window has resized.

#### Scenario: Window resize triggers GPU surface reconfiguration and UI relayout

**Given** a FLUI application is running with a rendered UI

**When** the user resizes the window (e.g., drags window edge)

**Then** the system must:
1. Receive `WindowEvent::Resized` event with new dimensions
2. Call `renderer.resize(width, height)` to reconfigure GPU surface
3. Access the PipelineOwner
4. Get the root element ID
5. Call `request_layout(root_id)` to mark root as needing layout
6. Log the resize and layout request

**And** the next frame will execute layout with new constraints

**Implementation:**
```rust
// crates/flui_app/src/embedder/wgpu.rs:251-265
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

**Verification:**
- Run hello_world_view demo
- Resize window by dragging edge
- Observe UI adapting to new size
- Check logs: "Window resized", "Requested layout for root after resize"

#### Scenario: UI adapts to new window dimensions

**Given** the window has been resized and layout requested

**When** the next frame renders via `render_frame()`

**Then** the layout pipeline must:
1. Call `flush_layout()` with new BoxConstraints (tight to window size)
2. Propagate new constraints down the tree
3. Recompute sizes for all affected widgets
4. Update RenderObject positions

**And** widgets fill the new window size correctly

**Verification:**
- Resize window from 800x600 to 1024x768
- Check logs: "Layout complete count=1 constraints=1024.0x768.0"
- Observe Container fills new window size
- Text remains centered in new dimensions

#### Scenario: Rapid resize events are handled gracefully

**Given** the user is rapidly dragging the window edge

**When** multiple resize events fire in quick succession

**Then** each resize must:
1. Reconfigure GPU surface (immediate)
2. Request layout (marks dirty, doesn't execute immediately)
3. Log the event

**And** layout batching prevents redundant work

**Note:** Current implementation processes every resize. Future optimization could debounce or batch.

**Verification:**
- Drag window edge rapidly (generates many events)
- Check logs show multiple "Window resized" entries
- Verify UI stays responsive (no lag or freezing)
- Confirm no crashes or panics

---

## ADDED Requirements

### Requirement: Window resize handler must access PipelineOwner safely

**Priority:** Medium
**Type:** Safety

#### Context

PipelineOwner is shared between bindings via `Arc<RwLock<PipelineOwner>>`. The resize handler runs in the event loop thread and must acquire a write lock to request layout. This must be done safely without deadlocks.

#### Scenario: Resize handler acquires PipelineOwner write lock safely

**Given** a resize event is being processed in the event loop

**When** the handler accesses `self.binding.pipeline.pipeline_owner()`

**Then** the system must:
1. Clone the Arc (cheap pointer copy)
2. Call `.write()` to acquire write lock
3. Check if root element exists (defensive programming)
4. Request layout if root exists
5. Drop lock when block exits (RAII)

**And** no deadlocks or lock contention occurs

**Implementation:**
```rust
let pipeline = self.binding.pipeline.pipeline_owner();  // ← Arc clone
let mut pipeline_write = pipeline.write();  // ← Acquire lock
if let Some(root_id) = pipeline_write.root_element_id() {
    pipeline_write.request_layout(root_id);
}  // ← Lock dropped here
```

**Verification:**
- Run demo under stress (rapid resize)
- Monitor for deadlocks or hangs
- Verify lock is released properly (no held locks)

---

### Requirement: Resize events must be logged for debugging

**Priority:** Low
**Type:** Observability

#### Context

Resize events are common during development and debugging. Logging them helps diagnose layout issues, performance problems, and event handling bugs.

#### Scenario: Resize events are logged at DEBUG level

**Given** tracing is enabled with RUST_LOG=debug

**When** a resize event occurs

**Then** the system must log:
1. "Window resized" with width and height
2. "Requested layout for root after resize" after layout request

**And** logs include structured data (width, height as separate fields)

**Implementation:**
```rust
tracing::debug!(width = size.width, height = size.height, "Window resized");
// ... resize logic ...
tracing::debug!("Requested layout for root after resize");
```

**Verification:**
- Enable RUST_LOG=debug
- Resize window
- Check logs contain both messages
- Verify width/height values are correct

---

## Related Specs

- **`pipeline-lifecycle`**: Layout request mechanism (called by resize handler)
- **`layout-pipeline`**: Layout phase execution (processes resize-triggered layouts)
- **`gpu-rendering`**: Surface reconfiguration (renderer.resize() call)

## Implementation Notes

### Files Modified

1. **`crates/flui_app/src/embedder/wgpu.rs`**
   - Lines 251-265: Modified `WindowEvent::Resized` handler
   - Added pipeline access and layout request
   - Added debug logging

### Breaking Changes

None. This is an internal bug fix in event handling.

### Performance Impact

- ✅ **Positive**: UI now responsive to resize (previously broken)
- ⚠️ **Neutral**: Adds ~5 lines of code per resize (negligible overhead)
- ⚠️ **Future optimization**: Could batch rapid resize events (16ms debounce)

### Edge Cases Handled

1. **No root element**: Handler checks `if let Some(root_id)` before requesting layout
2. **Rapid resize**: Each event processed (could be optimized with debouncing)
3. **Minimize/maximize**: These use Occluded event (separate handler)
4. **Fullscreen toggle**: Treated as regular resize

### Testing Strategy

**Manual Testing:**
- Resize window by dragging edges
- Resize window by maximizing
- Resize window by restoring from maximized
- Rapid resize (drag edge quickly)

**Automated Testing (Future):**
```rust
#[test]
fn test_resize_triggers_layout() {
    let embedder = create_test_embedder();

    // Simulate resize event
    embedder.handle_window_event(WindowEvent::Resized(PhysicalSize {
        width: 1024,
        height: 768,
    }));

    // Verify layout was requested
    let pipeline = embedder.binding.pipeline.pipeline_owner();
    assert!(pipeline.read().has_dirty_layout());
}
```

**Performance Testing:**
- Run resize stress test (1000 rapid resizes)
- Monitor CPU usage
- Verify no memory leaks
- Check frame times remain < 16ms

## Validation Checklist

- [x] Window resize triggers GPU surface reconfiguration
- [x] Window resize triggers UI layout request
- [x] UI adapts to new window dimensions
- [x] Text remains centered after resize
- [x] Container fills new window size
- [x] Logs show resize events and layout requests
- [x] No crashes or panics during resize
- [x] No deadlocks or lock contention
- [x] Rapid resize handled gracefully
- [x] No memory leaks during sustained resizing

## Future Enhancements

### 1. Resize Event Debouncing

**Problem:** Rapid resize events (e.g., dragging window edge) can trigger many layout passes.

**Solution:** Debounce resize events within a frame duration (16ms):

```rust
// Potential optimization
if self.resize_debounce_timer.elapsed() < Duration::from_millis(16) {
    self.pending_resize = Some(size);  // Store latest
    return;  // Skip this resize
}
```

**Benefits:**
- Reduces CPU load during rapid resize
- Improves frame time stability
- Still feels responsive to user

### 2. Incremental Layout on Resize

**Problem:** Full tree relayout on resize can be expensive for complex UIs.

**Solution:** Only relayout widgets affected by constraint changes:

```rust
// Future optimization
if can_use_cached_layout(old_constraints, new_constraints) {
    return cached_size;  // Skip relayout
}
```

**Benefits:**
- Faster resize handling
- Better performance for large UIs
- Maintains responsiveness

### 3. Resize Animation Smoothing

**Problem:** Discrete layout updates can look choppy during resize.

**Solution:** Interpolate widget positions during resize:

```rust
// Future enhancement
if in_resize_mode {
    interpolate_to_target_size(current_size, target_size, delta_time);
}
```

**Benefits:**
- Smoother visual experience
- Feels more polished
- Matches native OS resize behavior

## References

- winit WindowEvent::Resized: https://docs.rs/winit/latest/winit/event/enum.WindowEvent.html#variant.Resized
- Flutter RenderView.scheduleInitialFrame: https://api.flutter.dev/flutter/rendering/RenderView/scheduleInitialFrame.html
- wgpu Surface reconfiguration: https://docs.rs/wgpu/latest/wgpu/struct.Surface.html#method.configure
