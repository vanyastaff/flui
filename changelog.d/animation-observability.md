### Added

- Opt-in animation transition diagnostics and CPU spans with `flui-animation/diagnostics`, forwarded as `animation-diagnostics` by the SDK and facade. Diagnostics correlate controller lifetimes and run outcomes without adding observer callbacks.
- Devtools profiling of animation registry ticks, source sampling and listener/future delivery. The `profiling` feature enables animation instrumentation; timings are inclusive and restricted to the owning scheduler frame.

### Fixed

- Include headless animation sampling inside its scheduler frame so actual animation work reaches the frame profiler.
