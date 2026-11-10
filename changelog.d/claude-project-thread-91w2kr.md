### Added

- **`flui-testing`**: `motion`, a gate that records what the window shows each frame and fails
  on motion a person would notice (pop-in, pop-out, jump, fade-snap, flicker, overshoot,
  never-settles, wakes-after-end), with an allowlist whose rows need a reason and fail when they
  match nothing ([ADR-0321](/docs/adr/ADR-0321-a-motion-gate-reads-what-the-window-shows-each-frame.md)).
- **`flui-rendering`** (feature `testing`): `PipelineOwner::presented_nodes`, each node's box,
  visible part and opacity as the window shows them.
