### Added

- `flui_foundation::diagnostics::FrameSpan` provides the shared typed `(target, name)` contract for frame, presentation, pipeline, layout-root and animation spans, together with shared `runtime`, `presentation` and `period_us` field names. Public tests pin the complete wire table and exact classification.

### Changed

- Scheduler, runtime, view, rendering and animation producers now emit through `FrameSpan`; devtools capture, profiling, timeline and trace export classify the same identities and retain them as `CapturedSpan::kind`. `LayoutRoot` remains diagnostic detail rather than a captured profiler phase.
- Devtools subscriber examples and documentation use `tracing-subscriber` directly through the SDK package surface rather than the internal logging composition root.

### Removed

- The empty `flui_sdk::hooks` module, its `FrameSnapshot` re-export, the SDK's direct `flui-scheduler` dependency and `Timeline::record_frame_snapshots` are removed. Scheduler snapshots remain internal to the runtime overlay, histogram and structured-event consumers.
