### Changed

- `flui-rendering`: a frame phase is a `FramePass<'_, Phase>` that borrows the `PipelineOwner` it runs on (ADR-0399). `owner.layout_pass()` starts a frame by hand, `into_compositing`, `into_paint` and `into_semantics` hand the borrow on, and each `run_*` is a method of its own pass. A frame no longer copies the owner through its phases.
- `PipelineOwner` has no phase parameter, and `PipelineOwner::run_frame(&mut self, writer)` runs every pass in place, answering whether it painted.
- `FramePass<Compositing>::run_compositing` returns `()`: the compositing walk has no error to report.
- The testing kit's `SceneHost::run_frame` takes `&mut PipelineOwner`.

### Removed

- The `Idle` phase marker and `PhaseMarker`: an idle owner is one no pass borrows.
- `PipelineOwner::into_layout`, `into_idle`, `Semantics`'s `finish`, `run_frame_in_place` and the by-value `run_frame(self)`. A pass ends when it is dropped.
