### Added

- `Scene::of_version` holds a retained `SceneVersion`, which the engine draws in place, with no
  layer tree built. `Scene::content`, `Scene::tree` and `Scene::version` say which kind the scene
  holds. A partial frame skips every boundary its damage cannot reach.
- `SceneVersion::subtree_meets` answers whether a node's subtree can reach a rect on the surface.
- `SceneVersion::close_damage` closes a damage region over what the version's filters and
  destination-reading chunks read.
- `EntryIndex` converts from a position (`TryFrom<usize>`).

### Changed

- `Scene::tree` now returns `Option<&LayerTree>`, and `Scene::root` is removed.
- `SceneSnapshot::of_version` no longer flattens the version, and `SceneSnapshot` has no separate
  `version` field. Its `scene` holds the version.
