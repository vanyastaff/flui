### Added

- `EngineError::ReadbackTimedOut`: `HeadlessRenderer::render_layer_tree` waits at most 60 s for its pixel readback and reports a stalled GPU as this (fatal) error instead of blocking forever.

### Fixed

- The engine's coverage- and gradient-blend readback tests no longer hang on Windows: the feature-reduced twin renderer they compare against now comes from the first renderer's adapter instead of a `wgpu::Instance` of its own, whose teardown intermittently blocked inside the driver.
