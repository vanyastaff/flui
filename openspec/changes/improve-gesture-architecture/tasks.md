# Tasks: Gesture Architecture Improvements

## Pre-Implementation

- [ ] Review proposal and design docs
- [ ] Verify test coverage baseline
- [ ] Check dependent crates

## Implementation Tasks

### Phase 1: Arena Enhancements (Backward Compatible)

- [ ] Add `eager_winner` field to `ArenaEntry`
- [ ] Implement eager winner resolution in `GestureArena::resolve()`
- [ ] Add `is_held` and `has_pending_sweep` fields to `ArenaEntry`
- [ ] Implement `GestureArena::hold()` method
- [ ] Implement `GestureArena::release()` method
- [ ] Update `GestureArena::sweep()` for hold/release logic
- [ ] Add tests for eager winner pattern
- [ ] Add tests for hold/release mechanism

### Phase 2: GestureSettings (Backward Compatible)

- [ ] Create `crates/flui_interaction/src/settings.rs`
- [ ] Implement `GestureSettings` struct with defaults
- [ ] Add device-specific constructors (`for_device()`)
- [ ] Add settings getters (touch_slop, pan_slop, etc.)
- [ ] Update recognizers to use GestureSettings
- [ ] Add tests for device-specific settings

### Phase 3: Recognizer Trait Hierarchy

- [ ] Create `crates/flui_interaction/src/recognizers/one_sequence.rs`
- [ ] Define `OneSequenceGestureRecognizer` trait
- [ ] Implement `startTrackingPointer()` / `stopTrackingPointer()`
- [ ] Create `crates/flui_interaction/src/recognizers/primary_pointer.rs`
- [ ] Define `GestureRecognizerState` enum
- [ ] Define `PrimaryPointerGestureRecognizer` trait
- [ ] Implement state machine transitions
- [ ] Add deadline timer support
- [ ] Add tests for state machine

### Phase 4: Migrate Existing Recognizers

- [ ] Migrate `TapGestureRecognizer` to PrimaryPointerGestureRecognizer
- [ ] Migrate `LongPressGestureRecognizer` to PrimaryPointerGestureRecognizer
- [ ] Migrate `DragGestureRecognizer` to OneSequenceGestureRecognizer
- [ ] Migrate `ForcePressGestureRecognizer` to PrimaryPointerGestureRecognizer
- [ ] Update all recognizer tests

### Phase 5: GestureBinding

- [ ] Create `crates/flui_interaction/src/binding.rs`
- [ ] Implement `GestureBinding` struct
- [ ] Add hit test caching with `DashMap`
- [ ] Implement `handle_pointer_event()` entry point
- [ ] Wire up PointerRouter and GestureArena
- [ ] Add integration tests

### Phase 6: Documentation & Cleanup

- [ ] Update module documentation
- [ ] Add examples in doc comments
- [ ] Update CHANGELOG.md
- [ ] Run full test suite
- [ ] Run clippy and fix warnings

## Post-Implementation

- [ ] Performance benchmark comparison
- [ ] Update spec deltas
- [ ] Archive change proposal

## Verification

```bash
# Run all interaction tests
cargo test -p flui_interaction

# Run with specific feature flags if needed
cargo test -p flui_interaction --all-features

# Clippy check
cargo clippy -p flui_interaction -- -D warnings
```
