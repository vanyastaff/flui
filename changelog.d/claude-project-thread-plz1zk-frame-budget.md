### Fixed

- A GPU resource request that cannot fit beside what its own frame already holds now fails with `EngineError::PreparedResourceLimit`, whatever earlier work is still in flight. Before, it could fail with `EngineError::GpuBackpressure`, so the caller retried a frame that could never fit.
