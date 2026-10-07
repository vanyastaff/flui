### Fixed

- Gesture arena: an ignored accept candidate is dropped after the slot lock is released; pointer-level `release` releases the arena `hold` held.
- Recognizers: event timestamps stay monotonic when stamped and unstamped events mix; a restart does not overwrite a contact admitted from its own cancel callback, even on the same pointer.
