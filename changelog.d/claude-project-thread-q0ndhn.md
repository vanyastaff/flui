### Added

- **`flui-layer`**: `LeaderClip` on `FollowerLayer` (`with_leader_clip`) and on
  `RenderFollowerLayer`. `LeaderClip::Hide` hides a follower, and stops its hits, while its
  leader's anchor is clipped out, such as a selection handle whose end scrolled away; the default
  `Escape` keeps tooltips and menus visible past their leader's clips
  ([ADR-0276](/docs/adr/ADR-0276-a-follower-takes-its-leaders-full-transform-and-may-hide-with-its-clips.md)).

### Changed

- **`flui-layer`**: `resolve_follower_offset` is now `resolve_follower_transform` and returns a
  `Matrix4`; `FollowerLayer::calculate_offset` is now `linked_offset(leader_size)`.
  `PaintCx::with_follower` takes a `FollowerLayer`.

### Fixed

- **`flui-layer`, `flui-engine`, `flui-rendering`**: a follower under a scaled or rotated leader
  lands on it and takes its scale and rotation, and a leader inside another follower resolves
  through it. Painting, damage, hit-testing and input regions all read the same transform; a
  singular chain paints nothing instead of a degenerate transform.
- **`flui-widgets`**: a `SelectionArea` touch handle whose end scrolls out of its viewport no
  longer paints over the content around it or takes touches there.
