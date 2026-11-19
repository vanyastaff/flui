### Added

- `SceneVersion::from_tree` makes a version of a hand-built `LayerTree`; `LayerTree` and
  `LayerNode` are `Clone`.

### Changed

- A `Scene` holds one `SceneVersion`: `Scene::new(tree)` converts the tree and `Scene::version`
  returns the version. The engine draws only versions, so a hand-built scene and a pipeline's
  scene take one walk and one damage rule.
- `LayerDiffer::diff` takes the `&LayerTree` it compares.
- `close_over`, `read_closure`, `Closure`, `Closures` and `MAX_CLOSURE_ROUNDS` are exported only
  with the `testing` feature: the engine closes damage through `SceneVersion::close_damage`.

### Removed

- `Scene::tree`: a scene no longer carries a tree.
- `flui_app::run_direct` (`flui::app::run_direct`): a hand-built scene runs as a hot-reload scene
  plugin or is painted by the application's own render objects.
