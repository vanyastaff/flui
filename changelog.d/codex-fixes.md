### Fixed

- **Turning haptics off cancels queued feedback** (`flui-interaction`): a frame-aligned haptic
  queued while haptics were on no longer plays on the next presented frame after the user turns
  them off; turning them off resolves every queued request to `HapticOutcome::Suppressed` at
  once, delivered to the presentation's observers, as hiding the window does
  ([ADR-0212](/docs/adr/ADR-0212-system-preferences-for-input.md)).
