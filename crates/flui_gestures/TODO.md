# flui_gestures TODO

## Overview

Complete implementation of gesture recognition system for FLUI, inspired by Flutter's gesture system.

## Current Status

✅ Created crate structure
✅ Implemented TapGestureRecognizer (basic)
⏳ Remaining implementation below

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

### 2. GestureDetector Widget (~200 lines)

```rust
// crates/flui_gestures/src/detector.rs

pub struct GestureDetector {
    // Widget that wraps child and recognizes gestures
    // Integrates with render object for hit testing
}
```

**Key features:**
- Wraps child widget
- Creates and manages TapGestureRecognizer
- Proper hit testing via RenderObject
- Builder pattern API

### 3. Integration with flui_engine (~100 lines)

**In flui_app/src/app.rs:**
- Use flui_engine::EventRouter instead of direct egui events
- Route events through PointerRouter
- Integrate with layer hit testing

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

## Testing Plan

1. Create `examples/gesture_test.rs` with:
   - Tap detection
   - Multiple buttons
   - Nested gesture detectors

2. Update `examples/counter.rs` to use new system

3. Add unit tests for:
   - TapGestureRecognizer state machine
   - PointerRouter event dispatch
   - Hit testing integration

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

Before starting:
- ✅ Copy-based Signals working
- ✅ RebuildQueue integrated
- ✅ Counter example structure ready
- ⏳ Remove old GestureDetector from flui_widgets
