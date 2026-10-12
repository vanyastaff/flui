### Added

- Opt-in animation transition diagnostics and CPU spans with `flui-animation/diagnostics`, forwarded as `animation-diagnostics` by the SDK and facade. Diagnostics correlate controller lifetimes and run outcomes without adding observer callbacks.
- Devtools profiling of animation registry ticks, source sampling and listener/future delivery. The `profiling` feature enables animation instrumentation; timings are inclusive and restricted to the owning scheduler frame.

### Fixed

- Include headless animation sampling inside its scheduler frame so actual animation work reaches the frame profiler.
- Keep animation and frame timings bounded by their outermost entered visits, including retained span clones and same-span reentry; application spans named `frame` do not hide scheduler work.
- Preserve cancellation delivery and silent value listeners when an optional animation subscriber disposes the owning controller during listener-span creation or entry.
