### Added

- `PipelineOwner::run_frame_in_place`, a frame on an owner held behind `&mut`, and `PipelineCell::run_layout`, the layout pass of the build-during-layout fixpoint. Both leave the same owner in place on every exit, a panic included.

### Fixed

- A panic out of a frame phase (a `tracing` subscriber, a visual-update callback, a render object's `always_needs_compositing`) no longer replaces a presentation's render tree with an empty pipeline. The owner stays in its `PipelineCell` with the layout, compositing and paint work the frame had not finished, so the automatic retry repaints at unchanged constraints and invalidation handles keep reaching it.
- A render object whose `always_needs_compositing` panics keeps its previous compositing answer, so the retry still repaints for a compositing change.
