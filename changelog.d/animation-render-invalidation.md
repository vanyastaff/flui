### Changed

- Custom render-object attach hooks receive an owner-local invalidation handle.
  Worker invalidation retains its separate bounded, thread-safe capability.

### Fixed

- Final box and sliver opacity samples remain deliverable under external dirty
  channel backpressure or a failing frame wake, without another animation tick.
- Animated transform updates admit paint and semantics together, preserving final
  accessible bounds when the frame wake panics.
- Pipeline release revokes all local render handles before user destructors,
  including release from an active frame wake.
