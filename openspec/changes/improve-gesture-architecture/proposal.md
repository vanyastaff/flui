# Change: Improve Gesture Architecture Following Flutter Patterns

## Why

The current `flui_interaction` crate has a solid foundation but is missing key architectural patterns from Flutter's proven gesture system. These gaps cause:
- **Suboptimal gesture conflict resolution** - no eager winner pattern means slower disambiguation
- **No central coordinator** - GestureBinding is missing, making integration with rendering harder
- **Incomplete recognizer lifecycle** - state machine is partially implemented
- **Hardcoded settings** - no device-specific gesture configuration

## What Changes

### 1. Eager Winner Pattern in GestureArena
- **BREAKING**: Arena resolution behavior changes
- Add `eager_winner` field to arena entries
- When recognizer accepts while arena is open, store as eager winner
- Resolve in favor of eager winner when arena closes

### 2. GestureBinding - Central Coordinator
- New `GestureBinding` struct coordinating hit testing, routing, and arena
- Handles pointer event lifecycle from platform to recognizers
- Manages hit test caching per pointer
- Provides `handlePointerEvent()` entry point

### 3. OneSequenceGestureRecognizer Base
- New trait hierarchy for single-pointer-sequence recognizers
- `startTrackingPointer()` / `stopTrackingPointer()` lifecycle
- Automatic arena entry management
- Transform capture for coordinate conversion

### 4. PrimaryPointerGestureRecognizer with State Machine
- Formal state machine: `Ready` → `Possible` → `Accepted`/`Defunct`
- Pre/post acceptance slop tolerance
- Deadline timer support for time-based decisions (long-press)

### 5. DeviceGestureSettings
- Device-specific touch slop, pan slop, scale slop
- Runtime configuration instead of compile-time constants
- Support for different device types (touch, mouse, stylus)

### 6. Hold/Release Mechanism
- `arena.hold(pointer)` - prevent sweep while deciding
- `arena.release(pointer)` - allow deferred sweep
- Essential for long-press and other delayed gestures

## Impact

- **Affected specs**: `interaction-handlers`
- **Affected code**: 
  - `crates/flui_interaction/src/arena.rs` - eager winner, hold/release
  - `crates/flui_interaction/src/recognizers/` - new base traits
  - `crates/flui_interaction/src/binding.rs` - new file
  - `crates/flui_interaction/src/settings.rs` - new file
- **Breaking changes**: Arena resolution timing may differ slightly
