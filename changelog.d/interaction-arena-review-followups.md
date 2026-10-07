### Fixed

- Gesture arena: an ignored accept candidate is dropped after the slot lock is released and before callbacks run.
- Recognizers: event timestamps stay monotonic when stamped and unstamped events mix; drag and long-press restarts do not overwrite a contact admitted from their cancel callback.

### Removed

- `GestureArena::hold(pointer)` and `GestureArena::release(pointer)`: a pointer-keyed pair cannot tell two held generations of a reused pointer apart. Use `GestureArenaEntry::hold`/`release`, which name their generation.
