### Added

- `Scene::of_version` holds a retained `SceneVersion`, which the engine draws in place, with no
  layer tree built. `Scene::content`, `Scene::tree` and `Scene::version` say which kind the scene
  holds. A partial frame skips every boundary its damage cannot reach.
- `SceneVersion::subtree_cull` and `SubtreeCull` answer, for each slot a walk meets, whether the
  subtree it holds can reach a rect on the surface, at the cost of the effects above the slot
  rather than the depth of the tree.
- `HeadlessRenderer::render_scene` rasterizes a `Scene`, a retained version included.
- `SceneVersion::close_damage` closes a damage region over what the version's filters and
  destination-reading chunks read.
- `EntryIndex` converts from a position (`TryFrom<usize>`).

### Changed

- `Scene::tree` now returns `Option<&LayerTree>`, and `Scene::root` is removed.
- `SceneSnapshot::of_version` no longer flattens the version, and `SceneSnapshot` has no separate
  `version` field. Its `scene` holds the version.

### Fixed

- A partial frame of a retained version no longer forces a full repaint for a destination-reading
  chunk cut by a clip over it.
