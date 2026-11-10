### Added

- **`flui-testing`**: `motion`, a gate that records what the window shows each frame and fails
  on motion a person would notice (pop-in, pop-out, jump, fade-snap, flicker, overshoot,
  never-settles, wakes-after-end), with an allowlist whose rows need a reason and fail when they
  match nothing ([ADR-0321](/docs/adr/ADR-0321-a-motion-gate-reads-what-the-window-shows-each-frame.md)).
- **`flui-rendering`** (feature `testing`): `PipelineOwner::presented_nodes`, each node's box,
  visible part and opacity as the window shows them, a child's opacity including the paint
  scope its parent drew it in.
- **`flui-rendering`**: `RenderObject::drawn_transform` (and its `RenderBox` twin), the
  transform a node's own drawing is placed under; `RenderContainer` reports the translation it
  paints as an offset.

### Fixed

- **`flui-rendering`**: the input-region walk visits only the children a parent painted in the
  last frame, so a child a parent leaves out of paint without also leaving it out of semantics
  no longer reports input regions.
- **`flui-testing`**: `motion::gate` refuses a `MotionConfig` whose threshold a rule cannot
  compare against (a NaN or negative distance, an opacity outside `0.0..=1.0`), listed in
  `MotionReport::invalid_config`, instead of turning that rule off silently.
