### Changed

- `SceneBuilder::build` returns a `Scene`, as `dart:ui`'s does, and `push`, `add`, `pop` and the
  typed `push_*`/`add_picture` return the builder for chaining instead of layer ids. A caller that
  keeps ids builds a `LayerTree` with `push_child` and converts it with `Scene::new`.
  `SceneBuilder::with_root`, `current`, `root`, `depth` and `tree` are gone.
- `flui_layer::resolve_follower_transform`, `leader_origin` and `LayerTree::find_annotations` /
  `find_annotation` are behind the `testing` feature, as the oracles of the retained scene's own
  reads: a follower resolves with `SceneVersion::follower_transform`, a leader with
  `SceneVersion::leader_origin` and an annotation with `SceneVersion::find_annotation`.
