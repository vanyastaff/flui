# flui_gestures TODO

## Overview

Complete implementation of gesture recognition system for FLUI, inspired by Flutter's gesture system.

## Current Status (2025-11-06)

✅ Created crate structure
✅ Implemented TapGestureRecognizer (full featured)
✅ Implemented GestureDetector widget with RenderPointerListener integration
✅ Integrated with flui_engine::EventRouter for proper hit testing
✅ Removed old global-registry-based GestureDetector from flui_widgets
✅ **Working in production** - Counter example uses new system successfully

**Architecture:** GestureDetector → RenderPointerListener → PointerListenerLayer → EventRouter → Hit Testing

⏳ Additional recognizers (Drag, LongPress, Scale) - deferred for future

## To Implement

### 1. PointerRouter (~150 lines)

```rust
// crates/flui_gestures/src/pointer_router.rs

pub struct PointerRouter {
    // Route pointer events to appropriate gesture recognizers
    // Handle hit testing integration
    // Manage pointer capture/release
}
```

**Key features:**
- Event routing to recognizers
- Hit testing integration with flui_engine::EventRouter
- Pointer capture for drag gestures
- Multi-touch support

### 2. GestureDetector Widget ✅ DONE

**Implemented at:** `crates/flui_gestures/src/detector.rs` (240 lines)

**Features:**
- ✅ Wraps child widget
- ✅ Builder pattern API
- ✅ Callbacks: `on_tap`, `on_tap_down`, `on_tap_up`, `on_tap_cancel`
- ✅ Uses RenderPointerListener for proper hit testing
- ✅ Integrates with PointerListenerLayer
- ✅ Thread-safe Arc callbacks
- ✅ Full unit tests

### 3. Integration with flui_engine ✅ DONE

**Completed:**
- ✅ flui_app/src/app.rs uses flui_engine::EventRouter
- ✅ EventRouter performs layer hit testing
- ✅ PointerListenerLayer registers callbacks
- ✅ Events dispatched to correct widgets based on position
- ✅ Working with counter example buttons

### 4. Additional Recognizers (Future)

- `DragGestureRecognizer` - drag gestures
- `ScaleGestureRecognizer` - pinch/zoom
- `LongPressGestureRecognizer` - long press
- `PanGestureRecognizer` - pan gestures

### 5. GestureArena (Future)

```rust
// crates/flui_gestures/src/arena.rs

pub struct GestureArena {
    // Resolve conflicts between competing recognizers
    // E.g., tap vs drag on same widget
}
```

## Testing Status

✅ **Production Testing:**
- Counter example (`examples/counter.rs`) working perfectly
- Three buttons (Increment, Decrement, Reset) all respond correctly
- Proper hit testing - only clicked button responds
- Performance: 60 FPS, rebuilds only on interaction

✅ **Unit Tests:**
- GestureDetector builder tests
- GestureDetector callback tests

⏳ **Future Testing:**
- Gesture test example with nested detectors
- TapGestureRecognizer state machine tests
- Multi-touch scenarios

## Architecture Notes

### Event Flow

```
User Input (egui)
    ↓
flui_engine::EventRouter (hit testing)
    ↓
flui_gestures::PointerRouter (dispatch)
    ↓
GestureRecognizer (recognition)
    ↓
Callback (user code)
    ↓
Signal::update() (state change)
    ↓
RebuildQueue (automatic rebuild)
```

### Key Design Decisions

1. **Use flui_engine::EventRouter** - Don't reinvent hit testing
2. **Layer-based hit testing** - Proper spatial hierarchy
3. **Recognizer composition** - Multiple recognizers per detector
4. **Arena for conflicts** - Flutter-style gesture disambiguation

## References

- Flutter gesture system: https://api.flutter.dev/flutter/gestures/gestures-library.html
- Current temporary implementation: `crates/flui_widgets/src/gestures/`
- Event types: `crates/flui_types/src/events.rs`
- EventRouter: `crates/flui_engine/src/event_router.rs`

## Estimated Effort

- PointerRouter: 2-3 hours
- GestureDetector widget: 2-3 hours
- Integration & testing: 2-3 hours
- **Total: 6-9 hours**

## Dependencies

All dependencies met:
- ✅ Copy-based Signals working
- ✅ RebuildQueue integrated
- ✅ Counter example working with new system
- ✅ Old GestureDetector removed from flui_widgets
- ✅ flui_widgets re-exports from flui_gestures
