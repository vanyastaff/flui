### Added

- **`flui-rendering`**: `PipelineCell::watch_leader(link, lane)` returns a `LeaderWatch`, a
  `Listenable` whose `geometry()` is the leader's size in the last painted frame. Listeners run
  in the post-frame lane only when that size changed, so a follower can rebuild from its
  leader's layout without a per-frame check, and a steady layout requests no frame.
- **`flui-layer`**: `LayerTree::leaders()` iterates the frame's leaders with their links.

### Fixed

- **`flui-widgets`**: `SelectionArea`'s touch handles resize to their end's line after a
  relayout that rebuilt neither the area nor its text, such as a narrower width wrapping a tall
  span off the line.
