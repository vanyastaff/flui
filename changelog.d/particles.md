### Added

- **Particle effects** (`flui-widgets`, through `flui` and `flui-sdk`): `ParticleEmitter`, the
  checked `ParticleConfig` (`builder()…build()` with `ParticleConfigError`, presets
  `confetti`, `sparkle`, `snow`) and `ParticleController::emit_burst`/`emit_burst_at`, which
  return how many particles they emitted. The emitter draws all its particles with one atlas
  call, gives the same frames for the same seed, stops ticking when idle, emits nothing under
  reduced motion, and is left out of semantics.
