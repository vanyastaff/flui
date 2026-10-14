### Fixed

- A `tracing` subscriber that panics while handling an animation refusal, non-finite-value or never-settles warning, or while the controller span is cloned to name it, no longer replaces the command's `AnimationError` or fails the frame; the panic is contained like every other diagnostic callout. This includes refusals with no controller (`AnimatedValue::new`, `MotionUpdate::run`).
